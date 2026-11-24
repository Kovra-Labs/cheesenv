use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::paths::{validate_name, validate_version, write_atomic, Layout};
use crate::settings::validate_remote_url;

/// Registry signing key, embedded at build time (PRD §8).
const PUBLIC_KEY: &str = include_str!("../registry.pub");
const INDEX_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub schema: u32,
    pub updated: String,
    pub modules: Vec<IndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub description: BTreeMap<String, String>,
    pub latest: String,
    /// SHA-256 of `modules/<name>.json`; chains manifest trust to the signed index.
    pub manifest_sha256: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub homepage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    pub name: String,
    pub category: String,
    pub versions: Vec<VersionEntry>,
    #[serde(default)]
    pub service: Option<ServiceSpec>,
    #[serde(default)]
    pub config_templates: BTreeMap<String, String>,
    /// Install-time step run inside the module (e.g. `pip install`).
    #[serde(default)]
    pub setup: Option<InitSpec>,
    /// Files rendered into the module folder at install, keyed by relative path.
    #[serde(default)]
    pub module_files: BTreeMap<String, String>,
    /// Present for web panels that manage another module (e.g. phpMyAdmin).
    #[serde(default)]
    pub panel: Option<PanelSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelSpec {
    /// Module whose services the panel manages.
    #[serde(rename = "for")]
    pub target: String,
    /// URL path opened in the browser; may contain `{{token}}`.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub version: String,
    pub assets: BTreeMap<String, Asset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub url: String,
    pub sha256: String,
    /// `zip`, `tar.gz` or `binary` (a single executable).
    pub format: String,
    /// Leading path components dropped while extracting.
    #[serde(default)]
    pub strip: usize,
    /// Directory (relative to the module root) holding executables.
    #[serde(default)]
    pub bin_dir: String,
    /// File name for `binary` assets.
    #[serde(default)]
    pub bin_name: String,
    /// More archives extracted into sub-folders of the module.
    #[serde(default)]
    pub extra: Vec<ExtraAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtraAsset {
    pub url: String,
    pub sha256: String,
    /// `zip`, `tar.gz` or `tar.xz`.
    pub format: String,
    #[serde(default)]
    pub strip: usize,
    /// Target folder relative to the module root.
    pub dest: String,
}

/// Process definition. Args are passed directly to the OS (never via a shell).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSpec {
    pub start: Vec<String>,
    #[serde(default)]
    pub start_windows: Option<Vec<String>>,
    #[serde(default)]
    pub default_port: Option<u16>,
    /// One-time data initialisation (e.g. `initdb`), run before the first start.
    #[serde(default)]
    pub init: Option<InitSpec>,
    /// Present for databases: a random root password is generated at init.
    #[serde(default)]
    pub credentials: Option<CredentialSpec>,
}

/// `files` are rendered (with `{{password}}`) into a private temp folder,
/// exposed as `{{init}}`, and deleted once `commands` finish.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InitSpec {
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    pub commands: Vec<Vec<String>>,
    #[serde(default)]
    pub commands_windows: Option<Vec<Vec<String>>>,
}

