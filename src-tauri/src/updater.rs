use std::collections::BTreeMap;

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
    /// `notes_i18n` from latest.json: the same notes per language code.
    notes_i18n: BTreeMap<String, String>,
    date: Option<String>,
}

/// Per-language notes; anything that is not a language → text pair is ignored.
fn localized_notes(raw: &serde_json::Value) -> BTreeMap<String, String> {
    raw.get("notes_i18n")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter(|(k, _)| k.len() <= 8 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
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
        notes_i18n: localized_notes(&u.raw_json),
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

#[cfg(test)]
mod tests {
    use super::localized_notes;
    use serde_json::json;

    #[test]
    fn localized_notes_keeps_language_text_pairs_only() {
        let raw = json!({ "notes_i18n": { "id": "## [1.0.0]", "en": "## [1.0.0]", "x/../y": "bad", "fr": 1 } });
        let notes = localized_notes(&raw);
        assert_eq!(notes.keys().collect::<Vec<_>>(), ["en", "id"]);
        assert!(localized_notes(&json!({ "notes": "x" })).is_empty());
        assert!(localized_notes(&json!({ "notes_i18n": "x" })).is_empty());
    }
}
