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
    "set_runtime",
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
    "open_panel",
    "open_folder",
    "get_credentials",
    "sync_projects",
    "create_project",
    "sync_hosts",
    "open_project",
    "terminal_open",
    "terminal_write",
    "terminal_resize",
    "terminal_close",
    "check_update",
    "install_update",
];

fn main() {
    // Registry baked into the app (see registry::REGISTRY_URL); a typo should
    // fail the build, not every install. HTTP is only for a local registry.
    println!("cargo:rerun-if-env-changed=STACKYARD_REGISTRY_URL");
    if let Ok(url) = std::env::var("STACKYARD_REGISTRY_URL") {
        let local = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
            .iter()
            .any(|p| url.strip_prefix(p).is_some_and(|r| r.is_empty() || r.starts_with([':', '/'])));
        let https = url.strip_prefix("https://").is_some_and(|r| !r.is_empty());
        assert!(
            (https || local) && !url.chars().any(|c| c.is_whitespace() || c.is_control()),
            "STACKYARD_REGISTRY_URL must be an https:// URL (http only for localhost), got {url:?}"
        );
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
