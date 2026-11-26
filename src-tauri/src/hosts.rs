//! `.test` domains in the system hosts file (PRD §7 local domains).
//!
//! Stackyard only ever owns one marked block, holding `127.0.0.1 <name>.test`
//! lines. Writing it needs administrator rights, so the app re-runs its own
//! binary elevated in a tiny helper mode (`--hosts-sync <domains…>`) that
//! re-validates every name and touches nothing outside the block.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::projects::is_valid_domain;

pub const HELPER_FLAG: &str = "--hosts-sync";
const BEGIN: &str = "# BEGIN Stackyard (managed automatically, do not edit)";
const END: &str = "# END Stackyard";

/// Helper exit codes.
const EXIT_INVALID: i32 = 2;
const EXIT_IO: i32 = 3;

pub fn hosts_path() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join("System32").join("drivers").join("etc").join("hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}

/// Domains currently listed inside Stackyard's block.
pub fn managed(content: &str) -> BTreeSet<String> {
    let mut inside = false;
    let mut out = BTreeSet::new();
    for line in content.lines() {
        let line = line.trim();
        if line == BEGIN {
            inside = true;
        } else if line == END {
            inside = false;
        } else if inside {
            out.extend(line.split_whitespace().skip(1).filter(|d| is_valid_domain(d)).map(String::from));
        }
    }
    out
}

/// `content` with Stackyard's block replaced by one for `domains` (removed
/// when empty). Everything outside the block is kept byte for byte.
pub fn apply(content: &str, domains: &BTreeSet<String>) -> String {
    let eol = if content.contains("\r\n") { "\r\n" } else { "\n" };
    let mut kept = String::with_capacity(content.len());
    let mut inside = false;
    for line in content.split_inclusive('\n') {
        let t = line.trim();
        if t == BEGIN {
            inside = true;
        } else if t == END && inside {
            inside = false;
        } else if !inside {
            kept.push_str(line);
        }
    }
    if domains.is_empty() {
        return kept;
    }
    if !kept.is_empty() && !kept.ends_with('\n') {
        kept.push_str(eol);
    }
    kept.push_str(BEGIN);
    kept.push_str(eol);
    for d in domains {
        kept.push_str(&format!("127.0.0.1 {d}{eol}"));
    }
    kept.push_str(END);
    kept.push_str(eol);
    kept
}

/// Whether the hosts file already lists exactly `domains` (readable without admin rights).
pub fn in_sync(domains: &BTreeSet<String>) -> bool {
    std::fs::read_to_string(hosts_path()).is_ok_and(|c| &managed(&c) == domains)
}

/// Entry point of the elevated helper: `stackyard --hosts-sync a.test b.test`.
/// Returns `None` when the process was not started as the helper.
pub fn helper_main(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some(HELPER_FLAG) {
        return None;
    }
    let domains: BTreeSet<String> = args[2..].iter().cloned().collect();
    if domains.len() > 1000 || !domains.iter().all(|d| is_valid_domain(d)) {
        eprintln!("stackyard: invalid domain list");
        return Some(EXIT_INVALID);
    }
    let path = hosts_path();
    let result = (|| -> std::io::Result<()> {
        let content = std::fs::read_to_string(&path)?;
        let updated = apply(&content, &domains);
        if updated == content {
            return Ok(());
        }
        let backup = path.with_file_name("hosts.stackyard-backup");
        if !backup.exists() {
            std::fs::write(&backup, &content)?;
        }
        // Written in place (not renamed) so ownership, mode and SELinux labels stay.
        std::fs::write(&path, updated)
    })();
    Some(match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("stackyard: {}: {e}", path.display());
            EXIT_IO
        }
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
fn elevated_command(exe: &std::path::Path, domains: &BTreeSet<String>) -> tokio::process::Command {
    // Domains are validated labels (no quoting needed); the path is shell-quoted,
    // then escaped for the AppleScript string literal.
    let shell_quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let mut line = format!("{} {HELPER_FLAG}", shell_quote(&exe.to_string_lossy()));
    for d in domains {
        line.push(' ');
        line.push_str(d);
    }
    let script = format!(
        "do shell script \"{}\" with prompt \"Stackyard needs to update the hosts file for your .test domains.\" with administrator privileges",
        line.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let mut cmd = tokio::process::Command::new("/usr/bin/osascript");
    cmd.arg("-e").arg(script);
    cmd
}

#[cfg(windows)]
fn elevated_command(exe: &std::path::Path, domains: &BTreeSet<String>) -> tokio::process::Command {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let list = std::iter::once(quote(HELPER_FLAG))
        .chain(domains.iter().map(|d| quote(d)))
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
fn elevated_command(exe: &std::path::Path, domains: &BTreeSet<String>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("pkexec");
    cmd.arg(exe).arg(HELPER_FLAG).args(domains);
    cmd
}

/// Writes Stackyard's hosts block through the elevated helper (one admin prompt).
pub async fn sync_elevated(domains: &BTreeSet<String>) -> AppResult<()> {
    if !domains.iter().all(|d| is_valid_domain(d)) {
        return Err(AppError::InvalidInput("domain".into()));
    }
    if in_sync(domains) {
        return Ok(());
    }
    let exe = self_exe()?;
    let out = elevated_command(&exe, domains)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| AppError::Spawn(format!("admin prompt: {e}")))?;
    let code = out.status.code();
    let stderr = String::from_utf8_lossy(&out.stderr);
    // pkexec: 126 dismissed / 127 not authorized; osascript: -128 user cancelled; UAC: 1223.
    let cancelled = matches!(code, Some(126 | 127 | 1223)) || stderr.contains("-128");
    match code {
        Some(0) if in_sync(domains) => Ok(()),
        _ if cancelled => Err(AppError::AuthCancelled),
        _ => Err(AppError::Spawn(format!("hosts helper exited with {code:?}: {}", stderr.trim()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ds: &[&str]) -> BTreeSet<String> {
        ds.iter().map(|d| d.to_string()).collect()
    }

    #[test]
    fn block_is_added_replaced_and_removed() {
        let base = "127.0.0.1 localhost\n::1 localhost\n";
        let one = apply(base, &set(&["toko.test"]));
        assert!(one.starts_with(base));
        assert!(one.contains("127.0.0.1 toko.test\n"));
        assert_eq!(managed(&one), set(&["toko.test"]));

        let two = apply(&one, &set(&["blog.test", "toko.test"]));
        assert_eq!(two.matches(BEGIN).count(), 1);
        assert_eq!(managed(&two), set(&["blog.test", "toko.test"]));

        assert_eq!(apply(&two, &BTreeSet::new()), base, "outside of the block is untouched");
    }

    #[test]
    fn keeps_crlf_and_missing_final_newline() {
        let win = "127.0.0.1 localhost\r\n# comment";
        let out = apply(win, &set(&["a.test"]));
        assert!(out.starts_with("127.0.0.1 localhost\r\n# comment\r\n"));
        assert!(out.contains("127.0.0.1 a.test\r\n"));
    }

    #[test]
    fn foreign_lines_in_block_are_ignored() {
        let content = format!("{BEGIN}\n1.2.3.4 bank.com\n127.0.0.1 ok.test\n{END}\n");
        assert_eq!(managed(&content), set(&["ok.test"]));
    }

    #[test]
    fn helper_rejects_bad_names_and_ignores_other_args() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(helper_main(&args(&["stackyard"])), None);
        assert_eq!(helper_main(&args(&["stackyard", "--other"])), None);
        assert_eq!(helper_main(&args(&["stackyard", HELPER_FLAG, "bank.com"])), Some(EXIT_INVALID));
        assert_eq!(helper_main(&args(&["stackyard", HELPER_FLAG, "a.test\n1.2.3.4 x.test"])), Some(EXIT_INVALID));
    }
}
