use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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

fn default_archive_root_string() -> String {
    dirs::data_local_dir()
        .map(|root| root
            .join("kastrick's_software")
            .join("data")
            .join("GitManager")
            .join("archives")
            .to_string_lossy()
            .to_string())
        .unwrap_or_default()
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            scan_roots: crate::repository_index::default_scan_roots()
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            max_scan_depth: 8,
            include_hidden_dirs: false,
            index_dependency_repositories: true,
            show_dependency_repositories: true,
            repository_settings: HashMap::new(),
            first_run_complete: false,
            archive_root: default_archive_root_string(),
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

fn preferences_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Reads per-user preferences without bringing Tauri or a WebView into the
/// service process. The desktop performs additional provider URL validation.
pub fn load_background_preferences() -> Result<AppPreferences, String> {
    let _guard = preferences_lock().lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    crate::storage::ensure_data_layout()?;
    let config_dir = crate::storage::config_dir()?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Could not create GitManager config directory: {error}"))?;
    let path = config_dir.join("preferences.json");
    if !path.is_file() {
        // Recover either backup naming scheme from previous preference writers.
        for extension in ["json.previous", "json.bak"] {
            let backup = path.with_extension(extension);
            if backup.is_file() {
                fs::rename(&backup, &path)
                    .map_err(|error| format!("Could not recover previous GitManager preferences: {error}"))?;
                break;
            }
        }
    }
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
        preferences.archive_root = default_archive_root_string();
    }
    preferences.archive_trigger = preferences.archive_trigger.trim().to_ascii_lowercase();
    if !matches!(preferences.archive_trigger.as_str(), "manual" | "commit" | "change") {
        preferences.archive_trigger = "manual".to_string();
    }
    preferences.archive_quiet_ms = preferences.archive_quiet_ms.clamp(250, 60_000);
    Ok(preferences)
}


/// Persists the validated application preferences from the required service.
/// The desktop may normalize user input, but it never writes this file itself.
pub fn save_background_preferences(preferences: &mut AppPreferences) -> Result<(), String> {
    if !(2..=16).contains(&preferences.max_scan_depth) {
        return Err("Scan depth must be between 2 and 16.".to_string());
    }
    if preferences.scan_roots.len() > 32 {
        return Err("GitManager supports up to 32 scan roots.".to_string());
    }
    if !matches!(preferences.archive_trigger.as_str(), "manual" | "commit" | "change") {
        return Err("Archive trigger must be manual, commit, or change.".to_string());
    }
    if !(250..=60_000).contains(&preferences.archive_quiet_ms) {
        return Err("Archive quiet period must be between 250 and 60000 milliseconds.".to_string());
    }
    if preferences.archive_root.len() > 4096 || preferences.cloud_node_address.len() > 2048 {
        return Err("One of the saved application paths or addresses is too long.".to_string());
    }

    let archive_root = if preferences.archive_root.trim().is_empty() {
        default_archive_root_string()
    } else {
        preferences.archive_root.trim().to_string()
    };
    if archive_root.trim().is_empty() {
        return Err("Could not resolve the default GitManager archive directory.".to_string());
    }
    let archive_path = PathBuf::from(&archive_root);
    let archive_path = if archive_path.is_absolute() {
        archive_path
    } else {
        std::env::current_dir()
            .map_err(|error| format!("Could not resolve the current directory: {error}"))?
            .join(archive_path)
    };
    fs::create_dir_all(&archive_path)
        .map_err(|error| format!("Could not create archive storage directory: {error}"))?;
    if !archive_path.is_dir() {
        return Err("Archive storage path must be a directory.".to_string());
    }
    preferences.archive_root = fs::canonicalize(&archive_path)
        .map_err(|error| format!("Could not resolve archive storage directory: {error}"))?
        .to_string_lossy()
        .to_string();

    let _guard = preferences_lock().lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    crate::storage::ensure_data_layout()?;
    let config_dir = crate::storage::config_dir()?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Could not create GitManager config directory: {error}"))?;
    let path = config_dir.join("preferences.json");
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.previous");
    let bytes = serde_json::to_vec_pretty(preferences)
        .map_err(|error| format!("Could not serialize GitManager preferences: {error}"))?;
    if bytes.len() > 256 * 1024 {
        return Err("GitManager preferences exceed the 256 KiB safety limit.".to_string());
    }

    {
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Could not create temporary GitManager preferences: {error}"))?;
        output.write_all(&bytes)
            .map_err(|error| format!("Could not write temporary GitManager preferences: {error}"))?;
        output.sync_all()
            .map_err(|error| format!("Could not flush temporary GitManager preferences: {error}"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not restrict GitManager preference permissions: {error}"))?;
    }

    let had_previous = path.is_file();
    if had_previous {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Could not clear previous preferences backup: {error}"))?;
        }
        fs::rename(&path, &backup)
            .map_err(|error| format!("Could not stage previous GitManager preferences: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if had_previous {
            let _ = fs::rename(&backup, &path);
        }
        return Err(format!("Could not publish GitManager preferences: {error}"));
    }
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    Ok(())
}
