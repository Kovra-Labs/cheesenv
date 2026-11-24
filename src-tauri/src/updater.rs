use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::state::AppState;

pub const EVENT_UPDATE_PROGRESS: &str = "update-progress";

/// Update found by the last check, kept until the user installs it.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Serialize)]
pub struct UpdateInfo {
    version: String,
    current_version: String,
    notes: Option<String>,
    date: Option<String>,
}

#[derive(Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

fn map_err(e: tauri_plugin_updater::Error) -> AppError {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Minisign(_) | E::Base64(_) | E::SignatureUtf8(_) => AppError::Signature(e.to_string()),
        E::Reqwest(_) | E::Network(_) => AppError::Network(e.to_string()),
        // Endpoint answered but has no release JSON / no build for this target.
        E::ReleaseNotFound | E::TargetNotFound(_) | E::TargetsNotFound(_) => AppError::UpdateUnavailable(e.to_string()),
        E::AuthenticationFailed => AppError::AuthCancelled,
        other => AppError::Internal(other.to_string()),
    }
}

/// Asks the release endpoint for a newer, signed build (PRD §8).
#[tauri::command]
pub async fn check_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> Result<Option<UpdateInfo>, AppError> {
    let update = app.updater().map_err(map_err)?.check().await.map_err(map_err)?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    });
    *pending.0.lock().await = update;
    Ok(info)
}

/// Downloads and installs the pending update, then relaunches. Services are
/// stopped first because the installer replaces (and on Windows closes) the app.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingUpdate>,
) -> Result<(), AppError> {
    let update = pending
        .0
        .lock()
        .await
        .take()
        .ok_or_else(|| AppError::NotFound("update".into()))?;
    state.supervisor.stop_all().await;

    let (mut downloaded, mut last) = (0u64, 0u64);
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                if downloaded - last >= 256 * 1024 || Some(downloaded) == total {
                    last = downloaded;
                    let _ = app.emit(EVENT_UPDATE_PROGRESS, Progress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(map_err)?;

    state.quitting.store(true, std::sync::atomic::Ordering::SeqCst);
    app.restart()
}
