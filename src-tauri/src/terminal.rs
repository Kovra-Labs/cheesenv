//! Built-in terminal (PRD §4): a shell in a pseudo-terminal whose PATH starts
//! with Stackyard's default runtimes and the tools of the installed modules.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::error::{AppError, AppResult};
use crate::modules::InstalledModule;
use crate::paths::Layout;

/// Largest single write accepted from the UI (a big paste).
pub const MAX_INPUT: usize = 1 << 20;
const MAX_SESSIONS: usize = 16;

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Default)]
pub struct Terminals {
    sessions: Arc<Mutex<HashMap<u32, Session>>>,
    next: AtomicU32,
}

fn size(cols: u16, rows: u16) -> AppResult<PtySize> {
    if !(2..=1000).contains(&cols) || !(1..=500).contains(&rows) {
        return Err(AppError::InvalidInput("terminal size".into()));
    }
    Ok(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
}

fn pty_err(e: impl std::fmt::Display) -> AppError {
    AppError::Spawn(format!("terminal: {e}"))
}

/// Folders put in front of PATH: the default runtimes' shims, then the newest
/// installed version of every other module (mysql, psql, redis-cli, …).
pub fn path_dirs(layout: &Layout, installed: &[InstalledModule]) -> Vec<PathBuf> {
    let mut dirs = vec![layout.shim_dir()];
    let mut seen = std::collections::HashSet::new();
    // `installed` is sorted by name, newest version first.
    for m in installed {
        if m.panel.is_some() || !m.shims.is_empty() || !seen.insert(m.name.clone()) {
            continue;
        }
        let dir = layout.module_dir(&m.name, &m.version);
        let bin = if m.bin_dir.is_empty() { dir } else { dir.join(&m.bin_dir) };
        if bin.is_dir() {
            dirs.push(bin);
        }
    }
    if cfg!(target_os = "macos") {
        // Apps started from Finder get a bare PATH; add the usual tool folders.
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    dirs
}

fn default_shell() -> CommandBuilder {
    if cfg!(windows) {
        let mut cmd = CommandBuilder::new("powershell.exe");
        cmd.arg("-NoLogo");
        return cmd;
    }
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
        .or_else(|| ["/bin/bash", "/bin/zsh", "/bin/sh"].map(PathBuf::from).into_iter().find(|p| p.is_file()))
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    CommandBuilder::new(shell)
}

impl Terminals {
    /// Starts a shell in `cwd`. Output goes to `on_output`; `on_exit` runs once
    /// the shell has exited and the session is gone.
    pub fn open(
        &self,
        cwd: &Path,
        path_dirs: &[PathBuf],
        home: &Path,
        cols: u16,
        rows: u16,
        on_output: impl Fn(Vec<u8>) + Send + 'static,
        on_exit: impl FnOnce(u32, Option<u32>) + Send + 'static,
    ) -> AppResult<u32> {
        if self.sessions.lock().unwrap_or_else(|e| e.into_inner()).len() >= MAX_SESSIONS {
            return Err(AppError::Busy);
        }
        let pair = native_pty_system().openpty(size(cols, rows)?).map_err(pty_err)?;
        let mut cmd = default_shell();
        cmd.cwd(cwd);
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(path_dirs.iter().cloned().chain(std::env::split_paths(&inherited)))
            .map_err(pty_err)?;
        cmd.env("PATH", path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("STACKYARD_HOME", home);

        let mut child = pair.slave.spawn_command(cmd).map_err(pty_err)?;
        drop(pair.slave); // the shell holds the only slave handle: EOF once it exits
        let mut reader = pair.master.try_clone_reader().map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;
        let killer = child.clone_killer();
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Session { master: pair.master, writer, killer });

        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                on_output(buf[..n].to_vec());
            }
        });
        let sessions = Arc::clone(&self.sessions);
        std::thread::spawn(move || {
            let code = child.wait().ok().map(|s| s.exit_code());
            // Dropping the master ends the reader (ConPTY does not send EOF by itself).
            sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
            on_exit(id, code);
        });
        Ok(id)
    }

    fn with<T>(&self, id: u32, f: impl FnOnce(&mut Session) -> AppResult<T>) -> AppResult<T> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let s = sessions.get_mut(&id).ok_or_else(|| AppError::NotFound(format!("terminal {id}")))?;
        f(s)
    }

    pub fn write(&self, id: u32, data: &[u8]) -> AppResult<()> {
        if data.len() > MAX_INPUT {
            return Err(AppError::InvalidInput("terminal input too large".into()));
        }
        self.with(id, |s| {
            s.writer.write_all(data)?;
            s.writer.flush()?;
            Ok(())
        })
    }

    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> AppResult<()> {
        let sz = size(cols, rows)?;
        self.with(id, |s| s.master.resize(sz).map_err(pty_err))
    }

    /// Kills the shell; the exit callback then removes the session.
    pub fn close(&self, id: u32) -> AppResult<()> {
        self.with(id, |s| s.killer.kill().map_err(AppError::from))
    }

    pub fn close_all(&self) {
        for s in self.sessions.lock().unwrap_or_else(|e| e.into_inner()).values_mut() {
            let _ = s.killer.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn size_limits() {
        assert!(size(80, 24).is_ok());
        assert!(size(0, 24).is_err());
        assert!(size(80, 0).is_err());
        assert!(size(5000, 24).is_err());
    }

    #[test]
    fn path_puts_shims_first_and_skips_panels() {
        use std::collections::BTreeMap;
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::new(tmp.path().to_path_buf());
        let module = |name: &str, version: &str, panel: bool| {
            std::fs::create_dir_all(layout.module_dir(name, version).join("bin")).unwrap();
            InstalledModule {
                name: name.into(),
                version: version.into(),
                category: "database".into(),
                bin_dir: "bin".into(),
                service: None,
                config_templates: BTreeMap::new(),
                panel: panel.then(|| serde_json::from_str("{\"for\":\"mysql\",\"path\":\"/\"}").unwrap()),
                shims: BTreeMap::new(),
                installed_at: 0,
            }
        };
        let installed = [module("mysql", "8.4", false), module("mysql", "8.0", false), module("phpmyadmin", "5", true)];
        let dirs = path_dirs(&layout, &installed);
        assert_eq!(dirs[0], layout.shim_dir());
        assert_eq!(dirs[1], layout.module_dir("mysql", "8.4").join("bin"));
        assert!(!dirs.iter().any(|d| d.starts_with(layout.module_dir("mysql", "8.0"))), "newest only");
        assert!(!dirs.iter().any(|d| d.starts_with(layout.modules().join("phpmyadmin"))));
    }

    #[cfg(unix)]
    #[test]
    fn shell_runs_with_stackyard_path_and_exits() {
        let tmp = tempfile::tempdir().unwrap();
        let terms = Terminals::default();
        let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
        let (exit_tx, exit_rx) = mpsc::channel();
        let marker = tmp.path().join("marker-bin");
        let id = terms
            .open(
                tmp.path(),
                &[marker.clone()],
                tmp.path(),
                80,
                24,
                move |b| {
                    let _ = out_tx.send(b);
                },
                move |id, code| {
                    let _ = exit_tx.send((id, code));
                },
            )
            .unwrap();
        terms.write(id, b"echo \"P=$PATH\"; exit 3\n").unwrap();
        let (eid, code) = exit_rx.recv_timeout(Duration::from_secs(10)).expect("shell exits");
        assert_eq!((eid, code), (id, Some(3)));
        let mut output = Vec::new();
        while let Ok(chunk) = out_rx.recv_timeout(Duration::from_millis(300)) {
            output.extend(chunk);
        }
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains(&marker.display().to_string()), "PATH holds Stackyard dirs: {text}");
        assert!(terms.write(id, b"x").is_err(), "session removed after exit");
    }
}
