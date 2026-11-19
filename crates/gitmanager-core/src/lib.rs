//! Shared GitManager repository discovery, preferences, metadata and notification storage.
//!
//! This crate is Tauri-free. The desktop UI and required headless data service
//! share its repository index, user preferences and notification store.

pub mod account_store;
pub mod account_types;
pub mod app_preferences;
pub mod models;
pub mod repository_index;
#[cfg(feature = "git-engine")]
pub mod git_info;
#[cfg(feature = "git-engine")]
pub mod repository_browser;
#[cfg(feature = "git-engine")]
pub mod repository_workspace;
#[cfg(feature = "git-engine")]
pub mod repository_preferences;
#[cfg(feature = "scan-engine")]
pub mod scanner;
pub mod storage;
pub mod notification_center;

pub use app_preferences::{load_background_preferences, AppPreferences, RepositoryPreferences};
