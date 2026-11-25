use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, AppResult};
use crate::modules::{self, InstalledModule};
use crate::paths::{parse_service_id, service_id, validate_name, validate_version};
use crate::ports::{self, PortCheck};
use crate::registry::{compare_versions, platform_key, IndexEntry, LoadedIndex, ModuleManifest};
use crate::settings::{ServiceSettings, Settings, SettingsUpdate};
use crate::state::AppState;
use crate::supervisor::Status;
use crate::{logs, panel, runtimes, tray};

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
    /// Folder holding the default runtimes' shims (to be added to PATH).
    bin_dir: String,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: platform_key(),
        data_dir: state.settings().data_dir.display().to_string(),
        bin_dir: state.layout().shim_dir().display().to_string(),
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
    let layout = state.layout();
    state.registry(&layout).index(force)
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
    /// This version is the runtime's default (on PATH).
    default: bool,
}

#[derive(Serialize)]
pub struct ManifestView {
    name: String,
    category: String,
    has_service: bool,
    default_port: Option<u16>,
    /// Has command-line tools that can be made the default (see `runtimes`).
    runtime: bool,
    /// Modules installed along with it (e.g. `phpmyadmin` for MySQL).
    companions: Vec<String>,
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
    let layout = state.layout();
    let manifest = state.registry(&layout).manifest(&entry).await?;
    let platform = platform_key();
    let installed = modules::list_installed(&layout);
    let default = state.settings().runtimes.get(&name).cloned();
    let mut versions: Vec<VersionView> = manifest
        .versions
        .iter()
        .map(|v| VersionView {
            version: v.version.clone(),
            available: v.assets.contains_key(&platform),
            installed: installed.iter().any(|m| m.name == name && m.version == v.version),
            default: default.as_deref() == Some(v.version.as_str()),
        })
        .collect();
    versions.sort_by(|a, b| compare_versions(&b.version, &a.version));
    let runtime = !manifest.platform_shims().is_empty();
    Ok(ManifestView {
        companions: manifest.companions.clone(),
        name: manifest.name,
        category: manifest.category,
        has_service: manifest.service.is_some(),
        default_port: manifest.service.as_ref().and_then(modules::default_port),
        runtime,
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

/// What an install added besides the requested module.
#[derive(Serialize, Default, Debug)]
pub struct InstallOutcome {
    /// Companions installed along with it (e.g. `phpmyadmin` for MySQL).
    companions: Vec<String>,
    /// Companions that could not be installed: `"<name>: <reason>"`.
    companion_errors: Vec<String>,
}

/// Install progress callback: `(name, version, stage, downloaded, total)`.
pub type Progress<'a> = dyn Fn(&str, &str, &str, u64, Option<u64>) + Send + Sync + 'a;

#[tauri::command]
pub async fn install_module(app: AppHandle, state: State<'_, AppState>, name: String, version: String) -> Cmd<InstallOutcome> {
    let emit = |name: &str, version: &str, stage: &str, downloaded: u64, total: Option<u64>| {
        let _ = app.emit(EVENT_INSTALL_PROGRESS, InstallProgress { name, version, stage, downloaded, total });
    };
    let result = install_with_companions(&state, &name, &version, &emit).await;
    let _ = app.emit(EVENT_SERVICES_CHANGED, ());
    Ok(result?)
}

/// Installs `name@version`, then each of its companions that is not
/// installed yet (latest version available for this platform). A companion
/// that fails is reported, never undoing the main install.
pub(crate) async fn install_with_companions(
    state: &AppState,
    name: &str,
    version: &str,
    progress: &Progress<'_>,
) -> AppResult<InstallOutcome> {
    validate_name(name)?;
    validate_version(version)?;
    let entry = find_entry(state, name).await?;
    let layout = state.layout();
    let manifest = state.registry(&layout).manifest(&entry).await?;
    install_manifest(state, &manifest, version, progress).await?;

    let mut outcome = InstallOutcome::default();
    for companion in &manifest.companions {
        if modules::list_installed(&layout).iter().any(|m| &m.name == companion) {
            continue;
        }
        let installed = match latest_for_platform(state, companion).await {
            Ok(Some((manifest, version))) => install_manifest(state, &manifest, &version, progress).await.map(|()| true),
            Ok(None) => Ok(false), // no build for this platform
            Err(e) => Err(e),
        };
        match installed {
            Ok(true) => outcome.companions.push(companion.clone()),
            Ok(false) => {}
            Err(e) => {
                eprintln!("[stackyard] companion {companion} of {name}: {e}");
                outcome.companion_errors.push(format!("{companion}: {e}"));
            }
        }
    }
    Ok(outcome)
}

/// The manifest of `name` and its newest version built for this platform, if any.
async fn latest_for_platform(state: &AppState, name: &str) -> AppResult<Option<(ModuleManifest, String)>> {
    let entry = find_entry(state, name).await?;
    let layout = state.layout();
    let manifest = state.registry(&layout).manifest(&entry).await?;
    let platform = platform_key();
    let latest = manifest
        .versions
        .iter()
        .filter(|v| v.assets.contains_key(&platform))
        .map(|v| v.version.clone())
        .max_by(|a, b| compare_versions(a, b));
    Ok(latest.map(|v| (manifest, v)))
}

/// Updates a web panel (phpMyAdmin, pgAdmin) in place: unlike databases it is
/// a single-instance tool, so the newest version replaces the installed ones,
/// keeping the port, autostart and data of the newest of them.
#[tauri::command]
pub async fn update_panel(app: AppHandle, state: State<'_, AppState>, name: String) -> Cmd<String> {
    let emit = |name: &str, version: &str, stage: &str, downloaded: u64, total: Option<u64>| {
        let _ = app.emit(EVENT_INSTALL_PROGRESS, InstallProgress { name, version, stage, downloaded, total });
    };
    let result = update_panel_inner(&state, &name, &emit).await;
    let _ = app.emit(EVENT_SERVICES_CHANGED, ());
    Ok(result?)
}

pub(crate) async fn update_panel_inner(state: &AppState, name: &str, progress: &Progress<'_>) -> AppResult<String> {
    validate_name(name)?;
    let layout = state.layout();
    let old: Vec<_> = modules::list_installed(&layout).into_iter().filter(|m| m.name == name).collect();
    let Some(current) = old.iter().max_by(|a, b| compare_versions(&a.version, &b.version)) else {
        return Err(AppError::NotFound(name.to_string()));
    };
    if current.panel.is_none() {
        return Err(AppError::InvalidInput(format!("{name} is not a panel")));
    }
    let (manifest, latest) = latest_for_platform(state, name).await?.ok_or(AppError::UnsupportedPlatform)?;
    if old.len() == 1 && current.version == latest {
        return Ok(latest);
    }
    for m in &old {
        state.supervisor.stop(&service_id(name, &m.version)).await?;
    }
    let new_id = service_id(name, &latest);
    if !old.iter().any(|m| m.version == latest) {
        // Before installing, so the new version is configured with the kept port.
        let kept = state.settings().services.get(&service_id(name, &current.version)).cloned();
        if let Some(kept) = kept {
            state.update_settings(|s| s.set_service(&new_id, kept))?;
        }
        if let Err(e) = install_manifest(state, &manifest, &latest, progress).await {
            let _ = state.update_settings(|s| s.set_service(&new_id, ServiceSettings::default()));
            return Err(e);
        }
    }
    let _guard = BusyGuard::acquire(state, name)?;
    let new_data = layout.data_dir(name, &latest);
    let old_data = layout.data_dir(name, &current.version);
    if current.version != latest && old_data.exists() && is_empty_dir(&new_data) {
        let _ = std::fs::remove_dir(&new_data);
        std::fs::rename(&old_data, &new_data)?;
    }
    for m in old.iter().filter(|m| m.version != latest) {
        modules::uninstall(&layout, name, &m.version, false)?;
        state.update_settings(|s| s.set_service(&service_id(name, &m.version), ServiceSettings::default()))?;
    }
    Ok(latest)
}

fn is_empty_dir(path: &std::path::Path) -> bool {
    std::fs::read_dir(path).map_or(true, |mut d| d.next().is_none())
}

async fn install_manifest(state: &AppState, manifest: &ModuleManifest, version: &str, progress: &Progress<'_>) -> AppResult<()> {
    let name = manifest.name.as_str();
    let _guard = BusyGuard::acquire(state, name)?;
    let layout = state.layout();
    let mut last = 0u64;
    let id = service_id(name, version);
    let result = modules::install(
        &state.client,
        &layout,
        manifest,
        version,
        |spec| state.port_for(&id, spec),
        |stage, done, total| {
            // Throttle downloads: one event per 256 KiB (and the first/final chunk).
            if stage != "download" || done == 0 || done.abs_diff(last) >= 256 * 1024 || Some(done) == total {
                last = done;
                progress(name, version, stage, done, total);
            }
        },
    )
    .await;
    progress(name, version, if result.is_ok() { "done" } else { "failed" }, 0, None);
    let installed = result?;
    // The first installed version of a runtime becomes the default.
    if !installed.shims.is_empty() && !state.settings().runtimes.contains_key(name) {
        match runtimes::activate(&layout, &installed) {
            Ok(()) => state.update_settings(|s| {
                s.runtimes.insert(name.to_string(), version.to_string());
                Ok(())
            })?,
            Err(e) => eprintln!("[stackyard] default {name} {version}: {e}"),
        }
    }
    Ok(())
}

/// Makes `version` the runtime's default (its tools on PATH via shims), or
/// removes the default when `version` is `None`.
#[tauri::command]
pub async fn set_runtime(state: State<'_, AppState>, name: String, version: Option<String>) -> Cmd<()> {
    validate_name(&name)?;
    let _guard = BusyGuard::acquire(&state, &name)?;
    let layout = state.layout();
    if let Some(v) = &version {
        runtimes::activate(&layout, &modules::load_installed(&layout, &name, v)?)?;
    }
    let previous = state.settings().runtimes.get(&name).cloned();
    if let Some(old) = previous.filter(|old| Some(old) != version.as_ref()) {
        if let Ok(m) = modules::load_installed(&layout, &name, &old) {
            runtimes::deactivate(&layout, &m)?; // shims now owned by `version` are kept
        }
    }
    state.update_settings(|s| {
        match &version {
            Some(v) => s.runtimes.insert(name.clone(), v.clone()),
            None => s.runtimes.remove(&name),
        };
        Ok(())
    })?;
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
    let layout = state.layout();
    let is_default = state.settings().runtimes.get(&name) == Some(&version);
    if is_default {
        runtimes::deactivate(&layout, &modules::load_installed(&layout, &name, &version)?)?;
    }
    modules::uninstall(&layout, &name, &version, delete_data)?;
    state.update_settings(|s| {
        if is_default {
            s.runtimes.remove(&name);
        }
        s.set_service(&id, ServiceSettings::default())
    })?;
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
    /// Database login user, when the module generates credentials.
    credentials_user: Option<String>,
    initialized: bool,
    /// Modules this service is a web panel for (e.g. `mysql`, `mariadb` for phpMyAdmin).
    panel_for: Vec<String>,
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
        credentials_user: spec.credentials.as_ref().map(|c| c.user.clone()),
        initialized: spec.init.is_none() || modules::is_initialized(&state.layout(), m),
        panel_for: m.panel.as_ref().map(|p| p.targets.clone()).unwrap_or_default(),
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
    let (launch, port, m) = state.launch_for(id)?;
    if let Some(port) = port {
        if !ports::is_free(port) {
            let suggestion = ports::suggest(port, &reserved_ports(state, id));
            return Err(AppError::PortInUse { port, suggestion });
        }
    }
    let layout = state.layout();
    if !modules::is_initialized(&layout, &m) && m.service.as_ref().is_some_and(|s| s.init.is_some()) {
        // Same guard as install/uninstall: one init per module at a time.
        let _guard = BusyGuard::acquire(state, &m.name)?;
        modules::initialize(&layout, &m, port, &launch.log_file).await?;
    }
    if m.panel.is_some() {
        let port = port.ok_or(AppError::NoService)?;
        let targets = panel::targets(&m, &modules::list_installed(&layout), |id, spec| state.port_for(id, spec));
        panel::prepare(&layout, &m, port, targets)?;
    }
    state.supervisor.start(launch).await
}

#[derive(Serialize)]
pub struct Credentials {
    user: String,
    password: String,
    host: &'static str,
    port: Option<u16>,
    storage: crate::secrets::Storage,
}

/// Returns the generated database login. Only Stackyard's own services.
#[tauri::command]
pub fn get_credentials(state: State<'_, AppState>, id: String) -> Cmd<Credentials> {
    let (name, version) = parse_service_id(&id)?;
    let layout = state.layout();
    let m = modules::load_installed(&layout, &name, &version)?;
    let spec = m.service.as_ref().ok_or(AppError::NoService)?;
    let user = spec.credentials.as_ref().ok_or(AppError::NoService)?.user.clone();
    let (password, storage) = crate::secrets::load(&layout, &name, &version)?;
    Ok(Credentials { user, password, host: "127.0.0.1", port: state.port_for(&id, spec), storage })
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

const PANEL_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Starts a web panel if needed, waits until it accepts connections, then
/// opens it in the browser with its current access token.
#[tauri::command]
pub async fn open_panel(app: AppHandle, state: State<'_, AppState>, id: String) -> Cmd<()> {
    let (name, version) = parse_service_id(&id)?;
    let layout = state.layout();
    let m = modules::load_installed(&layout, &name, &version)?;
    let spec = m.service.as_ref().filter(|_| m.panel.is_some()).ok_or(AppError::NoService)?;
    let port = state.port_for(&id, spec).ok_or(AppError::NoService)?;
    start_service_inner(&state, &id).await?;
    let deadline = std::time::Instant::now() + PANEL_READY_TIMEOUT;
    while !panel::is_listening(port) {
        let status = state.supervisor.snapshot(&id).await.status;
        if matches!(status, Status::Stopped | Status::Crashed) {
            return Err(AppError::Spawn(format!("{id} exited before it was ready")));
        }
        if std::time::Instant::now() > deadline {
            return Err(AppError::Spawn(format!("{id} did not become ready")));
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    app.opener()
        .open_url(panel::url(&layout, &m)?, None::<&str>)
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
