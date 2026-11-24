use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::fetch_verified;
use crate::error::{AppError, AppResult};
use crate::extract::extract;
use crate::paths::{unix_now, validate_name, validate_version, write_atomic, Layout};
use crate::registry::{platform_key, ModuleManifest, ServiceSpec};

const META_FILE: &str = ".stackyard-module.json";

/// Snapshot of what was installed, so services run without the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledModule {
    pub name: String,
    pub version: String,
    pub category: String,
    pub bin_dir: String,
    pub service: Option<ServiceSpec>,
    pub config_templates: BTreeMap<String, String>,
    pub installed_at: u64,
}

pub fn list_installed(layout: &Layout) -> Vec<InstalledModule> {
    let mut out = Vec::new();
    let Ok(names) = std::fs::read_dir(layout.modules()) else { return out };
    for name in names.flatten() {
        let Ok(versions) = std::fs::read_dir(name.path()) else { continue };
        for v in versions.flatten() {
            if let Some(m) = read_meta(&v.path().join(META_FILE)) {
                out.push(m);
            }
        }
    }
    out.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| crate::registry::compare_versions(&b.version, &a.version))
    });
    out
}

fn read_meta(path: &Path) -> Option<InstalledModule> {
    let m: InstalledModule = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (validate_name(&m.name).is_ok() && validate_version(&m.version).is_ok()).then_some(m)
}

pub fn load_installed(layout: &Layout, name: &str, version: &str) -> AppResult<InstalledModule> {
    validate_name(name)?;
    validate_version(version)?;
    read_meta(&layout.module_dir(name, version).join(META_FILE))
        .filter(|m| m.name == name && m.version == version)
        .ok_or_else(|| AppError::NotFound(format!("{name}@{version}")))
}

/// Default port; on macOS/Linux privileged ports fall back to +8000
/// (80 → 8080, 443 → 8443) until the admin helper exists (PRD §7).
pub fn default_port(spec: &ServiceSpec) -> Option<u16> {
    spec.default_port.map(|p| {
        if cfg!(unix) && p < 1024 {
            p + 8000
        } else {
            p
        }
    })
}

/// Placeholder values for `{{var}}` in templates and service args.
pub fn template_vars(layout: &Layout, m: &InstalledModule, port: Option<u16>) -> Vec<(&'static str, String)> {
    let module_dir = layout.module_dir(&m.name, &m.version);
    let bin = if m.bin_dir.is_empty() {
        module_dir.clone()
    } else {
        module_dir.join(&m.bin_dir)
    };
    let p = |path: PathBuf| path.to_string_lossy().replace('\\', "/");
    vec![
        ("bin", p(bin)),
        ("prefix", p(module_dir)),
        ("conf", p(layout.conf_dir(&m.name, &m.version))),
        ("data", p(layout.data_dir(&m.name, &m.version))),
        ("run", p(layout.run_dir(&m.name, &m.version))),
        ("logs", p(layout.logs())),
        ("www", p(layout.www())),
        ("name", m.name.clone()),
        ("version", m.version.clone()),
        ("port", port.map(|p| p.to_string()).unwrap_or_default()),
    ]
}

pub fn render(template: &str, vars: &[(&str, String)]) -> String {
    vars.iter().fold(template.to_string(), |acc, (k, v)| {
        acc.replace(&format!("{{{{{k}}}}}"), v)
    })
}

/// Generates config files. With `overwrite`, a file that differs from the new
/// render is backed up first so user edits are never silently lost (PRD §8);
/// without it, only missing files are written.
pub fn write_configs(layout: &Layout, m: &InstalledModule, port: Option<u16>, overwrite: bool) -> AppResult<()> {
    for dir in [
        layout.conf_dir(&m.name, &m.version),
        layout.data_dir(&m.name, &m.version),
        layout.run_dir(&m.name, &m.version),
        layout.logs(),
    ] {
        std::fs::create_dir_all(dir)?;
    }
    let vars = template_vars(layout, m, port);
    let conf = layout.conf_dir(&m.name, &m.version);
    for (file, tpl) in &m.config_templates {
        let target = conf.join(file);
        let rendered = render(tpl, &vars);
        match std::fs::read_to_string(&target) {
            Ok(existing) if existing == rendered || !overwrite => continue,
            Ok(_) => {
                std::fs::copy(&target, conf.join(format!("{file}.bak-{}", unix_now())))?;
            }
            Err(_) => {}
        }
        write_atomic(&target, rendered.as_bytes())?;
    }
    Ok(())
}

