// Commands listed here get generated `allow-<command>` permissions; the
// capability file decides which the webview may call (strict IPC per PRD §8).
const COMMANDS: &[&str] = &[
    "get_app_info",
    "get_settings",
    "update_settings",
    "registry_index",
    "registry_module",
    "install_module",
    "uninstall_module",
    "list_services",
    "start_service",
    "stop_service",
    "restart_service",
    "start_all",
    "stop_all",
    "set_service_options",
    "check_port",
    "list_logs",
    "read_log",
    "clear_log",
    "open_service_url",
    "open_folder",
    "get_credentials",
    "check_update",
    "install_update",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
