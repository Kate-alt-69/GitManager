//! Headless background worker hosted by the standalone git-manager-service
//! workspace package. The OS service manager launches that separate executable
//! directly; it is not a child of the desktop UI or crash_helper.

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
use crate::scanner::{
    load_persisted_repository_index, save_persisted_repository_index,
    scan_repositories_with_report, ScanOptions,
};
use crate::notification_center::publish_repository_update;

use crash_helper::start_service_control_server;

const PREFERENCE_REFRESH: Duration = Duration::from_secs(30);
const REMOTE_UPDATE_INTERVAL: Duration = Duration::from_secs(120);
const CLOUD_ARCHIVE_SYNC_INTERVAL: Duration = Duration::from_secs(300);

fn retry_cloud_archive_sync(archive_root: &str) {
    match archive::archive_cloud_storage_info(Path::new(archive_root)) {
        Ok(info) if info.configured => {
            match archive::sync_archive_cloud_storage(Some(archive_root)) {
                Ok(action) if action != "None" => {
                    eprintln!("GitManager core service: cloud archive sync {action}.");
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
    let service_control = match start_service_control_server() {
        Ok(server) => server,
        Err(error) => {
            eprintln!("GitManager background service could not start its control endpoint: {error}");
            std::process::exit(2);
        }
    };
    let mut was_suspended = false;
    let mut notified_commits: HashSet<String> = HashSet::new();
    let mut last_scan_settings = String::new();
    // Keep the first remote update discovery out of application/service startup.
    let mut last_remote_check = Instant::now();
    let mut last_cloud_archive_sync = Instant::now() - CLOUD_ARCHIVE_SYNC_INTERVAL;

    loop {
        if service_control.is_suspended() {
            if !was_suspended {
                archive_watcher::stop_archive_watchers();
                eprintln!("GitManager background service: suspended by crash_helper; waiting for resume or timeout.");
                was_suspended = true;
            }
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        if was_suspended {
            // Avoid a burst of remote checks or cloud writes on recovery.
            last_remote_check = Instant::now();
            last_cloud_archive_sync = Instant::now();
            eprintln!("GitManager background service: suspension ended; resuming.");
            was_suspended = false;
            thread::sleep(Duration::from_millis(300));
        }

        match load_background_preferences() {
            Ok(preferences) => {
                let _ = archive_watcher::restart_archive_watchers(&preferences);

                let scan_settings = serde_json::to_string(&(
                    &preferences.scan_roots,
                    preferences.max_scan_depth,
                    preferences.include_hidden_dirs,
                    preferences.index_dependency_repositories,
                )).unwrap_or_default();
                let index_missing = load_persisted_repository_index()
                    .map(|index| index.is_none())
                    .unwrap_or(true);
                let scan_requested = service_control.take_scan_request();
                if scan_requested || index_missing || scan_settings != last_scan_settings {
                    eprintln!("GitManager core service: refreshing the repository index.");
                    let report = scan_repositories_with_report(ScanOptions {
                        root_paths: preferences.scan_roots.clone(),
                        max_depth: Some(preferences.max_scan_depth),
                        include_hidden_dirs: Some(preferences.include_hidden_dirs),
                        index_dependency_repositories: Some(preferences.index_dependency_repositories),
                    });
                    match report {
                        Ok(report) => match save_persisted_repository_index(&report) {
                            Ok(index) => {
                                eprintln!(
                                    "GitManager core service: indexed {} repositories{}.",
                                    index.repositories.len(),
                                    if index.truncated { " (safety limit reached)" } else { "" },
                                );
                                last_scan_settings = scan_settings;
                            }
                            Err(error) => {
                                eprintln!("GitManager core service: could not save repository index: {error}");
                            }
                        },
                        Err(error) => {
                            eprintln!("GitManager core service: repository indexing failed: {error}");
                        }
                    }
                    // Requests received while this scan was running are satisfied
                    // by the just-published index; avoid an immediate duplicate crawl.
                    let _ = service_control.take_scan_request();
                }

                if last_cloud_archive_sync.elapsed() >= CLOUD_ARCHIVE_SYNC_INTERVAL {
                    if !service_control.is_suspended() {
                        retry_cloud_archive_sync(&preferences.archive_root);
                    }
                    last_cloud_archive_sync = Instant::now();
                }

                if last_remote_check.elapsed() >= REMOTE_UPDATE_INTERVAL {
                    let updates = if service_control.is_suspended() {
                        Vec::new()
                    } else {
                        check_all_repos_for_updates(preferences.scan_roots.clone())
                    };

                    for update in updates {
                        if service_control.is_suspended() {
                            break;
                        }
                        let auto_update_allowed = repository_auto_update_enabled(&update.repo_path)
                            && update.working_tree_clean
                            && update.commits_ahead == 0;

                        if service_control.is_suspended() {
                            break;
                        }
                        if auto_update_allowed
                            && apply_repository_update(Path::new(&update.repo_path)).is_ok()
                        {
                            continue;
                        }

                        let key = format!("{}::{}", update.repo_path, update.remote_oid);
                        if notified_commits.insert(key) {
                            let created = publish_repository_update(
                                &update.repo_id,
                                &update.repo_name,
                                &update.repo_path,
                                &update.remote_oid,
                                &update.prompt_message,
                            ).map(|(_, created)| created).unwrap_or(true);
                            if preferences.os_notifications_enabled && created {
                                send_os_desktop_notification(&update);
                            }
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

        let refresh_deadline = Instant::now() + PREFERENCE_REFRESH;
        while Instant::now() < refresh_deadline && !service_control.is_suspended() {
            thread::sleep(Duration::from_secs(1));
        }
    }
}
