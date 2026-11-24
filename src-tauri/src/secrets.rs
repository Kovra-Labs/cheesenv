use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::paths::Layout;

#[cfg(not(test))]
const KEYRING_SERVICE: &str = "Stackyard";
/// Tests never touch the user's real credentials.
#[cfg(test)]
const KEYRING_SERVICE: &str = "Stackyard-test";

/// Where a database password lives. The OS keychain is preferred (PRD §8);
/// the file fallback only exists for systems without a keychain service
/// (e.g. Linux without Secret Service) and is readable by the user only.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    Keychain,
    File,
}

/// Alphanumeric only, so it can be embedded in SQL/conf files without quoting.
pub fn generate_password() -> AppResult<String> {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| AppError::Internal(e.to_string()))?;
    // 256 % 56 bias is negligible for a 24-char local password (~139 bits).
    Ok(bytes.iter().map(|b| CHARS[*b as usize % CHARS.len()] as char).collect())
}

/// Kept next to (not inside) the data dir so it survives an uninstall that
/// keeps data, and is removed together with the data.
fn fallback_path(layout: &Layout, name: &str, version: &str) -> PathBuf {
    layout.root.join("data").join(name).join(format!("{version}.credentials"))
}

/// Runs a keychain operation on a fresh OS thread. The Secret Service
/// backend drives its own async runtime and panics if called from inside
/// Tokio (which every Tauri command and the supervisor run on).
fn with_keychain<T: Send + 'static>(id: &str, f: impl FnOnce(&keyring::Entry) -> keyring::Result<T> + Send + 'static) -> Option<T> {
    let id = id.to_string();
    std::thread::spawn(move || keyring::Entry::new(KEYRING_SERVICE, &id).and_then(|e| f(&e)).ok())
        .join()
        .ok()
        .flatten()
}

pub fn store(layout: &Layout, name: &str, version: &str, password: &str) -> AppResult<Storage> {
    let id = crate::paths::service_id(name, version);
    let pw = password.to_string();
    if with_keychain(&id, move |e| e.set_password(&pw)).is_some() {
        let _ = std::fs::remove_file(fallback_path(layout, name, version));
        return Ok(Storage::Keychain);
    }
    eprintln!("[stackyard] keychain unavailable, storing {id} password in a private file");
    let path = fallback_path(layout, name, version);
    write_private(&path, password.as_bytes())?;
    Ok(Storage::File)
}

pub fn load(layout: &Layout, name: &str, version: &str) -> AppResult<(String, Storage)> {
    let id = crate::paths::service_id(name, version);
    if let Some(pw) = with_keychain(&id, |e| e.get_password()) {
        return Ok((pw, Storage::Keychain));
    }
    std::fs::read_to_string(fallback_path(layout, name, version))
        .map(|pw| (pw.trim().to_string(), Storage::File))
        .map_err(|_| AppError::NotFound("credentials".into()))
}

pub fn delete(layout: &Layout, name: &str, version: &str) {
    with_keychain(&crate::paths::service_id(name, version), |e| e.delete_credential());
    let _ = std::fs::remove_file(fallback_path(layout, name, version));
}

/// Writes a file only the current user can read.
pub fn write_private(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let tmp = path.with_extension("tmp-write");
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        // Files under the user profile inherit a user-only ACL on Windows.
        crate::paths::write_atomic(path, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_random_and_safe() {
        let a = generate_password().unwrap();
        let b = generate_password().unwrap();
        assert_eq!(a.len(), 24);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[cfg(unix)]
    #[test]
    fn private_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x/secret");
        write_private(&p, b"pw").unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