pub async fn install(
    client: &reqwest::Client,
    layout: &Layout,
    manifest: &ModuleManifest,
    version: &str,
    port_for: impl FnOnce(&ServiceSpec) -> Option<u16>,
    on_progress: impl FnMut(u64, Option<u64>),
) -> AppResult<InstalledModule> {
    validate_version(version)?;
    let entry = manifest
        .versions
        .iter()
        .find(|v| v.version == version)
        .ok_or_else(|| AppError::NotFound(format!("{}@{version}", manifest.name)))?;
    let asset = entry
        .assets
        .get(&platform_key())
        .ok_or(AppError::UnsupportedPlatform)?
        .clone();

    let final_dir = layout.module_dir(&manifest.name, version);
    if final_dir.join(META_FILE).exists() {
        return Err(AppError::AlreadyInstalled);
    }

    let archive = fetch_verified(client, &asset.url, &asset.sha256, &layout.downloads(), on_progress).await?;

    let meta = InstalledModule {
        name: manifest.name.clone(),
        version: version.to_string(),
        category: manifest.category.clone(),
        bin_dir: asset.bin_dir.clone(),
        service: manifest.service.clone(),
        config_templates: manifest.config_templates.clone(),
        installed_at: unix_now(),
    };
    let staging = layout
        .modules()
        .join(&manifest.name)
        .join(format!(".{version}.partial"));
    let meta_json = serde_json::to_vec_pretty(&meta)?;
    let (staging2, final2) = (staging.clone(), final_dir.clone());
    let result = tokio::task::spawn_blocking(move || -> AppResult<()> {
        if staging2.exists() {
            std::fs::remove_dir_all(&staging2)?;
        }
        if final2.exists() {
            std::fs::remove_dir_all(&final2)?; // leftover without metadata
        }
        std::fs::create_dir_all(&staging2)?;
        extract(&archive, &asset.format, &staging2, asset.strip, &asset.bin_name)?;
        std::fs::write(staging2.join(META_FILE), meta_json)?;
        std::fs::rename(&staging2, &final2)?;
        // Downloaded archive is no longer needed once extracted.
        let _ = std::fs::remove_file(&archive);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    let port = meta.service.as_ref().and_then(port_for);
    write_configs(layout, &meta, port, true)?;
    Ok(meta)
}

/// Removes binaries and generated config; database data is kept unless asked.
pub fn uninstall(layout: &Layout, name: &str, version: &str, delete_data: bool) -> AppResult<()> {
    load_installed(layout, name, version)?;
    std::fs::remove_dir_all(layout.module_dir(name, version))?;
    for dir in [layout.conf_dir(name, version), layout.run_dir(name, version)] {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
    }
    if delete_data && layout.data_dir(name, version).exists() {
        std::fs::remove_dir_all(layout.data_dir(name, version))?;
    }
    for parent in [
        layout.modules().join(name),
        layout.root.join("etc").join(name),
        layout.root.join("data").join(name),
    ] {
        let _ = std::fs::remove_dir(parent); // only succeeds when empty
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(layout: &Layout) -> InstalledModule {
        let m = InstalledModule {
            name: "demo".into(),
            version: "1.0.0".into(),
            category: "tools".into(),
            bin_dir: "bin".into(),
            service: None,
            config_templates: BTreeMap::from([("demo.conf".into(), "listen {{port}};\nroot {{www}};\n".into())]),
            installed_at: 0,
        };
        let dir = layout.module_dir(&m.name, &m.version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(META_FILE), serde_json::to_vec(&m).unwrap()).unwrap();
        m
    }

    #[test]
    fn render_replaces_known_vars_only() {
        let vars = vec![("port", "8080".to_string())];
        assert_eq!(render("server { listen {{port}}; }", &vars), "server { listen 8080; }");
        assert_eq!(render("{{unknown}}", &vars), "{{unknown}}");
    }

    #[test]
    fn configs_backup_and_uninstall_keeps_data() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::new(tmp.path().to_path_buf());
        layout.ensure().unwrap();
        let m = sample(&layout);
        assert_eq!(list_installed(&layout).len(), 1);

        write_configs(&layout, &m, Some(8080), true).unwrap();
        let conf = layout.conf_dir("demo", "1.0.0");
        assert!(std::fs::read_to_string(conf.join("demo.conf")).unwrap().contains("listen 8080;"));

        std::fs::write(conf.join("demo.conf"), "user edit").unwrap();
        write_configs(&layout, &m, Some(9090), false).unwrap();
        assert_eq!(std::fs::read_to_string(conf.join("demo.conf")).unwrap(), "user edit");
        write_configs(&layout, &m, Some(9090), true).unwrap();
        let backups = std::fs::read_dir(&conf)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("demo.conf.bak-"))
            .count();
        assert_eq!(backups, 1);

        std::fs::write(layout.data_dir("demo", "1.0.0").join("db"), "x").unwrap();
        uninstall(&layout, "demo", "1.0.0", false).unwrap();
        assert!(list_installed(&layout).is_empty());
        assert!(layout.data_dir("demo", "1.0.0").join("db").exists());
        assert!(!conf.exists());
    }
}
