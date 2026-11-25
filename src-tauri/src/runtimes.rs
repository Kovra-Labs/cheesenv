//! Default runtime versions (Node.js, Python, …). The command-line tools of the
//! default version are exposed as shims in `<data>/bin`, the one folder users
//! add to their PATH: symlinks on Unix, `.cmd` launchers on Windows (symlinks
//! there need admin rights or developer mode).

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::modules::InstalledModule;
use crate::paths::Layout;

fn shim_path(dir: &Path, cmd: &str) -> PathBuf {
    if cfg!(windows) {
        dir.join(format!("{cmd}.cmd"))
    } else {
        dir.join(cmd)
    }
}

/// Points every shim of `m` at its tools. Shims already owned by another
/// version of the same runtime are simply replaced.
pub fn activate(layout: &Layout, m: &InstalledModule) -> AppResult<()> {
    if m.shims.is_empty() {
        return Err(AppError::InvalidInput("not a runtime".into()));
    }
    let module_dir = layout.module_dir(&m.name, &m.version);
    // Check every target first so a broken module never half-replaces the default.
    let links: Vec<(String, PathBuf)> = m
        .shims
        .iter()
        .map(|(cmd, rel)| {
            let target = module_dir.join(rel);
            if target.exists() {
                Ok((cmd.clone(), target))
            } else {
                Err(AppError::NotFound(format!("{}/{rel}", m.name)))
            }
        })
        .collect::<AppResult<_>>()?;
    let dir = layout.shim_dir();
    std::fs::create_dir_all(&dir)?;
    for (cmd, target) in links {
        write_shim(&shim_path(&dir, &cmd), &target)?;
    }
    Ok(())
}

/// Removes the shims that still point into `m` (another version may own them now).
pub fn deactivate(layout: &Layout, m: &InstalledModule) -> AppResult<()> {
    let module_dir = layout.module_dir(&m.name, &m.version);
    let dir = layout.shim_dir();
    for cmd in m.shims.keys() {
        let shim = shim_path(&dir, cmd);
        if points_into(&shim, &module_dir) {
            std::fs::remove_file(&shim)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn write_shim(shim: &Path, target: &Path) -> AppResult<()> {
    if std::fs::symlink_metadata(shim).is_ok() {
        std::fs::remove_file(shim)?;
    }
    std::os::unix::fs::symlink(target, shim)?;
    Ok(())
}

#[cfg(windows)]
fn write_shim(shim: &Path, target: &Path) -> AppResult<()> {
    let target = target.to_string_lossy();
    // The path is pasted into a batch file: refuse anything cmd.exe would expand.
    if target.contains(['"', '%', '\r', '\n']) {
        return Err(AppError::InvalidInput("data_dir".into()));
    }
    crate::paths::write_atomic(shim, format!("@\"{target}\" %*\r\n").as_bytes())
}

#[cfg(unix)]
fn points_into(shim: &Path, module_dir: &Path) -> bool {
    std::fs::read_link(shim).is_ok_and(|t| t.starts_with(module_dir))
}

#[cfg(windows)]
fn points_into(shim: &Path, module_dir: &Path) -> bool {
    let prefix = format!("\"{}{}", module_dir.display(), std::path::MAIN_SEPARATOR);
    std::fs::read_to_string(shim).is_ok_and(|s| s.contains(&prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn runtime(layout: &Layout, version: &str) -> InstalledModule {
        let m = InstalledModule {
            name: "node".into(),
            version: version.into(),
            category: "runtime".into(),
            bin_dir: "bin".into(),
            service: None,
            config_templates: BTreeMap::new(),
            panel: None,
            shims: BTreeMap::from([("node".into(), "bin/node".into()), ("npm".into(), "bin/npm".into())]),
            installed_at: 0,
        };
        let bin = layout.module_dir("node", version).join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("node"), version).unwrap();
        std::fs::write(bin.join("npm"), version).unwrap();
        m
    }

    /// On Unix this follows the symlink, so it returns the target's content.
    fn shim_content(layout: &Layout, cmd: &str) -> Option<String> {
        std::fs::read_to_string(shim_path(&layout.shim_dir(), cmd)).ok()
    }

    #[test]
    fn switch_and_remove_default() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::new(tmp.path().to_path_buf());
        let (old, new) = (runtime(&layout, "24.0.0"), runtime(&layout, "26.0.0"));

        activate(&layout, &old).unwrap();
        if cfg!(unix) {
            assert_eq!(shim_content(&layout, "node").as_deref(), Some("24.0.0"));
        }
        // Switching: the new version takes over; deactivating the old one must not remove it.
        activate(&layout, &new).unwrap();
        deactivate(&layout, &old).unwrap();
        assert!(shim_content(&layout, "npm").is_some());
        if cfg!(unix) {
            assert_eq!(shim_content(&layout, "node").as_deref(), Some("26.0.0"));
        }

        deactivate(&layout, &new).unwrap();
        assert!(shim_content(&layout, "node").is_none());
    }

    #[test]
    fn missing_tool_leaves_shims_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::new(tmp.path().to_path_buf());
        let good = runtime(&layout, "24.0.0");
        activate(&layout, &good).unwrap();
        let mut broken = runtime(&layout, "26.0.0");
        broken.shims.insert("corepack".into(), "bin/corepack".into());
        assert!(activate(&layout, &broken).is_err());
        assert!(shim_content(&layout, "node").is_some());
        if cfg!(unix) {
            assert_eq!(shim_content(&layout, "node").as_deref(), Some("24.0.0"));
        }
    }
}
