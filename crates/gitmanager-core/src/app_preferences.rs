use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryPreferences {
    pub auto_update: bool,
    pub keep_remote_clean: bool,
    pub commit_clean: bool,
    pub cleaner_include_dependencies: bool,
    pub cleaner_include_virtualenvs: bool,
    pub cleaner_exclude_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppPreferences {
    pub scan_roots: Vec<String>,
    pub max_scan_depth: usize,
    pub include_hidden_dirs: bool,
    pub index_dependency_repositories: bool,
    pub show_dependency_repositories: bool,
    pub repository_settings: HashMap<String, RepositoryPreferences>,
    pub first_run_complete: bool,
    pub archive_root: String,
    pub archive_local_enabled: bool,
    pub archive_remote_enabled: bool,
    pub archive_trigger: String,
    pub archive_quiet_ms: u64,
    pub cloud_node_address: String,
    pub in_app_notifications_enabled: bool,
    pub os_notifications_enabled: bool,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            scan_roots: crate::scanner::default_scan_roots()
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            max_scan_depth: 8,
            include_hidden_dirs: false,
            index_dependency_repositories: true,
            show_dependency_repositories: true,
            repository_settings: HashMap::new(),
            first_run_complete: false,
            archive_root: gitmanager_archive::default_archive_root()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default(),
            archive_local_enabled: true,
            archive_remote_enabled: true,
            archive_trigger: "manual".to_string(),
            archive_quiet_ms: 1500,
            cloud_node_address: String::new(),
            in_app_notifications_enabled: true,
            os_notifications_enabled: true,
        }
    }
}

/// Reads per-user preferences without bringing Tauri or a WebView into the
/// service process. The desktop performs additional provider URL validation.
pub fn load_background_preferences() -> Result<AppPreferences, String> {
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the current user's local application-data directory.".to_string())?;
    let config_dir = local_data
        .join("kastrick's_software")
        .join("config")
        .join("GitManager");
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Could not create GitManager config directory: {error}"))?;
    let path = config_dir.join("preferences.json");
    if !path.is_file() {
        return Ok(AppPreferences::default());
    }
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("Could not inspect GitManager preferences: {error}"))?;
    if metadata.len() > 256 * 1024 {
        return Err("GitManager preferences file is unexpectedly large.".to_string());
    }
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read GitManager preferences: {error}"))?;
    let mut preferences: AppPreferences = serde_json::from_str(&content)
        .map_err(|error| format!("GitManager preferences are invalid JSON: {error}"))?;

    let mut retained_roots = Vec::new();
    for raw_root in preferences.scan_roots.drain(..).take(32) {
        let root = raw_root.trim();
        if root.is_empty() {
            continue;
        }
        let value = fs::canonicalize(root)
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|_| root.to_string());
        if !retained_roots.iter().any(|existing: &String| existing.eq_ignore_ascii_case(&value)) {
            retained_roots.push(value);
        }
    }
    preferences.scan_roots = retained_roots;
    preferences.max_scan_depth = preferences.max_scan_depth.clamp(2, 16);
    if preferences.archive_root.trim().is_empty() {
        preferences.archive_root = gitmanager_archive::default_archive_root()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
    }
    preferences.archive_trigger = preferences.archive_trigger.trim().to_ascii_lowercase();
    if !matches!(preferences.archive_trigger.as_str(), "manual" | "commit" | "change") {
        preferences.archive_trigger = "manual".to_string();
    }
    preferences.archive_quiet_ms = preferences.archive_quiet_ms.clamp(250, 60_000);
    Ok(preferences)
}
