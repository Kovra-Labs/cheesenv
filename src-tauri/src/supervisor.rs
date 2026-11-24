use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, watch, Mutex};

use crate::error::{AppError, AppResult};
use crate::paths::unix_now;

const CRASH_WINDOW: Duration = Duration::from_secs(60);
const MAX_RESTARTS: usize = 3;
const GRACE: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
}

/// Everything needed to (re)spawn a service, resolved by the caller.
#[derive(Debug, Clone)]
pub struct Launch {
    pub id: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub log_file: PathBuf,
    pub auto_restart: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub id: String,
    pub status: Status,
    pub pid: Option<u32>,
    pub started_at: Option<u64>,
    pub recent_crashes: usize,
}

struct Runtime {
    status: Status,
    pid: Option<u32>,
    started_at: Option<u64>,
    crashes: Vec<Instant>,
    generation: u64,
    stop_tx: Option<oneshot::Sender<()>>,
    done_rx: Option<watch::Receiver<bool>>,
}

impl Runtime {
    fn new() -> Self {
        Self {
            status: Status::Stopped,
            pid: None,
            started_at: None,
            crashes: Vec::new(),
            generation: 0,
            stop_tx: None,
            done_rx: None,
        }
    }

    fn snapshot(&self, id: &str) -> Snapshot {
        Snapshot {
            id: id.to_string(),
            status: self.status,
            pid: self.pid,
            started_at: self.started_at,
            recent_crashes: self.crashes.len(),
        }
    }
}

pub type Notify = Arc<dyn Fn(Snapshot) + Send + Sync>;

/// Owns every service process. Services run as separate OS processes; the UI
/// only observes snapshots (PRD §6).
#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Mutex<HashMap<String, Runtime>>>,
    notify: Notify,
}

impl Supervisor {
    pub fn new(notify: Notify) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            notify,
        }
    }

    pub async fn snapshot(&self, id: &str) -> Snapshot {
        let map = self.inner.lock().await;
        map.get(id)
            .map(|r| r.snapshot(id))
            .unwrap_or_else(|| Runtime::new().snapshot(id))
    }

    pub async fn active_ids(&self) -> Vec<String> {
        let map = self.inner.lock().await;
        map.iter()
            .filter(|(_, r)| matches!(r.status, Status::Running | Status::Starting))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub async fn is_active(&self, id: &str) -> bool {
        let map = self.inner.lock().await;
        map.get(id)
            .is_some_and(|r| !matches!(r.status, Status::Stopped | Status::Crashed))
    }

    /// Starts the service; a no-op if it is already running.
    pub async fn start(&self, launch: Launch) -> AppResult<()> {
        let mut map = self.inner.lock().await;
        let rt = map.entry(launch.id.clone()).or_insert_with(Runtime::new);
        match rt.status {
            Status::Running | Status::Starting => return Ok(()),
            Status::Stopping => return Err(AppError::Busy),
            Status::Stopped | Status::Crashed => {}
        }
        rt.crashes.clear();
        self.spawn_locked(rt, launch)
    }

    fn spawn_locked(&self, rt: &mut Runtime, launch: Launch) -> AppResult<()> {
        let child = match spawn(&launch) {
            Ok(c) => c,
            Err(e) => {
                rt.status = Status::Crashed;
                rt.pid = None;
                (self.notify)(rt.snapshot(&launch.id));
                return Err(e);
            }
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        let (done_tx, done_rx) = watch::channel(false);
        rt.generation += 1;
        rt.status = Status::Running;
        rt.pid = child.id();
        rt.started_at = Some(unix_now());
        rt.stop_tx = Some(stop_tx);
        rt.done_rx = Some(done_rx);
        (self.notify)(rt.snapshot(&launch.id));

        let me = self.clone();
        let generation = rt.generation;
        tauri::async_runtime::spawn(async move {
            me.monitor(child, stop_rx, done_tx, launch, generation).await;
        });
        Ok(())
    }

    async fn monitor(
        &self,
        mut child: Child,
        stop_rx: oneshot::Receiver<()>,
        done_tx: watch::Sender<bool>,
        launch: Launch,
        generation: u64,
    ) {
        let pid = child.id();
        let requested = tokio::select! {
            r = child.wait() => {
                let code = r.ok().and_then(|s| s.code());
                append_log(&launch.log_file, &format!("process exited (code {code:?})"));
                false
            }
            _ = stop_rx => {
                terminate(&mut child, pid).await;
                append_log(&launch.log_file, "stopped");
                true
            }
        };

        let mut map = self.inner.lock().await;
        let Some(rt) = map.get_mut(&launch.id) else { return };
        if rt.generation != generation {
            return;
        }
        rt.pid = None;
        rt.stop_tx = None;
        let _ = done_tx.send(true);

        if requested || rt.status == Status::Stopping {
            rt.status = Status::Stopped;
            rt.started_at = None;
            (self.notify)(rt.snapshot(&launch.id));
            return;
        }

        // Unexpected exit: clean up orphaned children, then maybe restart.
        kill_group(pid);
        let now = Instant::now();
        rt.crashes.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
        rt.crashes.push(now);
        rt.status = Status::Crashed;
        rt.started_at = None;
        (self.notify)(rt.snapshot(&launch.id));

        if launch.auto_restart && rt.crashes.len() <= MAX_RESTARTS {
            rt.status = Status::Starting;
            (self.notify)(rt.snapshot(&launch.id));
            drop(map);
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut map = self.inner.lock().await;
            let Some(rt) = map.get_mut(&launch.id) else { return };
            // A user stop during the back-off wins.
            if rt.status != Status::Starting || rt.generation != generation {
                return;
            }
            append_log(&launch.log_file, "auto-restart after crash");
            let _ = self.spawn_locked(rt, launch);
        }
    }

    pub async fn stop(&self, id: &str) -> AppResult<()> {
        let done_rx = {
            let mut map = self.inner.lock().await;
            let Some(rt) = map.get_mut(id) else { return Ok(()) };
            match rt.status {
                Status::Stopped => return Ok(()),
                Status::Crashed | Status::Starting if rt.stop_tx.is_none() => {
                    // Crashed for good, or waiting in auto-restart back-off.
                    rt.status = Status::Stopped;
                    (self.notify)(rt.snapshot(id));
                    return Ok(());
                }
                _ => {}
            }
            rt.status = Status::Stopping;
            (self.notify)(rt.snapshot(id));
            if let Some(tx) = rt.stop_tx.take() {
                let _ = tx.send(());
            }
            rt.done_rx.clone()
        };
        if let Some(mut rx) = done_rx {
            let _ = tokio::time::timeout(GRACE * 2, rx.wait_for(|d| *d)).await;
        }
        Ok(())
    }

    pub async fn stop_all(&self) {
        let ids: Vec<String> = {
            let map = self.inner.lock().await;
            map.iter()
                .filter(|(_, r)| r.status != Status::Stopped)
                .map(|(id, _)| id.clone())
                .collect()
        };
        let tasks: Vec<_> = ids
            .into_iter()
            .map(|id| {
                let me = self.clone();
                tauri::async_runtime::spawn(async move { me.stop(&id).await })
            })
            .collect();
        for t in tasks {
            let _ = t.await;
        }
    }
}

fn spawn(launch: &Launch) -> AppResult<Child> {
    if let Some(dir) = launch.log_file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    append_log(
        &launch.log_file,
        &format!("starting {} {}", launch.program.display(), launch.args.join(" ")),
    );
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&launch.log_file)?;
    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args)
        .current_dir(&launch.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .kill_on_drop(false);
    #[cfg(unix)]
    cmd.process_group(0); // own group: lets us clean up worker processes
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd.spawn().map_err(|e| AppError::Spawn(e.to_string()))
}

