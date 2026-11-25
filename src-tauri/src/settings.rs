use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::paths::{parse_service_id, write_atomic};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ServiceSettings {
    pub port: Option<u16>,
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub data_dir: PathBuf,
    pub auto_restart: bool,
    /// Check GitHub Releases for a new app version at startup.
    pub check_updates: bool,
    pub services: BTreeMap<String, ServiceSettings>,
    /// Default version per runtime module (e.g. `node` → `26.13.1`).
    pub runtimes: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "id".into(),
            theme: "system".into(),
            data_dir: default_data_dir(),
            auto_restart: true,
            check_updates: true,
            services: BTreeMap::new(),
            runtimes: BTreeMap::new(),
        }
    }
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Stackyard")
}

/// Partial update sent by the settings page.
#[derive(Debug, Deserialize)]
pub struct SettingsUpdate {
    pub language: Option<String>,
    pub theme: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub auto_restart: Option<bool>,
    pub check_updates: Option<bool>,
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                eprintln!("[stackyard] settings unreadable, using defaults: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> AppResult<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn apply(&mut self, u: SettingsUpdate) -> AppResult<()> {
        if let Some(lang) = u.language {
            if !matches!(lang.as_str(), "id" | "en") {
                return Err(AppError::InvalidInput("language".into()));
            }
            self.language = lang;
        }
        if let Some(theme) = u.theme {
            if !matches!(theme.as_str(), "system" | "light" | "dark") {
                return Err(AppError::InvalidInput("theme".into()));
            }
            self.theme = theme;
        }
        if let Some(dir) = u.data_dir {
            if !dir.is_absolute() {
                return Err(AppError::InvalidInput("data_dir".into()));
            }
            self.data_dir = dir;
        }
        if let Some(v) = u.auto_restart {
            self.auto_restart = v;
        }
        if let Some(v) = u.check_updates {
            self.check_updates = v;
        }
        Ok(())
    }

    pub fn service(&self, id: &str) -> ServiceSettings {
        self.services.get(id).cloned().unwrap_or_default()
    }

    pub fn set_service(&mut self, id: &str, s: ServiceSettings) -> AppResult<()> {
        parse_service_id(id)?;
        if s == ServiceSettings::default() {
            self.services.remove(id);
        } else {
            self.services.insert(id.to_string(), s);
        }
        Ok(())
    }
}

/// Remote URLs must be HTTPS; plain HTTP is only accepted for loopback
/// (local registry development). Integrity still relies on minisign/SHA-256.
pub fn validate_remote_url(url: &str) -> AppResult<()> {
    let ok = if let Some(rest) = url.strip_prefix("https://") {
        !rest.is_empty()
    } else if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        rest.starts_with("[::1]") || matches!(host, "127.0.0.1" | "localhost")
    } else {
        false
    };
    let ok = ok && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then_some(())
        .ok_or_else(|| AppError::InvalidInput("url".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_rules() {
        assert!(validate_remote_url(crate::registry::REGISTRY_URL).is_ok(), "built-in registry URL");
        assert!(validate_remote_url("http://127.0.0.1:8000").is_ok());
        assert!(validate_remote_url("http://localhost/reg").is_ok());
        assert!(validate_remote_url("http://example.com").is_err());
        assert!(validate_remote_url("http://localhost.evil.com").is_err());
        assert!(validate_remote_url("file:///etc").is_err());
        assert!(validate_remote_url("https://a b").is_err());
    }

    #[test]
    fn old_registry_url_setting_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, r#"{"language":"en","registry_url":"https://evil.example"}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.language, "en", "rest of the file still loads");
        s.save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("registry_url"));
    }

    #[test]
    fn apply_validates() {
        let mut s = Settings::default();
        let bad = SettingsUpdate {
            language: Some("fr".into()),
            theme: None,
            data_dir: None,
            auto_restart: None,
            check_updates: None,
        };
        assert!(s.apply(bad).is_err());
        let rel = SettingsUpdate {
            language: None,
            theme: None,
            data_dir: Some("relative".into()),
            auto_restart: None,
            check_updates: None,
        };
        assert!(s.apply(rel).is_err());
    }
}