impl InitSpec {
    pub fn commands(&self) -> &[Vec<String>] {
        if cfg!(windows) {
            self.commands_windows.as_deref().unwrap_or(&self.commands)
        } else {
            &self.commands
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSpec {
    pub user: String,
}

impl ServiceSpec {
    pub fn args(&self) -> &[String] {
        if cfg!(windows) {
            self.start_windows.as_deref().unwrap_or(&self.start)
        } else {
            &self.start
        }
    }
}

/// `<os>-<arch>` key used in manifest assets.
pub fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn verify_signature(data: &[u8], sig_text: &str) -> AppResult<()> {
    let pk = PublicKey::decode(PUBLIC_KEY.trim())
        .map_err(|e| AppError::Signature(format!("bad embedded key: {e}")))?;
    let sig = Signature::decode(sig_text).map_err(|e| AppError::Signature(e.to_string()))?;
    pk.verify(data, &sig, false)
        .map_err(|e| AppError::Signature(e.to_string()))
}

fn validate_index(index: &RegistryIndex) -> AppResult<()> {
    if index.schema != 1 {
        return Err(AppError::Signature(format!("unsupported schema {}", index.schema)));
    }
    for m in &index.modules {
        validate_name(&m.name)?;
        validate_version(&m.latest)?;
    }
    Ok(())
}

fn validate_manifest(m: &ModuleManifest, expected_name: &str) -> AppResult<()> {
    if m.name != expected_name {
        return Err(AppError::InvalidInput("manifest name mismatch".into()));
    }
    for v in &m.versions {
        validate_version(&v.version)?;
        for a in v.assets.values() {
            validate_download(&a.url, &a.sha256)?;
            if !matches!(a.format.as_str(), "zip" | "tar.gz" | "tar.xz" | "binary") {
                return Err(AppError::InvalidInput("asset format".into()));
            }
            if a.format == "binary" && !is_plain_file_name(&a.bin_name) {
                return Err(AppError::InvalidInput("asset bin_name".into()));
            }
            if !is_safe_relative(&a.bin_dir) {
                return Err(AppError::InvalidInput("asset bin_dir".into()));
            }
            for x in &a.extra {
                validate_download(&x.url, &x.sha256)?;
                if !matches!(x.format.as_str(), "zip" | "tar.gz" | "tar.xz") {
                    return Err(AppError::InvalidInput("asset format".into()));
                }
                if x.dest.is_empty() || !is_safe_relative(&x.dest) {
                    return Err(AppError::InvalidInput("asset dest".into()));
                }
            }
        }
    }
    let steps = || m.service.iter().flat_map(|s| s.init.iter()).chain(m.setup.iter());
    for file in m.config_templates.keys().chain(steps().flat_map(|i| i.files.keys())) {
        if !is_plain_file_name(file) {
            return Err(AppError::InvalidInput("template name".into()));
        }
    }
    if m.module_files.keys().any(|f| f.is_empty() || !is_safe_relative(f)) {
        return Err(AppError::InvalidInput("module file".into()));
    }
    let commands = steps().flat_map(|i| i.commands.iter().chain(i.commands_windows.iter().flatten()));
    if commands.clone().any(|c| c.is_empty()) {
        return Err(AppError::InvalidInput("empty command".into()));
    }
    if let Some(panel) = &m.panel {
        validate_name(&panel.target)?;
        if !panel.path.starts_with('/') || panel.path.chars().any(|c| c.is_whitespace() || c == '\\') {
            return Err(AppError::InvalidInput("panel path".into()));
        }
        if m.service.as_ref().is_none_or(|s| s.default_port.is_none()) {
            return Err(AppError::InvalidInput("panel without port".into()));
        }
    }
    if let Some(service) = &m.service {
        if service.start.is_empty() || service.start_windows.as_ref().is_some_and(|s| s.is_empty()) {
            return Err(AppError::InvalidInput("empty command".into()));
        }
        let user_ok = service.credentials.as_ref().is_none_or(|c| {
            !c.user.is_empty() && c.user.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        });
        if !user_ok {
            return Err(AppError::InvalidInput("credentials user".into()));
        }
    }
    Ok(())
}

fn validate_download(url: &str, sha256: &str) -> AppResult<()> {
    validate_remote_url(url)?;
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::InvalidInput("asset sha256".into()));
    }
    Ok(())
}

pub fn is_plain_file_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.contains(['/', '\\', ':', '\0'])
}

fn is_safe_relative(s: &str) -> bool {
    s.is_empty() || s.split('/').all(is_plain_file_name)
}

pub struct Registry<'a> {
    pub client: &'a reqwest::Client,
    pub base_url: &'a str,
    pub layout: &'a Layout,
}

pub struct LoadedIndex {
    pub index: RegistryIndex,
    pub fetched_at: u64,
    /// True when the network failed and a previously verified cache was used.
    pub stale: bool,
}

impl Registry<'_> {
    async fn get_bytes(&self, path: &str, limit: usize) -> AppResult<Vec<u8>> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), path);
        let resp = self.client.get(&url).send().await?.error_for_status()?;
        let bytes = resp.bytes().await?;
        if bytes.len() > limit {
            return Err(AppError::Network("response too large".into()));
        }
        Ok(bytes.to_vec())
    }

    /// Loads the signed index: cache if fresh, otherwise network; falls back to
    /// the cache when offline. Both paths are signature-verified.
    pub async fn index(&self, force: bool) -> AppResult<LoadedIndex> {
        let dir = self.layout.registry_cache();
        let idx_path = dir.join("index.json");
        let sig_path = dir.join("index.json.minisig");
        let cached_age = std::fs::metadata(&idx_path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok());

        if !force && cached_age.is_some_and(|a| a < INDEX_TTL) {
            if let Ok(loaded) = load_cached(&idx_path, &sig_path, false) {
                return Ok(loaded);
            }
        }

        match self.fetch_index().await {
            Ok((data, sig)) => {
                std::fs::create_dir_all(&dir)?;
                write_atomic(&sig_path, sig.as_bytes())?;
                write_atomic(&idx_path, &data)?;
                load_cached(&idx_path, &sig_path, false)
            }
            Err(e @ AppError::Signature(_)) => Err(e),
            Err(e) => load_cached(&idx_path, &sig_path, true).map_err(|_| e),
        }
    }

    async fn fetch_index(&self) -> AppResult<(Vec<u8>, String)> {
        let data = self.get_bytes("index.json", MAX_INDEX_BYTES).await?;
        let sig = self.get_bytes("index.json.minisig", 4096).await?;
        let sig = String::from_utf8(sig).map_err(|_| AppError::Signature("sig utf8".into()))?;
        verify_signature(&data, &sig)?;
        Ok((data, sig))
    }

    /// Fetches `modules/<name>.json`, trusted via the hash in the signed index.
    pub async fn manifest(&self, entry: &IndexEntry) -> AppResult<ModuleManifest> {
        validate_name(&entry.name)?;
        let cache = self
            .layout
            .registry_cache()
            .join("modules")
            .join(format!("{}.json", entry.name));
        let expected = entry.manifest_sha256.to_ascii_lowercase();

        let bytes = match std::fs::read(&cache) {
            Ok(b) if sha256_hex(&b) == expected => b,
            _ => {
                let b = self
                    .get_bytes(&format!("modules/{}.json", entry.name), MAX_INDEX_BYTES)
                    .await?;
                if sha256_hex(&b) != expected {
                    return Err(AppError::Signature("manifest hash mismatch".into()));
                }
                std::fs::create_dir_all(cache.parent().unwrap_or(Path::new(".")))?;
                write_atomic(&cache, &b)?;
                b
            }
        };
        let manifest: ModuleManifest = serde_json::from_slice(&bytes)?;
        validate_manifest(&manifest, &entry.name)?;
        Ok(manifest)
    }
}

