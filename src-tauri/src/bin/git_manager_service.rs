// Standalone Background Service / Daemon Binary (`git-manager-service`)
// Reads the same user preferences as the desktop app, checks configured scan roots,
// and performs opt-in safe fast-forwards only for clean, non-diverged repositories.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use git_manager_lib::watcher_service::{
    apply_repository_update, check_all_repos_for_updates, send_os_desktop_notification,
};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

const APP_CONFIG_DIR: &str = "com.gitmanager.desktop";

fn load_user_preferences() -> (Vec<String>, HashSet<String>) {
    let config_root = match dirs::config_dir() {
        Some(path) => path,
        None => return (Vec::new(), HashSet::new()),
    };
    let file_path = config_root
        .join(APP_CONFIG_DIR)
        .join("preferences.json");
    let content = match fs::read_to_string(file_path) {
        Ok(value) if value.len() <= 256 * 1024 => value,
        _ => return (Vec::new(), HashSet::new()),
    };
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return (Vec::new(), HashSet::new()),
    };

    let roots = value
        .get("scanRoots")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let auto_update_ids = value
        .get("repositorySettings")
        .and_then(Value::as_object)
        .map(|settings| {
            settings
                .iter()
                .filter_map(|(repo_id, settings)| {
                    settings
                        .get("autoUpdate")
                        .and_then(Value::as_bool)
                        .filter(|enabled| *enabled)
                        .map(|_| repo_id.clone())
                })
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    (roots, auto_update_ids)
}

fn main() {
    let mut notified_commits: HashSet<String> = HashSet::new();

    loop {
        let (scan_roots, auto_update_ids) = load_user_preferences();
        let updates = check_all_repos_for_updates(scan_roots);

        for update in updates {
            let auto_update_allowed = auto_update_ids.contains(&update.repo_id)
                && update.working_tree_clean
                && update.commits_ahead == 0;

            if auto_update_allowed
                && apply_repository_update(Path::new(&update.repo_path)).is_ok()
            {
                continue;
            }

            let key = format!("{}::{}", update.repo_path, update.remote_oid);
            if notified_commits.insert(key) {
                send_os_desktop_notification(&update);
            }
        }
        thread::sleep(Duration::from_secs(120));
    }
}
