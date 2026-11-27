mod autostart;
mod commands;
mod download;
mod error;
mod extract;
mod admin;
mod hosts;
mod logs;
mod modules;
mod panel;
mod paths;
mod ports;
mod projects;
mod registry;
mod runtimes;
mod secrets;
mod settings;
mod state;
mod supervisor;
mod terminal;
mod tls;
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

/// Runs the elevated hosts-file helper when the process was started as one
/// (`cheesenv --hosts-sync …`); returns its exit code, or `None` to start the app.
pub fn run_helper() -> Option<i32> {
    admin::helper_main(&std::env::args().collect::<Vec<_>>())
}

pub fn run() {
    tauri::Builder::default()
        // Must be registered first: a second launch exits here and just brings
        // the running instance (which may be hidden in the tray) to the front.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(autostart::plugin())
        .manage(updater::PendingUpdate::default())
        .manage(terminal::Terminals::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&settings_path);
            let lang = settings.language.clone();
            let launch_at_login = settings.launch_at_login;

            let client = reqwest::Client::builder()
                .user_agent(format!("CheesEnv/{}", app.package_info().version))
                .connect_timeout(Duration::from_secs(15))
                .build()?;

            let emitter = handle.clone();
            let supervisor = Supervisor::new(Arc::new(move |snap| {
                let _ = emitter.emit(commands::EVENT_SERVICE_STATUS, snap);
            }));

            let state = AppState {
                settings: RwLock::new(settings),
                settings_path,
                registry_url: registry::REGISTRY_URL.into(),
                client,
                supervisor,
                busy_modules: std::sync::Mutex::new(HashSet::new()),
                quitting: AtomicBool::new(false),
            };
            state.layout().ensure()?;
            app.manage(state);
            tray::create(&handle, &lang)?;

            // The window is created hidden; a login launch keeps it in the tray.
            if !autostart::started_hidden() {
                tray::show_main(&handle);
            }
            // Re-sync the login entry (it follows the installed app, not a dev build).
            if !cfg!(debug_assertions) {
                if let Err(e) = autostart::sync(&handle, launch_at_login) {
                    eprintln!("[cheesenv] {e}");
                }
            }

            // Built-in panels: installed/updated in the background on every start.
            let panels_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = panels_handle.state::<AppState>();
                let emit = |name: &str, version: &str, stage: &str, downloaded: u64, total: Option<u64>| {
                    let p = commands::InstallProgress { name, version, stage, downloaded, total };
                    let _ = panels_handle.emit(commands::EVENT_PANELS_SETUP, p);
                };
                for e in commands::ensure_panels(&state, &emit).await {
                    eprintln!("[cheesenv] built-in panel {e}");
                }
                let _ = panels_handle.emit(commands::EVENT_SERVICES_CHANGED, ());
            });

            // Auto-start services flagged by the user (PRD §4 service manager).
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                let settings = state.settings();
                for m in modules::list_installed(&state.layout()) {
                    let id = service_id(&m.name, &m.version);
                    if m.service.is_some() && settings.service(&id).autostart {
                        if let Err(e) = commands::start_service_inner(&state, &id).await {
                            eprintln!("[cheesenv] autostart {id} failed: {e}");
                        }
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps CheesEnv (and services) running in the tray.
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
            commands::set_runtime,
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
            commands::sync_projects,
            commands::create_project,
            commands::sync_hosts,
            commands::set_project_php,
            commands::enable_https,
            commands::enable_low_ports,
            commands::open_project,
            commands::terminal_open,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_close,
            updater::check_update,
            updater::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building CheesEnv")
        .run(|app, event| {
            // Quitting stops every service first, then exits for real.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                let state = app.state::<AppState>();
                if !state.quitting.swap(true, Ordering::SeqCst) {
                    api.prevent_exit();
                    app.state::<terminal::Terminals>().close_all();
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
/// `CHEESENV_E2E_REGISTRY=http://127.0.0.1:8765 cargo test e2e -- --ignored`
#[cfg(test)]
mod e2e {
    use super::*;
    use crate::registry::Registry;
    use crate::supervisor::Status;

    fn state(dir: &std::path::Path) -> AppState {
        let settings = Settings { data_dir: dir.join("CheesEnv"), ..Settings::default() };
        AppState {
            settings: RwLock::new(settings),
            settings_path: dir.join("settings.json"),
            registry_url: std::env::var("CHEESENV_E2E_REGISTRY").unwrap_or_default(),
            client: reqwest::Client::new(),
            supervisor: Supervisor::new(Arc::new(|s| eprintln!("status {} {:?}", s.id, s.status))),
            busy_modules: std::sync::Mutex::new(HashSet::new()),
            quitting: AtomicBool::new(false),
        }
    }

    async fn install(st: &AppState, name: &str) -> String {
        install_version(st, name, None).await
    }

    /// Installs `version` of a module (the latest when `None`); returns its service id.
    async fn install_version(st: &AppState, name: &str, version: Option<&str>) -> String {
        let url = std::env::var("CHEESENV_E2E_REGISTRY").expect("CHEESENV_E2E_REGISTRY");
        let layout = st.layout();
        let reg = Registry { client: &st.client, base_url: &url, layout: &layout };
        let idx = reg.index(true).await.expect("signed index");
        let entry = idx.index.modules.iter().find(|m| m.name == name).unwrap().clone();
        let manifest = reg.manifest(&entry).await.expect("manifest");
        let version = version.unwrap_or(&entry.latest).to_string();
        let id = service_id(name, &version);
        let result = modules::install(&st.client, &layout, &manifest, &version, |sp| st.port_for(&id, sp), |_, _, _| {}).await;
        if let Err(e) = result {
            let log = std::fs::read_to_string(layout.logs().join(format!("{name}-{version}.log"))).unwrap_or_default();
            let tail: Vec<&str> = log.lines().rev().take(40).collect();
            panic!("install {name}: {e:?}\n{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
        id
    }

    #[tokio::test]
    #[ignore]
    async fn install_start_serve_stop() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
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

    /// `www/<name>` is served at `<name>.test` (PHP through PHP-FPM), and a
    /// project added while nginx runs is picked up by `apply_vhosts`.
    #[tokio::test]
    #[ignore]
    async fn project_vhosts_serve_php() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        st.layout().ensure().unwrap();
        let layout = st.layout();
        let nginx = install(&st, "nginx").await;
        let php = install(&st, "php").await;
        // Like an install from the registry before the vhost include existed.
        let (n, v) = paths::parse_service_id(&nginx).unwrap();
        let mut old = modules::load_installed(&layout, &n, &v).unwrap();
        let tpl = old.config_templates.get_mut("nginx.conf").unwrap();
        let start = tpl.find("\n    # Projects").expect("include in template");
        tpl.replace_range(start..tpl.rfind('}').unwrap(), "\n");
        assert!(!projects::supports_vhosts(&old));
        modules::save_installed(&layout, &old).unwrap();
        modules::write_configs(&layout, &old, Some(8080), true).unwrap();

        std::fs::create_dir_all(layout.www().join("toko/public")).unwrap();
        std::fs::write(layout.www().join("toko/public/index.php"), "<?php echo 'php:' . $_SERVER['HTTP_HOST'] . '@' . PHP_MAJOR_VERSION . '.' . PHP_MINOR_VERSION . ($_SERVER['HTTPS'] ?? '');").unwrap();
        for id in [&php, &nginx] {
            commands::start_service_inner(&st, id).await.expect("start");
        }
        tokio::time::sleep(Duration::from_millis(800)).await;

        let client = reqwest::Client::new();
        let get = |host: &'static str, path: &'static str| {
            let client = client.clone();
            async move {
                let r = client.get(format!("http://127.0.0.1:8080{path}")).header("Host", host).send().await.unwrap();
                (r.status().as_u16(), r.text().await.unwrap())
            }
        };
        assert!(projects::supports_vhosts(&modules::load_installed(&layout, &n, &v).unwrap()), "include added on start");
        assert_eq!(get("toko.test", "/").await, (200, "php:toko.test@8.4".into()), "front controller via PHP-FPM");
        assert_eq!(get("toko.test", "/some/route?x=1").await.1, "php:toko.test@8.4", "pretty URLs reach index.php");
        assert!(get("localhost", "/").await.1.contains("It works!"), "localhost still serves www/");

        projects::create(&layout, "blog").unwrap();
        commands::apply_vhosts(&st).await.unwrap();
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(get("blog.test", "/").await.1.contains("<h1>blog.test</h1>"), "new project served after restart");

        // Per-project PHP: toko on 8.3 while 8.4 keeps serving the others.
        let php83 = install_version(&st, "php", Some("8.3.32")).await;
        st.update_settings(|s| {
            s.project_php.insert("toko".into(), "8.3".into());
            Ok(())
        })
        .unwrap();
        commands::apply_vhosts(&st).await.unwrap();
        let port_of = |id: &str| {
            let (n, v) = paths::parse_service_id(id).unwrap();
            st.port_for(id, modules::load_installed(&layout, &n, &v).unwrap().service.as_ref().unwrap()).unwrap()
        };
        assert_ne!(port_of(&php83), port_of(&php), "8.3 moved to its own port");
        commands::start_service_inner(&st, &php83).await.expect("start php 8.3");
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(get("toko.test", "/").await.1, "php:toko.test@8.3");

        // HTTPS: the local CA signs the projects' certificate; nginx adds 8443.
        let ca = tls::ensure_ca(&layout).unwrap();
        commands::apply_vhosts(&st).await.unwrap();
        tokio::time::sleep(Duration::from_millis(800)).await;
        let root = reqwest::Certificate::from_pem(&std::fs::read(&ca).unwrap()).unwrap();
        let https = reqwest::Client::builder()
            .tls_built_in_root_certs(false)
            .add_root_certificate(root)
            .resolve("toko.test", "127.0.0.1:8443".parse().unwrap())
            .build()
            .unwrap();
        let body = https.get("https://toko.test:8443/").send().await.expect("TLS verified by the CA").text().await.unwrap();
        assert_eq!(body, "php:toko.test:8443@8.3on", "HTTPS reaches PHP with HTTPS=on");
        assert_eq!(get("toko.test", "/").await.1, "php:toko.test@8.3", "plain HTTP still served");

        st.supervisor.stop_all().await;
    }

    /// Node.js and Python: the default version's tools run through `<data>/bin`.
    #[tokio::test]
    #[ignore]
    async fn runtimes_on_path() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        st.layout().ensure().unwrap();
        let bin = st.layout().shim_dir();
        // Shims are found through PATH, like in a user's terminal (`npm` runs `node`).
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let run = |cmd: &str, arg: &str| {
            let out = std::process::Command::new(bin.join(cmd)).arg(arg).env("PATH", &path).output().unwrap();
            assert!(out.status.success(), "{cmd} {arg}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        for name in ["node", "python"] {
            let id = install(&st, name).await;
            let (n, v) = paths::parse_service_id(&id).unwrap();
            let m = modules::load_installed(&st.layout(), &n, &v).unwrap();
            runtimes::activate(&st.layout(), &m).unwrap();
        }
        assert!(run("node", "--version").starts_with('v'));
        assert!(!run("npm", "--version").is_empty());
        assert!(!run("npx", "--version").is_empty());
        assert!(run("python", "--version").contains("Python 3"));
        assert!(run("pip", "--version").contains("pip"));
    }

    /// Composer runs through its `<data>/bin` shim with the PHP CLI it ships.
    #[tokio::test]
    #[ignore]
    async fn composer_on_path() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        st.layout().ensure().unwrap();
        let id = install(&st, "composer").await;
        let (n, v) = paths::parse_service_id(&id).unwrap();
        runtimes::activate(&st.layout(), &modules::load_installed(&st.layout(), &n, &v).unwrap()).unwrap();
        let out = std::process::Command::new(st.layout().shim_dir().join(if cfg!(windows) { "composer.cmd" } else { "composer" }))
            .args(["--version", "--no-interaction"])
            .env("PATH", "/usr/bin:/bin") // no system PHP needed
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && stdout.contains("Composer version"), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    }

    /// MariaDB (generated root password), MongoDB, Redis and Apache.
    #[tokio::test]
    #[ignore]
    async fn more_services() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        st.layout().ensure().unwrap();
        let layout = st.layout();
        let ports = [("mariadb", 53307), ("mongodb", 57017), ("redis", 56379), ("apache", 58081)];
        let mut ids = Vec::new();
        for (name, port) in ports {
            let id = install(&st, name).await;
            st.update_settings(|s| s.set_service(&id, settings::ServiceSettings { port: Some(port), autostart: false }))
                .unwrap();
            let (n, v) = paths::parse_service_id(&id).unwrap();
            let m = modules::load_installed(&layout, &n, &v).unwrap();
            modules::write_configs(&layout, &m, Some(port), true).unwrap();
            commands::start_service_inner(&st, &id).await.expect("init + start");
            ids.push(id);
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        for f in logs::list(&layout.logs()) {
            let c = logs::read(&layout.logs(), &f.name, None).unwrap().content;
            let tail: Vec<&str> = c.lines().rev().take(8).collect();
            eprintln!("--- {} ---\n{}", f.name, tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
        }
        let run = |id: &str, bin: &str, args: &[&str], env: &[(&str, &str)]| {
            let (n, v) = paths::parse_service_id(id).unwrap();
            let out = std::process::Command::new(layout.module_dir(&n, &v).join(bin)).args(args).envs(env.iter().copied()).output().unwrap();
            (out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string())
        };

        let (n, v) = paths::parse_service_id(&ids[0]).unwrap();
        let pw = secrets::load(&layout, &n, &v).unwrap().0;
        let maria = |password: &str| {
            run(&ids[0], "bin/mariadb", &["--no-defaults", "-h127.0.0.1", "-P53307", "-uroot", "-Nse", "SELECT 41+1"], &[("MYSQL_PWD", password)])
        };
        assert_eq!(maria(&pw), (true, "42".into()), "mariadb login with generated password");
        assert!(!maria("wrong").0, "mariadb rejects wrong password");
        assert!(!maria("").0, "mariadb root has a password");

        let mongo = run(&ids[1], "mongosh/bin/mongosh", &["--quiet", "mongodb://127.0.0.1:57017/test", "--eval", "db.runCommand({ping:1}).ok"], &[]);
        assert_eq!(mongo, (true, "1".into()), "mongod answers");

        assert_eq!(run(&ids[2], "bin/redis-cli", &["-p", "56379", "PING"], &[]), (true, "PONG".into()));

        std::fs::write(layout.www().join("cheesenv-e2e.html"), "served by apache").unwrap();
        let body = reqwest::get("http://127.0.0.1:58081/cheesenv-e2e.html").await.unwrap().text().await.unwrap();
        assert_eq!(body, "served by apache");
        let denied = reqwest::get("http://127.0.0.1:58081/.htaccess").await.unwrap().status();
        assert!(denied == 403 || denied == 404, ".ht* files are not served");

        st.supervisor.stop_all().await;
        for (_, port) in ports {
            assert!(ports::is_free(port), "port {port} released");
        }
    }

    /// Installing a database also installs its web panel, which then lists it.
    #[tokio::test]
    #[ignore]
    async fn database_brings_its_panel() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        st.layout().ensure().unwrap();
        let layout = st.layout();
        let idx = st.registry(&layout).index(true).await.unwrap();
        let latest = |n: &str| idx.index.modules.iter().find(|m| m.name == n).unwrap().latest.clone();
        let (maria, pma) = (latest("mariadb"), latest("phpmyadmin"));

        let seen = std::sync::Mutex::new(std::collections::BTreeSet::new());
        let progress = |name: &str, _: &str, stage: &str, _: u64, _: Option<u64>| {
            if stage == "done" {
                seen.lock().unwrap().insert(name.to_string());
            }
        };
        let outcome = commands::install_with_companions(&st, "mariadb", &maria, &progress).await.unwrap();
        eprintln!("{outcome:?}");
        assert!(modules::load_installed(&layout, "phpmyadmin", &pma).is_ok(), "phpMyAdmin installed along");
        assert_eq!(seen.lock().unwrap().iter().cloned().collect::<Vec<_>>(), ["mariadb", "phpmyadmin"]);

        // Already installed: a second database install does not reinstall it.
        modules::uninstall(&layout, "mariadb", &maria, false).unwrap();
        let again = commands::install_with_companions(&st, "mariadb", &maria, &|_, _, _, _, _| {}).await.unwrap();
        assert!(format!("{again:?}").contains("companions: []"), "{again:?}");

        let (db, panel) = (service_id("mariadb", &maria), service_id("phpmyadmin", &pma));
        st.update_settings(|s| {
            s.set_service(&db, settings::ServiceSettings { port: Some(53308), autostart: false })?;
            // The panel's port stays editable like any service.
            s.set_service(&panel, settings::ServiceSettings { port: Some(58089), autostart: false })
        })
        .unwrap();
        let m = modules::load_installed(&layout, "mariadb", &maria).unwrap();
        modules::write_configs(&layout, &m, Some(53308), true).unwrap();
        commands::start_service_inner(&st, &db).await.expect("mariadb");
        commands::start_service_inner(&st, &panel).await.expect("phpmyadmin");
        tokio::time::sleep(Duration::from_secs(3)).await;

        let raw = std::fs::read_to_string(layout.run_dir("phpmyadmin", &pma).join("panel.json")).unwrap();
        let file: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(file["port"], 58089);
        assert_eq!(file["targets"][0]["name"], "mariadb");
        assert_eq!(file["targets"][0]["port"], 53308);
        let page = reqwest::get("http://127.0.0.1:58089/").await.unwrap();
        assert_eq!(page.status(), 200);
        assert!(page.text().await.unwrap().contains("phpMyAdmin"));

        // Stopping its only running database stops the panel too.
        commands::stop_service_inner(&st, &db).await.unwrap();
        assert!(!st.supervisor.is_active(&panel).await, "panel stopped with its database");
        st.supervisor.stop_all().await;
    }

    /// Updating a panel replaces the old version, keeping its port and data.
    /// Needs a registry listing at least two phpMyAdmin versions.
    #[tokio::test]
    #[ignore]
    async fn panel_update_keeps_port_and_data() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        st.layout().ensure().unwrap();
        let layout = st.layout();
        let idx = st.registry(&layout).index(true).await.unwrap();
        let entry = idx.index.modules.iter().find(|m| m.name == "phpmyadmin").unwrap().clone();
        let manifest = st.registry(&layout).manifest(&entry).await.unwrap();
        let mut versions: Vec<_> = manifest.versions.iter().map(|v| v.version.clone()).collect();
        versions.sort_by(|a, b| registry::compare_versions(a, b));
        let (old, new) = (versions[0].clone(), versions.last().unwrap().clone());
        assert_ne!(old, new, "registry needs two phpMyAdmin versions");

        let none = |_: &str, _: &str, _: &str, _: u64, _: Option<u64>| {};
        commands::install_with_companions(&st, "phpmyadmin", &old, &none).await.unwrap();
        let (old_id, new_id) = (service_id("phpmyadmin", &old), service_id("phpmyadmin", &new));
        let kept = settings::ServiceSettings { port: Some(58090), autostart: true };
        st.update_settings(|s| s.set_service(&old_id, kept.clone())).unwrap();
        std::fs::create_dir_all(layout.data_dir("phpmyadmin", &old)).unwrap();
        std::fs::write(layout.data_dir("phpmyadmin", &old).join("marker"), "x").unwrap();
        commands::start_service_inner(&st, &old_id).await.unwrap();

        // Not a panel: refused.
        assert!(commands::update_panel(&st, "mariadb", &none).await.is_err());

        assert_eq!(commands::update_panel(&st, "phpmyadmin", &none).await.unwrap(), new);
        assert!(modules::load_installed(&layout, "phpmyadmin", &old).is_err(), "old version removed");
        assert!(modules::load_installed(&layout, "phpmyadmin", &new).is_ok());
        assert!(!st.supervisor.is_active(&old_id).await, "old panel stopped");
        let s = st.settings();
        assert_eq!(s.services.get(&new_id), Some(&kept));
        assert!(!s.services.contains_key(&old_id));
        assert!(layout.data_dir("phpmyadmin", &new).join("marker").exists(), "data moved");

        // Already current: nothing to do.
        assert_eq!(commands::update_panel(&st, "phpmyadmin", &none).await.unwrap(), new);
        st.supervisor.stop_all().await;
    }

    /// A fresh app installs the built-in panels itself; a second start changes nothing.
    #[tokio::test]
    #[ignore]
    async fn panels_are_built_in() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        st.layout().ensure().unwrap();
        let layout = st.layout();
        let none = |_: &str, _: &str, _: &str, _: u64, _: Option<u64>| {};
        assert_eq!(commands::ensure_panels(&st, &none).await, Vec::<String>::new());
        let installed = |l: &paths::Layout| {
            let mut v: Vec<_> = modules::list_installed(l).into_iter().map(|m| format!("{}@{}", m.name, m.version)).collect();
            v.sort();
            v
        };
        let first = installed(&layout);
        assert_eq!(first.len(), 2, "{first:?}");
        assert!(first[0].starts_with("pgadmin@") && first[1].starts_with("phpmyadmin@"), "{first:?}");
        assert_eq!(commands::ensure_panels(&st, &none).await, Vec::<String>::new());
        assert_eq!(installed(&layout), first);
    }

    fn client(st: &AppState, id: &str, bin: &str) -> std::path::PathBuf {
        let (n, v) = paths::parse_service_id(id).unwrap();
        st.layout().module_dir(&n, &v).join("bin").join(bin)
    }

    #[tokio::test]
    #[ignore]
    async fn databases_init_auth_restart() {
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
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
        if std::env::var_os("CHEESENV_E2E_REGISTRY").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let st = Arc::new(state(tmp.path()));
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
        let (code, body) = get(panel_url.clone()).await;
        assert!(code == 200 || code == 302, "pgAdmin accepts its key: {code} {body}");

        // The web UI itself loads: its JS bundles are part of the package
        // (the pgadmin4 9.19 wheel shipped without them: a blank page).
        let res = http.get(&panel_url).send().await.unwrap();
        let cookies: Vec<String> = res
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|c| c.to_str().ok()?.split(';').next().map(String::from))
            .collect();
        let cookie = cookies.join("; ");
        let page = http.get("http://127.0.0.1:55050/browser/").header("cookie", &cookie).send().await.unwrap();
        assert_eq!(page.status(), 200);
        let page = page.text().await.unwrap();
        let bundle = page
            .split('"')
            .find(|s| s.starts_with("/static/js/generated/") && s.contains(".js"))
            .unwrap_or_else(|| panic!("no bundle referenced: {page}"));
        let js = http.get(format!("http://127.0.0.1:55050{bundle}")).header("cookie", &cookie).send().await.unwrap();
        assert_eq!(js.status(), 200, "{bundle} is served");
    }
}
