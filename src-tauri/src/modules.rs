use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::fetch_verified;
use crate::error::{AppError, AppResult};
use crate::extract::extract;
use crate::paths::{unix_now, validate_name, validate_version, write_atomic, Layout};
use crate::registry::{platform_key, InitSpec, ModuleManifest, PanelSpec, ServiceSpec};

const META_FILE: &str = ".cheesenv-module.json";
const INIT_MARKER: &str = ".cheesenv-initialized";
const INIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
/// Setup may download and build packages (e.g. pgAdmin's ~110 wheels).
const SETUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// Snapshot of what was installed, so services run without the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledModule {
    pub name: String,
    pub version: String,
    pub category: String,
    pub bin_dir: String,
    pub service: Option<ServiceSpec>,
    pub config_templates: BTreeMap<String, String>,
    #[serde(default)]
    pub panel: Option<PanelSpec>,
    /// Shims for this platform (see `ModuleManifest::shims`).
    #[serde(default)]
    pub shims: BTreeMap<String, String>,
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

/// Replaces the installed snapshot of an already installed module.
pub fn save_installed(layout: &Layout, m: &InstalledModule) -> AppResult<()> {
    load_installed(layout, &m.name, &m.version)?;
    let path = layout.module_dir(&m.name, &m.version).join(META_FILE);
    write_atomic(&path, &serde_json::to_vec_pretty(m)?)
}

