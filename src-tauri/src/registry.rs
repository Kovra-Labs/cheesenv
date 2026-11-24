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
}

/// Process definition. Args are passed directly to the OS (never via a shell).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSpec {
    pub start: Vec<String>,
    #[serde(default)]
    pub start_windows: Option<Vec<String>>,
    #[serde(default)]
    pub default_port: Option<u16>,
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
            validate_remote_url(&a.url)?;
            if a.sha256.len() != 64 || !a.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(AppError::InvalidInput("asset sha256".into()));
            }
            if !matches!(a.format.as_str(), "zip" | "tar.gz" | "binary") {
                return Err(AppError::InvalidInput("asset format".into()));
            }
            if a.format == "binary" && !is_plain_file_name(&a.bin_name) {
                return Err(AppError::InvalidInput("asset bin_name".into()));
            }
            if !is_safe_relative(&a.bin_dir) {
                return Err(AppError::InvalidInput("asset bin_dir".into()));
            }
        }
    }
    for file in m.config_templates.keys() {
        if !is_plain_file_name(file) {
            return Err(AppError::InvalidInput("template name".into()));
        }
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

    #[test]
    fn rejects_bad_signature() {
        assert!(verify_signature(b"x", "untrusted comment: x\nnot-base64").is_err());
    }
}
