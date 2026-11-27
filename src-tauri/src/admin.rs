//! Administrator tasks (PRD §7). The app re-runs its own binary elevated
//! (pkexec, an AppleScript admin prompt or UAC) in a tiny helper mode that
//! does one validated job and exits:
//!   `--hosts-sync <name.test…>`  the hosts-file block (hosts.rs)
//!   `--trust-ca <file.pem>`      installs CheesEnv's local CA (tls.rs)
//!   `--grant-ports <nginx>`      Linux: lets nginx bind ports 80 and 443

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::{hosts, tls};

/// Helper exit codes.
pub const EXIT_INVALID: i32 = 2;
pub const EXIT_IO: i32 = 3;
/// The file system ignores file capabilities (`nosuid` mount).
pub const EXIT_NOSUID: i32 = 4;

pub const GRANT_PORTS_FLAG: &str = "--grant-ports";

/// Entry point of the elevated helper. `None` when the process was not
/// started as the helper.
pub fn helper_main(args: &[String]) -> Option<i32> {
    let rest = args.get(2..).unwrap_or_default();
    Some(match args.get(1)?.as_str() {
        hosts::HELPER_FLAG => hosts::helper(rest),
        tls::HELPER_FLAG => tls::trust_helper(rest),
        GRANT_PORTS_FLAG => grant_ports_helper(rest),
        _ => return None,
    })
}

/// The program to re-run elevated: inside an AppImage, the AppImage itself
/// (its mount point is not readable by root).
fn self_exe() -> AppResult<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(appimage);
    }
    std::env::current_exe().map_err(AppError::from)
}

#[cfg(target_os = "macos")]
fn elevated_command(exe: &Path, args: &[String], prompt: &str) -> tokio::process::Command {
    // Every word is shell-quoted, then the line is escaped for the AppleScript string.
    let shell_quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let line = std::iter::once(exe.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .map(|a| shell_quote(&a))
        .collect::<Vec<_>>()
        .join(" ");
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "do shell script \"{}\" with prompt \"{}\" with administrator privileges",
        esc(&line),
        esc(prompt)
    );
    let mut cmd = tokio::process::Command::new("/usr/bin/osascript");
    cmd.arg("-e").arg(script);
    cmd
}

#[cfg(windows)]
fn elevated_command(exe: &Path, args: &[String], _prompt: &str) -> tokio::process::Command {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    // Start-Process joins the list with spaces: paths with spaces need their own quotes.
    let list = args
        .iter()
        .map(|a| quote(&if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "try {{ $p = Start-Process -FilePath {} -ArgumentList @({list}) -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode }} catch {{ exit 1223 }}",
        quote(&exe.to_string_lossy())
    );
    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(all(unix, not(target_os = "macos")))]
fn elevated_command(exe: &Path, args: &[String], _prompt: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("pkexec");
    cmd.arg(exe).args(args);
    cmd
}

/// Runs `cheesenv <args…>` with administrator rights (one prompt; `prompt`
/// is shown where the OS allows a custom message).
pub async fn run_elevated(args: &[String], prompt: &str) -> AppResult<()> {
    let exe = self_exe()?;
    let out = elevated_command(&exe, args, prompt)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| AppError::Spawn(format!("admin prompt: {e}")))?;
    let code = out.status.code();
    let stderr = String::from_utf8_lossy(&out.stderr);
    // pkexec: 126 dismissed / 127 not authorized; osascript: -128 user cancelled; UAC: 1223.
    let cancelled = matches!(code, Some(126 | 127 | 1223)) || stderr.contains("-128");
    match code {
        Some(0) => Ok(()),
        _ if cancelled => Err(AppError::AuthCancelled),
        Some(EXIT_NOSUID) => Err(AppError::Spawn(
            "the data folder is on a file system mounted with nosuid, which ignores the port permission".into(),
        )),
        _ => Err(AppError::Spawn(format!("admin helper exited with {code:?}: {}", stderr.trim()))),
    }
}

// ---------- ports below 1024 (Linux) ----------

/// `cap_net_bind_service` (bit 10) in a file capability: VFS_CAP_REVISION_2
/// or 3, effective flag set, bit present in the permitted set.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn grants_bind_service(xattr: &[u8]) -> bool {
    let word = |i: usize| xattr.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let (Some(magic), Some(permitted)) = (word(0), word(4)) else { return false };
    matches!(magic & 0xFF00_0000, 0x0200_0000 | 0x0300_0000) && magic & 1 == 1 && permitted & (1 << 10) != 0
}

/// Whether `program` may bind ports below 1024 without running as root.
#[cfg(target_os = "linux")]
pub fn low_ports_granted(program: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(program.as_os_str().as_bytes()) else { return false };
    let mut buf = [0u8; 64];
    // SAFETY: valid NUL-terminated strings and a buffer of the given length.
    let n = unsafe {
        libc::getxattr(path.as_ptr(), c"security.capability".as_ptr(), buf.as_mut_ptr().cast(), buf.len())
    };
    n > 0 && grants_bind_service(&buf[..n as usize])
}

#[cfg(not(target_os = "linux"))]
pub fn low_ports_granted(_program: &Path) -> bool {
    false
}