/// Default port; on macOS/Linux privileged ports fall back to +8000
/// (80 → 8080, 443 → 8443) unless the program may bind them (PRD §7).
pub fn default_port(spec: &ServiceSpec, low_ports_granted: impl FnOnce() -> bool) -> Option<u16> {
    spec.default_port.map(|p| {
        if cfg!(unix) && p < 1024 && !low_ports_granted() {
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
        ("sock", p(layout.socket_dir(&m.name, &m.version))),
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
    if m.service.is_some() {
        layout.ensure_socket_dir(&m.name, &m.version)?;
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
    mut on_progress: impl FnMut(&str, u64, Option<u64>),
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

    let archive = fetch_verified(client, &asset.url, &asset.sha256, &layout.downloads(), |d, t| {
        on_progress("download", d, t)
    })
    .await?;
    let mut extras = Vec::new();
    for x in &asset.extra {
        let path = fetch_verified(client, &x.url, &x.sha256, &layout.downloads(), |d, t| on_progress("download", d, t)).await?;
        extras.push((path, x.clone()));
    }

    let meta = InstalledModule {
        name: manifest.name.clone(),
        version: version.to_string(),
        category: manifest.category.clone(),
        bin_dir: asset.bin_dir.clone(),
        service: manifest.service.clone(),
        config_templates: manifest.platform_templates().clone(),
        panel: manifest.panel.clone(),
        shims: manifest.platform_shims().clone(),
        installed_at: unix_now(),
    };
    let staging = layout
        .modules()
        .join(&manifest.name)
        .join(format!(".{version}.partial"));
    let module_files: Vec<(String, String)> = {
        let vars = template_vars(layout, &meta, None);
        manifest.module_files.iter().map(|(f, tpl)| (f.clone(), render(tpl, &vars))).collect()
    };
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
        for (path, x) in &extras {
            let dest = staging2.join(&x.dest);
            std::fs::create_dir_all(&dest)?;
            ensure_inside(&staging2, &dest)?;
            extract(path, &x.format, &dest, x.strip, "")?;
        }
        for (file, content) in &module_files {
            let target = staging2.join(file);
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir)?;
                ensure_inside(&staging2, dir)?;
            }
            write_atomic(&target, content.as_bytes())?;
        }
        std::fs::rename(&staging2, &final2)?;
        // Downloaded archives are no longer needed once extracted.
        for path in std::iter::once(&archive).chain(extras.iter().map(|(p, _)| p)) {
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    // Metadata is written last: a module without it counts as not installed.
    let finish = async {
        if let Some(setup) = &manifest.setup {
            on_progress("setup", 0, None);
            run_setup(layout, &meta, setup).await?;
        }
        std::fs::write(final_dir.join(META_FILE), serde_json::to_vec_pretty(&meta)?)?;
        Ok::<_, AppError>(())
    };
    if let Err(e) = finish.await {
        let _ = std::fs::remove_dir_all(&final_dir);
        return Err(e);
    }

    let port = meta.service.as_ref().and_then(port_for);
    write_configs(layout, &meta, port, true)?;
    Ok(meta)
}

/// `dir` (already created) must resolve inside `root`, so a symlink shipped in
/// an archive can never redirect later writes outside the module.
fn ensure_inside(root: &Path, dir: &Path) -> AppResult<()> {
    if dir.canonicalize()?.starts_with(root.canonicalize()?) {
        Ok(())
    } else {
        Err(AppError::UnsafeArchive(dir.display().to_string()))
    }
}

/// Runs the manifest's install-time commands from inside the module folder.
async fn run_setup(layout: &Layout, m: &InstalledModule, setup: &InitSpec) -> AppResult<()> {
    let setup_dir = layout.run_dir(&m.name, &m.version).join("setup");
    let mut vars = template_vars(layout, m, None);
    vars.push(("init", setup_dir.to_string_lossy().replace('\\', "/")));
    let log_file = layout.logs().join(format!("{}-{}.log", m.name, m.version));
    let result = async {
        for (file, tpl) in &setup.files {
            crate::secrets::write_private(&setup_dir.join(file), render(tpl, &vars).as_bytes())?;
        }
        let cwd = layout.module_dir(&m.name, &m.version);
        for cmd in setup.commands() {
            let (program, args) = resolve_command(layout, m, cmd, &vars)?;
            run_logged("setup", &program, &args, &cwd, &log_file, SETUP_TIMEOUT).await?;
        }
        Ok::<_, AppError>(())
    }
    .await;
    let _ = std::fs::remove_dir_all(&setup_dir);
    result
}

/// Renders a command and checks its program ships inside the module itself,
/// so a manifest can never launch arbitrary system binaries.
pub fn resolve_command(
    layout: &Layout,
    m: &InstalledModule,
    args: &[String],
    vars: &[(&str, String)],
) -> AppResult<(PathBuf, Vec<String>)> {
    let mut rendered = args.iter().map(|a| render(a, vars));
    let program = PathBuf::from(rendered.next().ok_or(AppError::NoService)?);
    let module_dir = layout.module_dir(&m.name, &m.version);
    if !program.starts_with(&module_dir) || program.components().any(|c| c == std::path::Component::ParentDir) {
        return Err(AppError::InvalidInput("service program outside module".into()));
    }
    Ok((program, rendered.collect()))
}

pub fn is_initialized(layout: &Layout, m: &InstalledModule) -> bool {
    layout.data_dir(&m.name, &m.version).join(INIT_MARKER).exists()
}

/// Runs the manifest's one-time init (e.g. `initdb`) into an empty data dir.
/// A generated root password goes to the keychain; the init files that carry
/// it exist only in a private temp folder for the duration of the commands.
/// On failure the data dir is emptied again so the next start can retry.
pub async fn initialize(layout: &Layout, m: &InstalledModule, port: Option<u16>, log_file: &Path) -> AppResult<()> {
    let Some(init) = m.service.as_ref().and_then(|s| s.init.as_ref()) else { return Ok(()) };
    let data = layout.data_dir(&m.name, &m.version);
    if is_initialized(layout, m) {
        return Ok(());
    }
    std::fs::create_dir_all(&data)?;
    if std::fs::read_dir(&data)?.next().is_some() {
        // Existing data we did not create: never touch it.
        return Ok(());
    }

    let has_credentials = m.service.as_ref().is_some_and(|s| s.credentials.is_some());
    let password = if has_credentials { crate::secrets::generate_password()? } else { String::new() };
    let init_dir = layout.run_dir(&m.name, &m.version).join("init");
    let mut vars = template_vars(layout, m, port);
    vars.push(("init", init_dir.to_string_lossy().replace('\\', "/")));

    let result = async {
        if has_credentials {
            crate::secrets::store(layout, &m.name, &m.version, &password)?;
        }
        let mut file_vars = vars.clone();
        file_vars.push(("password", password.clone()));
        for (file, tpl) in &init.files {
            crate::secrets::write_private(&init_dir.join(file), render(tpl, &file_vars).as_bytes())?;
        }
        for cmd in init.commands() {
            let (program, args) = resolve_command(layout, m, cmd, &vars)?;
            let cwd = layout.conf_dir(&m.name, &m.version);
            run_logged("init", &program, &args, &cwd, log_file, INIT_TIMEOUT).await?;
        }
        Ok::<_, AppError>(())
    }
    .await;

    let _ = std::fs::remove_dir_all(&init_dir);
    match result {
        Ok(()) => {
            std::fs::write(data.join(INIT_MARKER), "")?;
            Ok(())
        }
        Err(e) => {
            if let Ok(entries) = std::fs::read_dir(&data) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                }
            }
            crate::secrets::delete(layout, &m.name, &m.version);
            Err(e)
        }
    }
}

async fn run_logged(
    step: &str,
    program: &Path,
    args: &[String],
    cwd: &Path,
    log_file: &Path,
    timeout: std::time::Duration,
) -> AppResult<()> {
    use std::process::Stdio;
    if let Some(dir) = log_file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log = std::fs::OpenOptions::new().create(true).append(true).open(log_file)?;
    {
        use std::io::Write;
        let mut l = &log;
        let _ = writeln!(l, "[cheesenv {}] {step}: {}", unix_now(), program.display());
    }
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| AppError::Spawn(e.to_string()))?;
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(AppError::Spawn(format!("{step} exited with {status}"))),
        Ok(Err(e)) => Err(AppError::Spawn(e.to_string())),
        Err(_) => Err(AppError::Spawn(format!("{step} timed out"))),
    }
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
    let sock = layout.socket_dir(name, version);
    let _ = std::fs::remove_dir_all(&sock);
    if let Some(parent) = sock.parent() {
        let _ = std::fs::remove_dir(parent); // only when empty
    }
    if delete_data {
        crate::secrets::delete(layout, name, version);
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
            panel: None,
            shims: BTreeMap::new(),
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
