use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Data folder layout (PRD §6): modules/, etc/, data/, www/, logs/, certs/, cache/.
#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn modules(&self) -> PathBuf {
        self.root.join("modules")
    }
    pub fn module_dir(&self, name: &str, version: &str) -> PathBuf {
        self.modules().join(name).join(version)
    }
    pub fn conf_dir(&self, name: &str, version: &str) -> PathBuf {
        self.root.join("etc").join(name).join(version)
    }
    pub fn data_dir(&self, name: &str, version: &str) -> PathBuf {
        self.root.join("data").join(name).join(version)
    }
    pub fn run_dir(&self, name: &str, version: &str) -> PathBuf {
        self.cache().join("run").join(name).join(version)
    }
    pub fn www(&self) -> PathBuf {
        self.root.join("www")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn certs(&self) -> PathBuf {
        self.root.join("certs")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn downloads(&self) -> PathBuf {
        self.cache().join("downloads")
    }
    pub fn registry_cache(&self) -> PathBuf {
        self.cache().join("registry")
    }

    /// Unix sockets are limited to ~104-108 bytes, so they live in a short
    /// per-user runtime dir instead of the (possibly long) data folder. The
    /// hash keeps two data folders from sharing sockets.
    pub fn socket_dir(&self, name: &str, version: &str) -> PathBuf {
        use sha2::Digest;
        let hash = hex::encode(&sha2::Sha256::digest(self.root.to_string_lossy().as_bytes())[..4]);
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            // A long runtime dir would push socket paths past the limit again.
            .filter(|p| p.is_absolute() && p.is_dir() && p.as_os_str().len() <= 48)
            .unwrap_or_else(std::env::temp_dir);
        base.join(format!("stackyard-{hash}")).join(format!("{name}-{version}"))
    }

    /// Creates the socket dir as private (0700) and refuses one that another
    /// user planted (e.g. a pre-created dir or symlink in a shared /tmp).
    pub fn ensure_socket_dir(&self, name: &str, version: &str) -> AppResult<PathBuf> {
        let dir = self.socket_dir(name, version);
        let parent = dir.parent().unwrap_or(&dir).to_path_buf();
        for d in [&parent, &dir] {
            match std::fs::symlink_metadata(d) {
                Ok(meta) => check_private_dir(d, &meta)?,
                Err(_) => create_private_dir(d)?,
            }
        }
        Ok(dir)
    }

    pub fn ensure(&self) -> AppResult<()> {
        for dir in [
            self.modules(),
            self.root.join("etc"),
            self.root.join("data"),
            self.www(),
            self.logs(),
            self.certs(),
            self.downloads(),
            self.registry_cache(),
        ] {
            std::fs::create_dir_all(&dir)?;
        }
        let index = self.www().join("index.html");
        if !index.exists() {
            std::fs::write(index, WELCOME_PAGE)?;
        }
        Ok(())
    }
}

const WELCOME_PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<title>Stackyard</title></head><body style=\"font-family:system-ui;max-width:40rem;margin:4rem auto\">\
<h1>It works!</h1><p>This page is served by Stackyard from the <code>www</code> folder.</p></body></html>\n";

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> AppResult<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(dir)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dir)?;
    Ok(())
}

#[cfg(unix)]
fn check_private_dir(dir: &Path, meta: &std::fs::Metadata) -> AppResult<()> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(AppError::InvalidInput(format!("unsafe socket dir {}", dir.display())));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private_dir(_dir: &Path, _meta: &std::fs::Metadata) -> AppResult<()> {
    Ok(())
}

/// Module names: lowercase ASCII, digits and dashes (used as path segments).
pub fn validate_name(name: &str) -> AppResult<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    ok.then_some(())
        .ok_or_else(|| AppError::InvalidInput(format!("module name '{name}'")))
}

/// Versions: alphanumerics plus `.+-`, never `..` (used as path segments).
pub fn validate_version(version: &str) -> AppResult<()> {
    let ok = !version.is_empty()
        && version.len() <= 64
        && version.starts_with(|c: char| c.is_ascii_alphanumeric())
        && !version.contains("..")
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'));
    ok.then_some(())
        .ok_or_else(|| AppError::InvalidInput(format!("version '{version}'")))
}

/// Service id is `name@version`.
pub fn parse_service_id(id: &str) -> AppResult<(String, String)> {
    let (name, version) = id
        .split_once('@')
        .ok_or_else(|| AppError::InvalidInput(format!("service id '{id}'")))?;
    validate_name(name)?;
    validate_version(version)?;
    Ok((name.to_string(), version.to_string()))
}

pub fn service_id(name: &str, version: &str) -> String {
    format!("{name}@{version}")
}

/// Write via temp file + rename so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let tmp = path.with_extension("tmp-write");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn socket_dir_is_short_private_and_rejects_foreign() {
        use std::os::unix::fs::PermissionsExt;
        let long = PathBuf::from("/home/someone/").join("x".repeat(150));
        let a = Layout::new(long).socket_dir("postgresql", "18.7.0");
        assert!(a.join(".s.PGSQL.55432").to_string_lossy().len() < 100);
        let b = Layout::new("/other".into()).socket_dir("postgresql", "18.7.0");
        assert_ne!(a, b);
        std::env::set_var("XDG_RUNTIME_DIR", "/".to_string() + &"r".repeat(120));
        let c = Layout::new("/x".into()).socket_dir("postgresql", "18.7.0");
        assert!(c.starts_with(std::env::temp_dir()), "long runtime dir falls back to temp");

        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::new(tmp.path().join("root"));
        let dir = layout.ensure_socket_dir("demo", "1.0.0").unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(layout.ensure_socket_dir("demo", "1.0.0").is_err(), "world-writable dir refused");
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn names_and_versions() {
        assert!(validate_name("php").is_ok());
        assert!(validate_name("node-lts").is_ok());
        assert!(validate_name("../etc").is_err());
        assert!(validate_name("PHP").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_version("8.3.14").is_ok());
        assert!(validate_version("1.0.0-rc1+b2").is_ok());
        assert!(validate_version("..").is_err());
        assert!(validate_version("1/2").is_err());
        assert!(validate_version(".hidden").is_err());
        assert!(parse_service_id("nginx@1.30.5").is_ok());
        assert!(parse_service_id("nginx").is_err());
        assert!(parse_service_id("nginx@../x").is_err());
    }
}
