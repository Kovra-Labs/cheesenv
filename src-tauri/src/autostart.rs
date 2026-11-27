//! Launch at login: a Windows Run key, a macOS LaunchAgent or a Linux
//! autostart entry. The `launch_at_login` setting is the source of truth and
//! the OS entry follows it.

use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

use crate::error::{AppError, AppResult};

/// Added by the login entry: the app starts hidden in the tray.
const HIDDEN_FLAG: &str = "--hidden";

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_autostart::Builder::new().arg(HIDDEN_FLAG).build()
}

/// Whether this process was started by the login entry.
pub fn started_hidden() -> bool {
    std::env::args().skip(1).any(|a| a == HIDDEN_FLAG)
}

/// Makes the OS entry match `enabled`. Enabling rewrites the entry, so it
/// also follows the app to a new location (e.g. a moved AppImage).
pub fn sync<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> AppResult<()> {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else if launcher.is_enabled().unwrap_or(true) {
        launcher.disable()
    } else {
        Ok(())
    };
    result.map_err(|e| AppError::Internal(format!("launch at login: {e}")))
}
