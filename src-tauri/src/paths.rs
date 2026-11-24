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
