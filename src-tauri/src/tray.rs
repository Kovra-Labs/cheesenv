use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use crate::commands::start_all_inner;
use crate::state::AppState;

const TRAY_ID: &str = "main";

fn labels(lang: &str) -> [&'static str; 4] {
    match lang {
        "en" => ["Open CheesEnv", "Start all services", "Stop all services", "Quit"],
        _ => ["Buka CheesEnv", "Jalankan semua service", "Hentikan semua service", "Keluar"],
    }
}

fn build_menu(app: &AppHandle, lang: &str) -> tauri::Result<Menu<Wry>> {
    let [show, start, stop, quit] = labels(lang);
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", show, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "start_all", start, true, None::<&str>)?,
            &MenuItem::with_id(app, "stop_all", stop, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", quit, true, None::<&str>)?,
        ],
    )
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn create(app: &AppHandle, lang: &str) -> tauri::Result<()> {
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("CheesEnv")
        .menu(&build_menu(app, lang)?)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "start_all" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    for f in start_all_inner(&state).await {
                        eprintln!("[cheesenv] tray start {} failed: {}", f.id, f.error);
                    }
                });
            }
            "stop_all" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    app.state::<AppState>().supervisor.stop_all().await;
                });
            }
            // Exit is intercepted in lib.rs so services are stopped first.
            "quit" => app.exit(0),
            _ => {}
        });
    // Monochrome glyph; as a template image macOS recolors it for light/dark menu bars.
    builder
        .icon(tauri::include_image!("icons/tray.png"))
        .icon_as_template(true)
        .build(app)?;
    Ok(())
}

pub fn refresh(app: &AppHandle, lang: &str) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app, lang)) {
        let _ = tray.set_menu(Some(menu));
    }
}
