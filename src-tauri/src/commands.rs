use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, AppResult};
use crate::modules::{self, InstalledModule};
use crate::paths::{parse_service_id, service_id, validate_name, validate_version};
use crate::ports::{self, PortCheck};
use crate::registry::{compare_versions, platform_key, IndexEntry, LoadedIndex, Registry};
use crate::settings::{ServiceSettings, Settings, SettingsUpdate};
use crate::state::AppState;
use crate::supervisor::Status;
use crate::{logs, tray};

pub const EVENT_SERVICE_STATUS: &str = "service-status";
pub const EVENT_SERVICES_CHANGED: &str = "services-changed";
pub const EVENT_INSTALL_PROGRESS: &str = "install-progress";

type Cmd<T> = Result<T, AppError>;

// ---------- app & settings ----------

#[derive(Serialize)]
pub struct AppInfo {
    version: String,
    platform: String,
    data_dir: String,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: platform_key(),
        data_dir: state.settings().data_dir.display().to_string(),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, state: State<'_, AppState>, update: SettingsUpdate) -> Cmd<Settings> {
    let before = state.settings();
    if update.data_dir.as_ref().is_some_and(|d| *d != before.data_dir)
        && !state.supervisor.active_ids().await.is_empty()
    {
        return Err(AppError::ServiceRunning);
    }
    let after = state.update_settings(|s| {
        s.apply(update)?;
        Ok(s.clone())
    })?;
    state.layout().ensure()?;
    if after.language != before.language {
        tray::refresh(&app, &after.language);
    }
    if after.data_dir != before.data_dir {
        let _ = app.emit(EVENT_SERVICES_CHANGED, ());
    }
    Ok(after)
}

// ---------- registry & modules ----------

#[derive(Serialize)]
pub struct ModuleView {
    #[serde(flatten)]
    entry: IndexEntry,
    installed: Vec<String>,
    update_available: bool,
}

#[derive(Serialize)]
pub struct IndexView {
    modules: Vec<ModuleView>,
    fetched_at: u64,
    stale: bool,
    platform: String,
}

async fn load_index(state: &AppState, force: bool) -> AppResult<LoadedIndex> {
    let settings = state.settings();
    let layout = state.layout();
    Registry { client: &state.client, base_url: &settings.registry_url, layout: &layout }
        .index(force)
        .await
}

#[tauri::command]
pub async fn registry_index(state: State<'_, AppState>, force: bool) -> Cmd<IndexView> {
    let loaded = load_index(&state, force).await?;
    let installed = modules::list_installed(&state.layout());
    let modules = loaded
        .index
        .modules
        .into_iter()
        .map(|entry| {
            let versions: Vec<String> = installed
                .iter()
                .filter(|m| m.name == entry.name)
                .map(|m| m.version.clone())
                .collect();
            let newest = versions.iter().max_by(|a, b| compare_versions(a, b));
            let update_available =
                newest.is_some_and(|v| compare_versions(&entry.latest, v).is_gt());
            ModuleView { entry, installed: versions, update_available }
        })
        .collect();
    Ok(IndexView { modules, fetched_at: loaded.fetched_at, stale: loaded.stale, platform: platform_key() })
}

#[derive(Serialize)]
pub struct VersionView {
    version: String,
    available: bool,
    installed: bool,
}

#[derive(Serialize)]
pub struct ManifestView {
    name: String,
    category: String,
    has_service: bool,
    default_port: Option<u16>,
    versions: Vec<VersionView>,
}

async fn find_entry(state: &AppState, name: &str) -> AppResult<IndexEntry> {
    validate_name(name)?;
    load_index(state, false)
        .await?
        .index
        .modules
        .into_iter()
        .find(|m| m.name == name)
        .ok_or_else(|| AppError::NotFound(name.into()))
}

#[tauri::command]
pub async fn registry_module(state: State<'_, AppState>, name: String) -> Cmd<ManifestView> {
    let entry = find_entry(&state, &name).await?;
    let settings = state.settings();
    let layout = state.layout();
    let manifest = Registry { client: &state.client, base_url: &settings.registry_url, layout: &layout }
        .manifest(&entry)
        .await?;
    let platform = platform_key();
    let installed = modules::list_installed(&layout);
    let mut versions: Vec<VersionView> = manifest
        .versions
        .iter()
        .map(|v| VersionView {
            version: v.version.clone(),
            available: v.assets.contains_key(&platform),
            installed: installed.iter().any(|m| m.name == name && m.version == v.version),
        })
        .collect();
    versions.sort_by(|a, b| compare_versions(&b.version, &a.version));
    Ok(ManifestView {
        name: manifest.name,
        category: manifest.category,
        has_service: manifest.service.is_some(),
        default_port: manifest.service.as_ref().and_then(modules::default_port),
        versions,
    })
}

