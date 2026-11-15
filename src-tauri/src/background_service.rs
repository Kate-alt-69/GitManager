//! Headless background worker shared by the desktop executable and the
//! optional standalone service binary. The OS service launches the installed
//! GitManager executable with --background-service, avoiding an extra runtime
//! binary that would otherwise need to be copied into each native installer.

use std::collections::HashSet;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::archive;
use crate::archive_watcher;
use crate::load_background_preferences;
use crate::watcher_service::{
    apply_repository_update, check_all_repos_for_updates, send_os_desktop_notification,
};

const PREFERENCE_REFRESH: Duration = Duration::from_secs(30);
const REMOTE_UPDATE_INTERVAL: Duration = Duration::from_secs(120);
const CLOUD_ARCHIVE_SYNC_INTERVAL: Duration = Duration::from_secs(300);

fn retry_cloud_archive_sync(archive_root: &str) {
    match archive::archive_cloud_storage_info(Path::new(archive_root)) {
        Ok(info) if info.configured => {
            match archive::sync_archive_cloud_storage(Some(archive_root)) {
                Ok(action) if action != "None" => {
                    eprintln!("GitManager background service: cloud archive sync {action}.");
                }
                Ok(_) => {}
                Err(error) if error.contains("Another GitManager archive operation is already running") => {
                    // A capture/configuration sync owns the shared provider lock.
                    // The next five-minute cycle retries after that operation.
                }
                Err(error) => {
                    eprintln!("GitManager background service: cloud archive sync failed: {error}");
                }
            }
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("GitManager background service: cloud archive configuration check failed: {error}");
        }
    }
}

fn repository_auto_update_enabled(repo_path: &str) -> bool {
    let repo = match git2::Repository::open(repo_path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let settings_file = repo.path().join("info").join("gitmanager-settings.json");
    let content = match std::fs::read_to_string(settings_file) {
        Ok(value) if value.len() <= 64 * 1024 => value,
        _ => return false,
    };
    serde_json::from_str::<serde_json::Value>(&content)
        .ok()
        .and_then(|value| value.get("autoUpdate").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// Runs until the operating system stops the background process.
///
/// A cross-process lease in archive_watcher makes it safe for the desktop
/// process and this worker to use the same watcher implementation: one owns
/// filesystem notifications at a time, and the other retries periodically.
pub fn run() -> ! {
    let mut notified_commits: HashSet<String> = HashSet::new();
    let mut last_remote_check = Instant::now() - REMOTE_UPDATE_INTERVAL;
    let mut last_cloud_archive_sync = Instant::now() - CLOUD_ARCHIVE_SYNC_INTERVAL;

    loop {
        match load_background_preferences() {
            Ok(preferences) => {
                let _ = archive_watcher::restart_archive_watchers(&preferences);

                if last_cloud_archive_sync.elapsed() >= CLOUD_ARCHIVE_SYNC_INTERVAL {
                    retry_cloud_archive_sync(&preferences.archive_root);
                    last_cloud_archive_sync = Instant::now();
                }

                if last_remote_check.elapsed() >= REMOTE_UPDATE_INTERVAL {
                    let updates = check_all_repos_for_updates(preferences.scan_roots.clone());

                    for update in updates {
                        let auto_update_allowed = repository_auto_update_enabled(&update.repo_path)
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
                    last_remote_check = Instant::now();
                }
            }
            Err(error) => {
                // Do not discard an already-running watcher on a transient
                // preference read/parse failure. A later iteration retries.
                eprintln!("GitManager background service: could not load preferences: {error}");
            }
        }

        thread::sleep(PREFERENCE_REFRESH);
    }
}
