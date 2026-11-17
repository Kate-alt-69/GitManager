//! Shared GitManager repository discovery, preferences, metadata and notification storage.
//!
//! This crate is Tauri-free. The desktop UI and required headless data service
//! share its repository index, user preferences and notification store.

pub mod app_preferences;
pub mod git_info;
pub mod scanner;
pub mod notification_center;

pub use app_preferences::{load_background_preferences, AppPreferences, RepositoryPreferences};
