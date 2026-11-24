mod commands;
mod download;
mod error;
mod extract;
mod logs;
mod modules;
mod paths;
mod ports;
mod registry;
mod settings;
mod state;
mod supervisor;
mod tray;
mod updater;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tauri::{Emitter, Manager, RunEvent, WindowEvent};

use crate::paths::service_id;
use crate::settings::Settings;
use crate::state::AppState;
use crate::supervisor::Supervisor;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::PendingUpdate::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&settings_path);
            let lang = settings.language.clone();

            let client = reqwest::Client::builder()
                .user_agent(format!("Stackyard/{}", app.package_info().version))
                .connect_timeout(Duration::from_secs(15))
                .build()?;

            let emitter = handle.clone();
            let supervisor = Supervisor::new(Arc::new(move |snap| {
                let _ = emitter.emit(commands::EVENT_SERVICE_STATUS, snap);
            }));

            let state = AppState {
                settings: RwLock::new(settings),
                settings_path,
                client,
                supervisor,
                busy_modules: std::sync::Mutex::new(HashSet::new()),
                quitting: AtomicBool::new(false),
            };
            state.layout().ensure()?;
            app.manage(state);
            tray::create(&handle, &lang)?;

            // Auto-start services flagged by the user (PRD §4 service manager).
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                let settings = state.settings();
                for m in modules::list_installed(&state.layout()) {
                    let id = service_id(&m.name, &m.version);
                    if m.service.is_some() && settings.service(&id).autostart {
                        if let Err(e) = commands::start_service_inner(&state, &id).await {
                            eprintln!("[stackyard] autostart {id} failed: {e}");
                        }
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Stackyard (and services) running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_settings,
            commands::update_settings,
            commands::registry_index,
            commands::registry_module,
            commands::install_module,
            commands::uninstall_module,
            commands::list_services,
            commands::start_service,
            commands::stop_service,
            commands::restart_service,
            commands::start_all,
            commands::stop_all,
            commands::set_service_options,
            commands::check_port,
            commands::list_logs,
            commands::read_log,
            commands::clear_log,
            commands::open_service_url,
            commands::open_folder,
            updater::check_update,
            updater::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Stackyard")
        .run(|app, event| {
            // Quitting stops every service first, then exits for real.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                let state = app.state::<AppState>();
                if !state.quitting.swap(true, Ordering::SeqCst) {
                    api.prevent_exit();
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        app.state::<AppState>().supervisor.stop_all().await;
                        app.exit(code.unwrap_or(0));
                    });
                }
            }
        });
}

/// End-to-end check against a real registry (network + real binaries):
/// `STACKYARD_E2E_REGISTRY=http://127.0.0.1:8765 cargo test e2e -- --ignored`
#[cfg(test)]
mod e2e {
    use super::*;
    use crate::registry::Registry;
    use crate::supervisor::Status;

    fn state(dir: &std::path::Path, url: &str) -> AppState {
        let settings = Settings {
            data_dir: dir.join("Stackyard"),
            registry_url: url.into(),
            ..Settings::default()
        };
        AppState {
            settings: RwLock::new(settings),
            settings_path: dir.join("settings.json"),
            client: reqwest::Client::new(),
            supervisor: Supervisor::new(Arc::new(|s| eprintln!("status {} {:?}", s.id, s.status))),
            busy_modules: std::sync::Mutex::new(HashSet::new()),
            quitting: AtomicBool::new(false),
        }
    }

    async fn install(st: &AppState, name: &str) -> String {
        let s = st.settings();
        let layout = st.layout();
        let reg = Registry { client: &st.client, base_url: &s.registry_url, layout: &layout };
        let idx = reg.index(true).await.expect("signed index");
        let entry = idx.index.modules.iter().find(|m| m.name == name).unwrap().clone();
        let manifest = reg.manifest(&entry).await.expect("manifest");
        let id = service_id(name, &entry.latest);
        modules::install(&st.client, &layout, &manifest, &entry.latest, |sp| st.port_for(&id, sp), |_, _| {})
            .await
            .expect("install");
        id
    }

    #[tokio::test]
    #[ignore]
    async fn install_start_serve_stop() {
        let Ok(url) = std::env::var("STACKYARD_E2E_REGISTRY") else { return };
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path(), &url);
        st.layout().ensure().unwrap();

        let nginx = install(&st, "nginx").await;
        let php = install(&st, "php").await;
        for id in [&nginx, &php] {
            commands::start_service_inner(&st, id).await.expect("start");
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
        for id in [&nginx, &php] {
            assert_eq!(st.supervisor.snapshot(id).await.status, Status::Running, "{id}");
        }

        let body = reqwest::get("http://127.0.0.1:8080/").await.unwrap().text().await.unwrap();
        assert!(body.contains("It works!"), "nginx served: {body}");
        assert!(!ports::is_free(9000), "php-fpm listening");

        // Port conflict is reported with a suggestion.
        let (launch, _, _) = st.launch_for(&nginx).unwrap();
        st.supervisor.stop(&nginx).await.unwrap();
        let blocker = std::net::TcpListener::bind("127.0.0.1:8080").unwrap();
        match commands::start_service_inner(&st, &nginx).await {
            Err(error::AppError::PortInUse { port: 8080, suggestion: Some(_) }) => {}
            other => panic!("expected PortInUse, got {other:?}"),
        }
        drop(blocker);
        drop(launch);

        st.supervisor.stop_all().await;
        assert_eq!(st.supervisor.snapshot(&php).await.status, Status::Stopped);
        assert!(ports::is_free(9000), "php-fpm stopped (no orphan workers)");

        let (n, v) = paths::parse_service_id(&php).unwrap();
        modules::uninstall(&st.layout(), &n, &v, false).unwrap();
        assert!(!st.layout().module_dir(&n, &v).exists());
    }
}