fn load_cached(idx: &Path, sig: &Path, stale: bool) -> AppResult<LoadedIndex> {
    let data = std::fs::read(idx)?;
    let sig_text = std::fs::read_to_string(sig)?;
    verify_signature(&data, &sig_text)?;
    let index: RegistryIndex = serde_json::from_slice(&data)?;
    validate_index(&index)?;
    let fetched_at = std::fs::metadata(idx)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(LoadedIndex { index, fetched_at, stale })
}

/// Numeric-aware version ordering ("1.30.10" > "1.30.9").
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parts(a).cmp(&parts(b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("1.30.10", "1.30.9"), Greater);
        assert_eq!(compare_versions("8.2.0", "8.3.0"), Less);
        assert_eq!(compare_versions("8.3.0", "8.3.0"), Equal);
    }

    #[test]
    fn file_names() {
        assert!(is_plain_file_name("nginx.conf"));
        assert!(!is_plain_file_name("../x"));
        assert!(!is_plain_file_name(".."));
        assert!(!is_plain_file_name("a\\b"));
        assert!(is_safe_relative("bin"));
        assert!(is_safe_relative(""));
        assert!(!is_safe_relative("../bin"));
        assert!(!is_safe_relative("/bin"));
    }

    /// Uses the registry built by `npm run registry build` when present.
    #[test]
    fn verifies_built_index_and_detects_tampering() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../registry/dist");
        let (Ok(data), Ok(sig)) = (
            std::fs::read(dir.join("index.json")),
            std::fs::read_to_string(dir.join("index.json.minisig")),
        ) else {
            return;
        };
        verify_signature(&data, &sig).expect("genuine index verifies");
        let mut tampered = data.clone();
        let i = tampered.iter().position(|b| *b == b'n').unwrap();
        tampered[i] = b'm';
        assert!(matches!(verify_signature(&tampered, &sig), Err(AppError::Signature(_))));
    }

    /// Every source manifest must pass the same validation the app applies.
    #[test]
    fn source_manifests_are_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../registry/modules");
        let mut seen = 0;
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let m: ModuleManifest = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            validate_manifest(&m, &name).unwrap_or_else(|e| panic!("{name}: {e}"));
            seen += 1;
        }
        assert!(seen > 0);
    }

    #[test]
    fn rejects_unsafe_panel_and_module_files() {
        let base = r#"{"name":"p","category":"tools","versions":[],
            "service":{"start":["{{bin}}/x"],"default_port":1}"#;
        let check = |extra: &str| {
            let m: ModuleManifest = serde_json::from_str(&format!("{base},{extra}}}")).unwrap();
            validate_manifest(&m, "p")
        };
        assert!(check(r#""panel":{"for":"mysql","path":"/"}"#).is_ok());
        assert!(check(r#""panel":{"for":"mysql","path":"http://evil"}"#).is_err());
        assert!(check(r#""panel":{"for":"../x","path":"/"}"#).is_err());
        assert!(check(r#""module_files":{"app/a.php":""}"#).is_ok());
        assert!(check(r#""module_files":{"../a.php":""}"#).is_err());
        assert!(check(r#""module_files":{"/etc/a":""}"#).is_err());
        assert!(check(r#""setup":{"commands":[[]]}"#).is_err());
        assert!(check(r#""setup":{"files":{"../r.txt":""},"commands":[["x"]]}"#).is_err());
    }

    #[test]
    fn rejects_bad_signature() {
        assert!(verify_signature(b"x", "untrusted comment: x\nnot-base64").is_err());
    }
}