/// Only CheesEnv's own nginx: `…/modules/nginx/<version>/…/nginx`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_cheesenv_nginx(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == "nginx")
        && path.ancestors().skip(1).any(|a| {
            a.file_name().is_some_and(|n| n == "nginx") && a.parent().and_then(Path::file_name).is_some_and(|n| n == "modules")
        })
}

#[cfg(target_os = "linux")]
fn grant_ports_helper(args: &[String]) -> i32 {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let [path] = args else { return EXIT_INVALID };
    // Opened without following a final symlink; checked on the open file, so a
    // swapped path cannot redirect the grant to another program.
    let file = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cheesenv: {path}: {e}");
            return EXIT_INVALID;
        }
    };
    use std::os::fd::AsRawFd;
    let fd = file.as_raw_fd();
    let real = std::fs::read_link(format!("/proc/self/fd/{fd}")).unwrap_or_default();
    let Ok(meta) = file.metadata() else { return EXIT_IO };
    // The invoking user's own file: system programs (owned by root) are refused.
    let caller = std::env::var("PKEXEC_UID").or_else(|_| std::env::var("SUDO_UID")).ok().and_then(|u| u.parse::<u32>().ok());
    let owner_ok = meta.uid() != 0 && caller.is_none_or(|uid| uid == meta.uid());
    if !meta.is_file() || !is_cheesenv_nginx(&real) || !owner_ok {
        eprintln!("cheesenv: not CheesEnv's nginx: {}", real.display());
        return EXIT_INVALID;
    }
    // SAFETY: zeroed POD out-parameter for fstatvfs on an open descriptor.
    let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatvfs(fd, &mut vfs) } == 0 && vfs.f_flag & libc::ST_NOSUID != 0 {
        return EXIT_NOSUID;
    }
    let mut value = [0u8; 20];
    value[0..4].copy_from_slice(&(0x0200_0000u32 | 1).to_le_bytes()); // revision 2, effective
    value[4..8].copy_from_slice(&(1u32 << 10).to_le_bytes()); // permitted: cap_net_bind_service
    // SAFETY: valid descriptor, NUL-terminated name and a buffer of the given length.
    let rc = unsafe { libc::fsetxattr(fd, c"security.capability".as_ptr(), value.as_ptr().cast(), value.len(), 0) };
    if rc != 0 {
        eprintln!("cheesenv: {}: {}", real.display(), std::io::Error::last_os_error());
        return EXIT_IO;
    }
    0
}

#[cfg(not(target_os = "linux"))]
fn grant_ports_helper(_args: &[String]) -> i32 {
    EXIT_INVALID
}

/// Lets CheesEnv's nginx at `program` bind ports 80 and 443 (one admin prompt).
pub async fn grant_low_ports(program: &Path) -> AppResult<()> {
    if !cfg!(target_os = "linux") {
        return Err(AppError::UnsupportedPlatform);
    }
    let args = [GRANT_PORTS_FLAG.to_string(), program.to_string_lossy().into_owned()];
    run_elevated(&args, "CheesEnv needs permission to let nginx use ports 80 and 443.").await?;
    if low_ports_granted(program) {
        Ok(())
    } else {
        Err(AppError::Spawn("nginx did not get the port permission".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_parsing() {
        let mut v2 = [0u8; 20];
        v2[0..4].copy_from_slice(&0x0200_0001u32.to_le_bytes());
        v2[4..8].copy_from_slice(&(1u32 << 10).to_le_bytes());
        assert!(grants_bind_service(&v2));
        let mut v3 = [0u8; 24];
        v3[..20].copy_from_slice(&v2);
        v3[0..4].copy_from_slice(&0x0300_0001u32.to_le_bytes());
        assert!(grants_bind_service(&v3));
        let mut not_effective = v2;
        not_effective[0] = 0;
        assert!(!grants_bind_service(&not_effective));
        let mut other_cap = v2;
        other_cap[4..8].copy_from_slice(&(1u32 << 13).to_le_bytes()); // cap_net_raw
        assert!(!grants_bind_service(&other_cap));
        assert!(!grants_bind_service(&[1, 2]));
    }

    #[test]
    fn only_cheesenv_nginx_is_accepted() {
        assert!(is_cheesenv_nginx(Path::new("/home/u/CheesEnv/modules/nginx/1.30.5/nginx")));
        assert!(is_cheesenv_nginx(Path::new("/home/u/CheesEnv/modules/nginx/1.30.5/sbin/nginx")));
        assert!(!is_cheesenv_nginx(Path::new("/usr/sbin/nginx")));
        assert!(!is_cheesenv_nginx(Path::new("/home/u/CheesEnv/modules/php/8.4/nginx")));
        assert!(!is_cheesenv_nginx(Path::new("/home/u/CheesEnv/modules/nginx/1.30.5/python3")));
    }

    #[test]
    fn helper_ignores_other_invocations() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(helper_main(&args(&["cheesenv"])), None);
        assert_eq!(helper_main(&args(&["cheesenv", "--minimized"])), None);
        assert_eq!(helper_main(&args(&["cheesenv", GRANT_PORTS_FLAG])), Some(EXIT_INVALID));
        assert_eq!(helper_main(&args(&["cheesenv", GRANT_PORTS_FLAG, "/usr/bin/env"])), Some(EXIT_INVALID));
    }
}