fn append_log(path: &std::path::Path, msg: &str) {
    use std::io::Write;
    let line = format!("[stackyard {}] {msg}\n", unix_now());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(unix)]
async fn terminate(child: &mut Child, pid: Option<u32>) {
    if let Some(pid) = pid {
        // SAFETY: plain syscall on a pid we spawned and have not reaped yet.
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }
    if tokio::time::timeout(GRACE, child.wait()).await.is_err() {
        kill_group(pid);
        let _ = child.kill().await;
    }
}

#[cfg(windows)]
async fn terminate(child: &mut Child, pid: Option<u32>) {
    if let Some(pid) = pid {
        // Kill the whole tree (nginx/php-cgi spawn worker processes).
        // Absolute path: never resolve taskkill through a user-writable PATH.
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let taskkill = PathBuf::from(root).join("System32").join("taskkill.exe");
        let _ = Command::new(taskkill)
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    if tokio::time::timeout(GRACE, child.wait()).await.is_err() {
        let _ = child.kill().await;
    }
}

#[cfg(unix)]
fn kill_group(pid: Option<u32>) {
    if let Some(pid) = pid {
        // SAFETY: signals the process group we created via process_group(0).
        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    }
}

#[cfg(windows)]
fn kill_group(_pid: Option<u32>) {}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn launch(dir: &std::path::Path, script: &str, auto_restart: bool) -> Launch {
        Launch {
            id: "test@1".into(),
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: dir.to_path_buf(),
            log_file: dir.join("test.log"),
            auto_restart,
        }
    }

    #[tokio::test]
    async fn start_and_stop() {
        let tmp = tempfile::tempdir().unwrap();
        let sup = Supervisor::new(Arc::new(|_| {}));
        sup.start(launch(tmp.path(), "echo hello; sleep 30", false)).await.unwrap();
        assert_eq!(sup.snapshot("test@1").await.status, Status::Running);
        sup.stop("test@1").await.unwrap();
        assert_eq!(sup.snapshot("test@1").await.status, Status::Stopped);
        let log = std::fs::read_to_string(tmp.path().join("test.log")).unwrap();
        assert!(log.contains("hello"));
    }

    #[tokio::test]
    async fn crash_restarts_at_most_three_times() {
        let tmp = tempfile::tempdir().unwrap();
        let spawns = Arc::new(AtomicUsize::new(0));
        let s2 = spawns.clone();
        let sup = Supervisor::new(Arc::new(move |snap| {
            if snap.status == Status::Running {
                s2.fetch_add(1, Ordering::SeqCst);
            }
        }));
        sup.start(launch(tmp.path(), "exit 1", true)).await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(spawns.load(Ordering::SeqCst), 4); // 1 start + 3 restarts
        let snap = sup.snapshot("test@1").await;
        assert_eq!(snap.status, Status::Crashed);
        assert_eq!(snap.recent_crashes, 4);
    }
}
