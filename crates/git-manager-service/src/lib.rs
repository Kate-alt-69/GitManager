//! Standalone headless GitManager data and background service.
//!
//! This crate intentionally has no Tauri or WebView dependency. It hosts
//! repository discovery, persistent indexing, archive watchers, remote checks,
//! and notification delivery as a separate background process that supports
//! both per-user execution and native Windows SCM service hosting.

pub use gitmanager_core::{
	git_info, notification_center, repository_index, scanner, AppPreferences,
	RepositoryPreferences, load_background_preferences,
};

pub mod archive {
	pub use gitmanager_archive::*;
}

pub mod process_utils;
pub mod archive_watcher;
pub mod background_service;
pub mod watcher_service;
pub mod windows_scm;

/// Attempts to run under the Windows Service Control Manager when launched by
/// `services.msc` / `sc.exe`, returning `false` on normal user/supervisor launches.
pub fn try_run_as_windows_service() -> bool {
	windows_scm::try_run_as_windows_service()
}

/// Runs the standalone background service loop directly in the current session.
pub fn run() -> ! {
	background_service::run()
}
