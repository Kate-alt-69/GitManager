//! Standalone headless GitManager data and background service.
//!
//! This crate intentionally has no Tauri or WebView dependency. It hosts
//! repository discovery, persistent indexing, archive watchers, remote checks
//! and notification delivery as a separate per-user process.

pub use gitmanager_core::{
    git_info, notification_center, scanner, AppPreferences, RepositoryPreferences,
    load_background_preferences,
};

pub mod archive {
    pub use gitmanager_archive::*;
}

#[path = "../../../src-tauri/src/process_utils.rs"]
pub mod process_utils;
#[path = "../../../src-tauri/src/archive_watcher.rs"]
pub mod archive_watcher;
#[path = "../../../src-tauri/src/background_service.rs"]
pub mod background_service;

pub mod watcher_service;

pub fn run() -> ! {
    background_service::run()
}
