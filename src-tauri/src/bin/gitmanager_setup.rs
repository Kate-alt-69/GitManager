#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Cross-platform first-run companion for GitManager.
//!
//! This binary shares the main Tauri UI and opens it in forced setup mode.
//! The normal application launches this sidecar only when first-run setup is
//! incomplete. Once setup is saved, the helper continues as the regular app.

fn main() {
    std::env::set_var("GITMANAGER_SETUP_MODE", "1");
    git_manager_lib::run();
}
