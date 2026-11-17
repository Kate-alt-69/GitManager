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

pub mod process_utils;
pub mod archive_watcher;
pub mod background_service;

pub mod watcher_service;

pub fn run() -> ! {
    background_service::run()
}
