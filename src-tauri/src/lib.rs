mod commands;
mod download;
mod error;
mod extract;
mod logs;
mod modules;
mod panel;
mod paths;
mod ports;
mod registry;
mod secrets;
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
            commands::open_panel,
            commands::open_folder,
            commands::get_credentials,
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
        let result =
            modules::install(&st.client, &layout, &manifest, &entry.latest, |sp| st.port_for(&id, sp), |_, _, _| {}).await;
        if let Err(e) = result {
            let log = std::fs::read_to_string(layout.logs().join(format!("{name}-{}.log", entry.latest))).unwrap_or_default();
            let tail: Vec<&str> = log.lines().rev().take(40).collect();
            panic!("install {name}: {e:?}\n{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
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

        for id in [&php, &nginx] {
            let (n, v) = paths::parse_service_id(id).unwrap();
            modules::uninstall(&st.layout(), &n, &v, false).unwrap();
            assert!(!st.layout().module_dir(&n, &v).exists());
            assert!(!st.layout().socket_dir(&n, &v).exists());
        }
    }

    fn client(st: &AppState, id: &str, bin: &str) -> std::path::PathBuf {
        let (n, v) = paths::parse_service_id(id).unwrap();
        st.layout().module_dir(&n, &v).join("bin").join(bin)
    }

    #[tokio::test]
    #[ignore]
    async fn databases_init_auth_restart() {
        let Ok(url) = std::env::var("STACKYARD_E2E_REGISTRY") else { return };
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path(), &url);
        st.layout().ensure().unwrap();

        let mysql = install(&st, "mysql").await;
        let pg = install(&st, "postgresql").await;
        // Non-default ports so a system MySQL/PostgreSQL never interferes.
        st.update_settings(|s| {
            s.set_service(&mysql, settings::ServiceSettings { port: Some(53306), autostart: false })?;
            s.set_service(&pg, settings::ServiceSettings { port: Some(55432), autostart: false })
        })
        .unwrap();
        let layout = st.layout();
        for id in [&mysql, &pg] {
            let (n, v) = paths::parse_service_id(id).unwrap();
            let m = modules::load_installed(&layout, &n, &v).unwrap();
            modules::write_configs(&layout, &m, st.port_for(id, m.service.as_ref().unwrap()), true).unwrap();
            commands::start_service_inner(&st, id).await.expect("init + start");
            assert!(modules::is_initialized(&layout, &m), "{id} initialised");
            assert!(!layout.run_dir(&n, &v).join("init").exists(), "init secrets removed");
        }
        tokio::time::sleep(Duration::from_secs(6)).await;
        for f in logs::list(&layout.logs()) {
            let c = logs::read(&layout.logs(), &f.name, None).unwrap().content;
            let tail: Vec<&str> = c.lines().rev().take(12).collect();
            eprintln!("--- {} ---\n{}", f.name, tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }

        let pw = |id: &str| {
            let (n, v) = paths::parse_service_id(id).unwrap();
            secrets::load(&st.layout(), &n, &v).unwrap().0
        };
        let (mysql_pw, pg_pw) = (pw(&mysql), pw(&pg));
        let mysql_query = |password: &str| {
            std::process::Command::new(client(&st, &mysql, "mysql"))
                .args(["--no-defaults", "-h127.0.0.1", "-P53306", "-uroot", "-Nse", "SELECT 41+1"])
                .env("MYSQL_PWD", password)
                .output()
                .unwrap()
        };
        let pg_query = |password: &str| {
            std::process::Command::new(client(&st, &pg, "psql"))
                .args(["-h", "127.0.0.1", "-p", "55432", "-U", "postgres", "-d", "postgres", "-tAc", "SELECT 41+1"])
                .env("PGPASSWORD", password)
                .output()
                .unwrap()
        };
        let ok = |o: std::process::Output| String::from_utf8_lossy(&o.stdout).trim().to_string();
        assert_eq!(ok(mysql_query(&mysql_pw)), "42", "mysql login with generated password");
        assert_eq!(ok(pg_query(&pg_pw)), "42", "postgres login with generated password");
        assert!(!mysql_query("wrong").status.success(), "mysql rejects wrong password");
        assert!(!pg_query("wrong").status.success(), "postgres rejects wrong password");

        // Restart: no re-init, same password, data persists.
        st.supervisor.stop_all().await;
        commands::start_service_inner(&st, &pg).await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(pw(&pg), pg_pw);
        assert_eq!(ok(pg_query(&pg_pw)), "42");
        st.supervisor.stop_all().await;
        assert!(ports::is_free(53306) && ports::is_free(55432), "databases stopped");

        // Uninstall keeping data keeps credentials; deleting data removes both.
        let (n, v) = paths::parse_service_id(&pg).unwrap();
        modules::uninstall(&layout, &n, &v, false).unwrap();
        assert!(layout.data_dir(&n, &v).exists() && secrets::load(&layout, &n, &v).is_ok());
        secrets::delete(&layout, &n, &v);
        let (n, v) = paths::parse_service_id(&mysql).unwrap();
        modules::uninstall(&layout, &n, &v, true).unwrap();
        assert!(!layout.data_dir(&n, &v).exists() && secrets::load(&layout, &n, &v).is_err());
    }

    async fn wait_listening(port: u16, secs: u64) {
        for _ in 0..secs * 2 {
            if panel::is_listening(port) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("port {port} never opened");
    }

    /// Status code for a request path sent verbatim (HTTP clients normalise `..`).
    fn raw_status(port: u16, path: &str) -> u16 {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes())
            .unwrap();
        let mut resp = Vec::new();
        let _ = s.read_to_end(&mut resp);
        let text = String::from_utf8_lossy(&resp);
        text.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or_else(|| panic!("{path}: bad response {text:?}"))
    }

    /// Logs in through phpMyAdmin's form (manual cookie jar) and returns the
    /// MySQL server version it reports, or "" when login fails.
    async fn phpmyadmin_login(http: &reqwest::Client, port: u16, user: &str, password: &str) -> String {
        let base = format!("http://127.0.0.1:{port}");
        let mut jar: std::collections::BTreeMap<String, String> = Default::default();
        let keep = |jar: &mut std::collections::BTreeMap<String, String>, r: &reqwest::Response| {
            for c in r.headers().get_all(reqwest::header::SET_COOKIE) {
                let pair = c.to_str().unwrap().split(';').next().unwrap();
                if let Some((k, v)) = pair.split_once('=') {
                    jar.insert(k.into(), v.into());
                }
            }
        };
        let cookie = |jar: &std::collections::BTreeMap<String, String>| {
            jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
        };
        let r = http.get(format!("{base}/")).send().await.unwrap();
        keep(&mut jar, &r);
        let page = r.text().await.unwrap();
        let token = page.split(r#"name="token" value=""#).nth(1).unwrap().split('"').next().unwrap().to_string();
        let form = [
            ("route", "/"),
            ("token", token.as_str()),
            ("set_session", jar.get("phpMyAdmin").map(String::as_str).unwrap_or("")),
            ("pma_username", user),
            ("pma_password", password),
            ("server", "1"),
        ];
        let body: String = form
            .iter()
            .map(|(k, v)| format!("{k}={}", v.bytes().map(|b| format!("%{b:02X}")).collect::<String>()))
            .collect::<Vec<_>>()
            .join("&");
        let r = http
            .post(format!("{base}/index.php?route=/"))
            .header(reqwest::header::COOKIE, cookie(&jar))
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .unwrap();
        keep(&mut jar, &r);
        let r = http.get(format!("{base}/index.php?route=/")).header(reqwest::header::COOKIE, cookie(&jar)).send().await.unwrap();
        let page = r.text().await.unwrap();
        // Logged-in home page lists "Server version: 8.4.x - MySQL Community Server".
        page.split("Server version:").nth(1).map(|s| s.chars().take(40).collect()).unwrap_or_default()
    }

    /// phpMyAdmin (static PHP) for MySQL and pgAdmin (portable Python + pip)
    /// for PostgreSQL: install, auto-filled servers, token-guarded URL.
    #[tokio::test]
    #[ignore]
    async fn panels_manage_databases() {
        let Ok(url) = std::env::var("STACKYARD_E2E_REGISTRY") else { return };
        let tmp = tempfile::tempdir().unwrap();
        let st = Arc::new(state(tmp.path(), &url));
        st.layout().ensure().unwrap();
        // Always stop the services, even when an assertion fails.
        let result = tokio::spawn(panels_body(st.clone())).await;
        st.supervisor.stop_all().await;
        if let Err(e) = result {
            std::panic::resume_unwind(e.into_panic());
        }
        for port in [53306, 58081, 55432, 55050] {
            assert!(ports::is_free(port), "{port} released");
        }
    }

    async fn panels_body(st: Arc<AppState>) {
        let layout = st.layout();
        let mysql = install(&st, "mysql").await;
        let pma = install(&st, "phpmyadmin").await;
        let pg = install(&st, "postgresql").await;
        let pga = install(&st, "pgadmin").await;
        st.update_settings(|s| {
            for (id, port) in [(&mysql, 53306), (&pma, 58081), (&pg, 55432), (&pga, 55050)] {
                s.set_service(id, settings::ServiceSettings { port: Some(port), autostart: false })?;
            }
            Ok(())
        })
        .unwrap();
        for id in [&mysql, &pma, &pg, &pga] {
            let (n, v) = paths::parse_service_id(id).unwrap();
            let m = modules::load_installed(&layout, &n, &v).unwrap();
            modules::write_configs(&layout, &m, st.port_for(id, m.service.as_ref().unwrap()), true).unwrap();
            commands::start_service_inner(&st, id).await.unwrap_or_else(|e| panic!("{id}: {e:?}"));
        }
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let get = |url: String| {
            let http = http.clone();
            async move {
                let r = http.get(&url).send().await.unwrap();
                (r.status().as_u16(), r.text().await.unwrap())
            }
        };

        wait_listening(58081, 30).await;
        let (code, body) = get("http://127.0.0.1:58081/".into()).await;
        assert_eq!(code, 200, "{body}");
        assert!(body.contains(r#"name="server" value="1""#), "MySQL server auto-filled");
        let (n, v) = paths::parse_service_id(&mysql).unwrap();
        let mysql_pw = secrets::load(&layout, &n, &v).unwrap().0;
        let version = phpmyadmin_login(&http, 58081, "root", &mysql_pw).await;
        assert!(version.contains("8.4"), "logged in through phpMyAdmin: {version}");
        assert!(phpmyadmin_login(&http, 58081, "root", "wrong").await.is_empty(), "wrong password rejected");
        for internal in [
            "/libraries/classes/Config.php",
            "/Setup/",
            "/libraries./classes/Config.php",
            "/./vendor/autoload.php",
            "/js/../config.inc.php",
            "/%2e%2e/x",
            "/js/%2e%2e/libraries/classes/Config.php",
        ] {
            assert_eq!(raw_status(58081, internal), 403, "{internal} must not be served");
        }

        wait_listening(55050, 180).await;
        let (n, v) = paths::parse_service_id(&pga).unwrap();
        let panel_url = panel::url(&layout, &modules::load_installed(&layout, &n, &v).unwrap()).unwrap();
        let (code, _) = get("http://127.0.0.1:55050/?key=wrong".into()).await;
        assert_eq!(code, 401, "pgAdmin rejects a wrong key");
        let (code, body) = get(panel_url).await;
        assert!(code == 200 || code == 302, "pgAdmin accepts its key: {code} {body}");
    }
}