#[derive(Clone, Serialize)]
struct InstallProgress<'a> {
    name: &'a str,
    version: &'a str,
    stage: &'a str,
    downloaded: u64,
    total: Option<u64>,
}

/// Holds a module name in `busy_modules` for the guard's lifetime.
struct BusyGuard<'a> {
    state: &'a AppState,
    name: String,
}

impl<'a> BusyGuard<'a> {
    fn acquire(state: &'a AppState, name: &str) -> AppResult<Self> {
        let mut set = state.busy_modules.lock().unwrap_or_else(|e| e.into_inner());
        if !set.insert(name.to_string()) {
            return Err(AppError::Busy);
        }
        Ok(Self { state, name: name.to_string() })
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        let mut set = self.state.busy_modules.lock().unwrap_or_else(|e| e.into_inner());
        set.remove(&self.name);
    }
}

#[tauri::command]
pub async fn install_module(app: AppHandle, state: State<'_, AppState>, name: String, version: String) -> Cmd<()> {
    validate_name(&name)?;
    validate_version(&version)?;
    let _guard = BusyGuard::acquire(&state, &name)?;
    let entry = find_entry(&state, &name).await?;
    let settings = state.settings();
    let layout = state.layout();
    let manifest = Registry { client: &state.client, base_url: &settings.registry_url, layout: &layout }
        .manifest(&entry)
        .await?;

    let emit = |stage: &str, downloaded: u64, total: Option<u64>| {
        let _ = app.emit(
            EVENT_INSTALL_PROGRESS,
            InstallProgress { name: &name, version: &version, stage, downloaded, total },
        );
    };
    let mut last = 0u64;
    let id = service_id(&name, &version);
    let result = modules::install(
        &state.client,
        &layout,
        &manifest,
        &version,
        |spec| state.port_for(&id, spec),
        |done, total| {
            // Throttle: one event per 256 KiB (and the final chunk).
            if done == 0 || done - last >= 256 * 1024 || Some(done) == total {
                last = done;
                emit("download", done, total);
            }
        },
    )
    .await;
    emit(if result.is_ok() { "done" } else { "failed" }, 0, None);
    result?;
    let _ = app.emit(EVENT_SERVICES_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub async fn uninstall_module(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    version: String,
    delete_data: bool,
) -> Cmd<()> {
    validate_name(&name)?;
    validate_version(&version)?;
    let id = service_id(&name, &version);
    if state.supervisor.is_active(&id).await {
        return Err(AppError::ServiceRunning);
    }
    let _guard = BusyGuard::acquire(&state, &name)?;
    modules::uninstall(&state.layout(), &name, &version, delete_data)?;
    state.update_settings(|s| s.set_service(&id, ServiceSettings::default()))?;
    let _ = app.emit(EVENT_SERVICES_CHANGED, ());
    Ok(())
}

// ---------- services ----------

#[derive(Serialize)]
pub struct ServiceInfo {
    id: String,
    name: String,
    version: String,
    category: String,
    status: Status,
    pid: Option<u32>,
    started_at: Option<u64>,
    recent_crashes: usize,
    port: Option<u16>,
    autostart: bool,
}

async fn service_info(state: &AppState, m: &InstalledModule) -> Option<ServiceInfo> {
    let spec = m.service.as_ref()?;
    let id = service_id(&m.name, &m.version);
    let snap = state.supervisor.snapshot(&id).await;
    Some(ServiceInfo {
        port: state.port_for(&id, spec),
        autostart: state.settings().service(&id).autostart,
        name: m.name.clone(),
        version: m.version.clone(),
        category: m.category.clone(),
        status: snap.status,
        pid: snap.pid,
        started_at: snap.started_at,
        recent_crashes: snap.recent_crashes,
        id,
    })
}

#[tauri::command]
pub async fn list_services(state: State<'_, AppState>) -> Cmd<Vec<ServiceInfo>> {
    let mut out = Vec::new();
    for m in modules::list_installed(&state.layout()) {
        if let Some(info) = service_info(&state, &m).await {
            out.push(info);
        }
    }
    Ok(out)
}

/// Ports configured for other installed services (avoided when suggesting).
fn reserved_ports(state: &AppState, except: &str) -> Vec<u16> {
    modules::list_installed(&state.layout())
        .iter()
        .filter_map(|m| {
            let id = service_id(&m.name, &m.version);
            (id != except).then(|| state.port_for(&id, m.service.as_ref()?))?
        })
        .collect()
}

pub async fn start_service_inner(state: &AppState, id: &str) -> AppResult<()> {
    if state.supervisor.is_active(id).await {
        return Ok(());
    }
    let (launch, port, _) = state.launch_for(id)?;
    if let Some(port) = port {
        if !ports::is_free(port) {
            let suggestion = ports::suggest(port, &reserved_ports(state, id));
            return Err(AppError::PortInUse { port, suggestion });
        }
    }
    state.supervisor.start(launch).await
}

#[tauri::command]
pub async fn start_service(state: State<'_, AppState>, id: String) -> Cmd<()> {
    parse_service_id(&id)?;
    start_service_inner(&state, &id).await
}

#[tauri::command]
pub async fn stop_service(state: State<'_, AppState>, id: String) -> Cmd<()> {
    parse_service_id(&id)?;
    state.supervisor.stop(&id).await
}

#[tauri::command]
pub async fn restart_service(state: State<'_, AppState>, id: String) -> Cmd<()> {
    parse_service_id(&id)?;
    state.supervisor.stop(&id).await?;
    start_service_inner(&state, &id).await
}

#[derive(Serialize)]
pub struct BulkFailure {
    pub id: String,
    pub error: AppError,
}

pub async fn start_all_inner(state: &AppState) -> Vec<BulkFailure> {
    let mut failures = Vec::new();
    for m in modules::list_installed(&state.layout()) {
        if m.service.is_none() {
            continue;
        }
        let id = service_id(&m.name, &m.version);
        if let Err(error) = start_service_inner(state, &id).await {
            failures.push(BulkFailure { id, error });
        }
    }
    failures
}

#[tauri::command]
pub async fn start_all(state: State<'_, AppState>) -> Cmd<Vec<BulkFailure>> {
    Ok(start_all_inner(&state).await)
}

#[tauri::command]
pub async fn stop_all(state: State<'_, AppState>) -> Cmd<()> {
    state.supervisor.stop_all().await;
    Ok(())
}

#[tauri::command]
pub async fn set_service_options(
    state: State<'_, AppState>,
    id: String,
    port: Option<u16>,
    autostart: Option<bool>,
) -> Cmd<ServiceInfo> {
    let (name, version) = parse_service_id(&id)?;
    let layout = state.layout();
    let m = modules::load_installed(&layout, &name, &version)?;
    let spec = m.service.clone().ok_or(AppError::NoService)?;
    if port == Some(0) {
        return Err(AppError::InvalidInput("port".into()));
    }
    let port_changed = port.is_some_and(|p| Some(p) != state.port_for(&id, &spec));
    if port_changed && state.supervisor.is_active(&id).await {
        return Err(AppError::ServiceRunning);
    }
    state.update_settings(|s| {
        let mut cur = s.service(&id);
        if let Some(p) = port {
            cur.port = Some(p);
        }
        if let Some(a) = autostart {
            cur.autostart = a;
        }
        s.set_service(&id, cur)
    })?;
    if port_changed {
        modules::write_configs(&layout, &m, port, true)?;
    }
    service_info(&state, &m).await.ok_or(AppError::NoService)
}

#[tauri::command]
pub fn check_port(state: State<'_, AppState>, port: u16) -> Cmd<PortCheck> {
    if port == 0 {
        return Err(AppError::InvalidInput("port".into()));
    }
    Ok(ports::check(port, &reserved_ports(&state, "")))
}

// ---------- logs ----------

#[tauri::command]
pub fn list_logs(state: State<'_, AppState>) -> Vec<logs::LogFile> {
    logs::list(&state.layout().logs())
}

#[tauri::command]
pub fn read_log(state: State<'_, AppState>, name: String, offset: Option<u64>) -> Cmd<logs::LogChunk> {
    logs::read(&state.layout().logs(), &name, offset)
}

#[tauri::command]
pub fn clear_log(state: State<'_, AppState>, name: String) -> Cmd<()> {
    logs::clear(&state.layout().logs(), &name)
}

// ---------- open in OS ----------

#[tauri::command]
pub async fn open_service_url(app: AppHandle, state: State<'_, AppState>, id: String) -> Cmd<()> {
    let (name, version) = parse_service_id(&id)?;
    let m = modules::load_installed(&state.layout(), &name, &version)?;
    let port = m
        .service
        .as_ref()
        .and_then(|s| state.port_for(&id, s))
        .ok_or(AppError::NoService)?;
    app.opener()
        .open_url(format!("http://localhost:{port}/"), None::<&str>)
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// Opens one of Stackyard's own folders; arbitrary paths are not accepted.
#[tauri::command]
pub fn open_folder(app: AppHandle, state: State<'_, AppState>, kind: String, id: Option<String>) -> Cmd<()> {
    let layout = state.layout();
    let path = match (kind.as_str(), id) {
        ("root", _) => layout.root.clone(),
        ("www", _) => layout.www(),
        ("logs", _) => layout.logs(),
        ("config", Some(id)) => {
            let (n, v) = parse_service_id(&id)?;
            layout.conf_dir(&n, &v)
        }
        ("module", Some(id)) => {
            let (n, v) = parse_service_id(&id)?;
            layout.module_dir(&n, &v)
        }
        _ => return Err(AppError::InvalidInput("folder kind".into())),
    };
    if !path.exists() {
        return Err(AppError::NotFound(kind));
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::Internal(e.to_string()))
}
