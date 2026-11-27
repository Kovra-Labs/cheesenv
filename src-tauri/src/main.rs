// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The elevated hosts helper exits before any window or tray is created.
    if let Some(code) = cheesenv_lib::run_helper() {
        std::process::exit(code);
    }
    cheesenv_lib::run()
}
