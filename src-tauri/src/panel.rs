//! Web panels (phpMyAdmin, pgAdmin) that manage another module's services.
//!
//! Before every start Stackyard writes `{{run}}/panel.json` with the port, a
//! fresh access token and the target services (host, port, user; never
//! passwords). The panel's own config reads it, and Stackyard reads the token
//! back to build the URL it opens.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::modules::{render, InstalledModule};
use crate::paths::{service_id, Layout};
use crate::registry::ServiceSpec;

const PANEL_FILE: &str = "panel.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelTarget {
    /// Module name (e.g. `mysql` or `mariadb`), so the panel can label servers.
    #[serde(default)]
    pub name: String,
    pub version: String,
    pub host: String,
    pub port: u16,
    pub user: String,
}

#[derive(Serialize, Deserialize)]
struct PanelFile {
    port: u16,
    token: String,
    targets: Vec<PanelTarget>,
}

/// Installed services of the module that `m` (a panel) manages.
pub fn targets(
    m: &InstalledModule,
    installed: &[InstalledModule],
    port_for: impl Fn(&str, &ServiceSpec) -> Option<u16>,
) -> Vec<PanelTarget> {
    let Some(panel) = &m.panel else { return Vec::new() };
    installed
        .iter()
        .filter(|t| panel.targets.contains(&t.name))
        .filter_map(|t| {
            let spec = t.service.as_ref()?;
            Some(PanelTarget {
                name: t.name.clone(),
                version: t.version.clone(),
                host: "127.0.0.1".into(),
                port: port_for(&service_id(&t.name, &t.version), spec)?,
                user: spec.credentials.as_ref().map(|c| c.user.clone()).unwrap_or_default(),
            })
        })
        .collect()
}

/// Rewrites `panel.json` with a new random token (so a restart signs everyone out).
pub fn prepare(layout: &Layout, m: &InstalledModule, port: u16, targets: Vec<PanelTarget>) -> AppResult<()> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| AppError::Internal(e.to_string()))?;
    let file = PanelFile { port, token: hex::encode(bytes), targets };
    let path = layout.run_dir(&m.name, &m.version).join(PANEL_FILE);
    crate::secrets::write_private(&path, &serde_json::to_vec_pretty(&file)?)
}

/// URL of a started panel, including its current access token.
pub fn url(layout: &Layout, m: &InstalledModule) -> AppResult<String> {
    let panel = m.panel.as_ref().ok_or(AppError::NoService)?;
    let path = layout.run_dir(&m.name, &m.version).join(PANEL_FILE);
    let file: PanelFile = serde_json::from_slice(&std::fs::read(path).map_err(|_| AppError::NoService)?)?;
    let path = render(&panel.path, &[("token", file.token)]);
    Ok(format!("http://127.0.0.1:{}{path}", file.port))
}

pub fn is_listening(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{CredentialSpec, PanelSpec};
    use std::collections::BTreeMap;

    fn module(name: &str, version: &str, service: Option<ServiceSpec>, panel: Option<PanelSpec>) -> InstalledModule {
        InstalledModule {
            name: name.into(),
            version: version.into(),
            category: "x".into(),
            bin_dir: String::new(),
            service,
            config_templates: BTreeMap::new(),
            panel,
            shims: BTreeMap::new(),
            installed_at: 0,
        }
    }

    fn spec(port: u16, user: Option<&str>) -> ServiceSpec {
        ServiceSpec {
            start: vec!["x".into()],
            start_windows: None,
            default_port: Some(port),
            init: None,
            credentials: user.map(|u| CredentialSpec { user: u.into() }),
        }
    }

    #[test]
    fn targets_prepare_and_url() {
        let panel = module(
            "pgadmin",
            "9.19",
            Some(spec(5050, None)),
            Some(PanelSpec { targets: vec!["postgresql".into()], path: "/?key={{token}}".into() }),
        );
        let installed = vec![
            module("postgresql", "18.7.0", Some(spec(5432, Some("postgres"))), None),
            module("postgresql", "17.12.0", Some(spec(5432, Some("postgres"))), None),
            module("mysql", "8.4.11", Some(spec(3306, Some("root"))), None),
        ];
        let port_for = |id: &str, s: &ServiceSpec| if id == "postgresql@17.12.0" { Some(5433) } else { s.default_port };
        let t = targets(&panel, &installed, port_for);
        assert_eq!(t.iter().map(|t| (t.version.as_str(), t.port)).collect::<Vec<_>>(), [("18.7.0", 5432), ("17.12.0", 5433)]);
        assert!(t.iter().all(|t| t.user == "postgres" && t.host == "127.0.0.1"));

        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::new(tmp.path().to_path_buf());
        assert!(url(&layout, &panel).is_err(), "no URL before the first start");
        prepare(&layout, &panel, 5051, t.clone()).unwrap();
        let first = url(&layout, &panel).unwrap();
        assert!(first.starts_with("http://127.0.0.1:5051/?key="));
        assert_eq!(first.len(), "http://127.0.0.1:5051/?key=".len() + 64);
        prepare(&layout, &panel, 5051, t).unwrap();
        assert_ne!(url(&layout, &panel).unwrap(), first, "token rotates on every start");

        let raw = std::fs::read_to_string(layout.run_dir("pgadmin", "9.19").join(PANEL_FILE)).unwrap();
        assert!(!raw.contains("password"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(layout.run_dir("pgadmin", "9.19").join(PANEL_FILE)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn panel_for_several_modules() {
        // Installed modules written by older versions store a single name.
        let old: PanelSpec = serde_json::from_str(r#"{"for":"mysql","path":"/"}"#).unwrap();
        assert_eq!(old.targets, ["mysql"]);
        let panel = module(
            "phpmyadmin",
            "5.2.3",
            Some(spec(8081, None)),
            Some(serde_json::from_str(r#"{"for":["mysql","mariadb"],"path":"/"}"#).unwrap()),
        );
        let installed = vec![
            module("mysql", "8.4.11", Some(spec(3306, Some("root"))), None),
            module("mariadb", "12.3.4", Some(spec(3307, Some("root"))), None),
            module("postgresql", "18.7.0", Some(spec(5432, Some("postgres"))), None),
        ];
        let t = targets(&panel, &installed, |_, s| s.default_port);
        assert_eq!(
            t.iter().map(|t| (t.name.as_str(), t.port)).collect::<Vec<_>>(),
            [("mysql", 3306), ("mariadb", 3307)]
        );
    }
}
