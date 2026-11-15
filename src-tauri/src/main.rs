// Prevents additional console window on Windows 10 and 11 in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // The installed GitManager executable is also the per-user background
    // worker. It uses the same preferences and watcher lease as the desktop.
    if args.iter().any(|argument| argument == "--background-service") {
        git_manager_lib::background_service::run();
    }

    if args.iter().any(|argument| argument == "--setup") {
        std::env::set_var("GITMANAGER_SETUP_MODE", "1");
    }

    // Tauri remains the actual UI/runtime for normal and first-run modes.
    git_manager_lib::run();
}
