// Standalone Background Service / Daemon Binary (`git-manager-service`).
// The worker checks Git remotes and owns automatic repository archive watching
// whenever the desktop process is not already watching the same user profile.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use git_manager_lib::archive_watcher;
use git_manager_lib::watcher_service::{
    apply_repository_update, check_all_repos_for_updates, send_os_desktop_notification,
};
use git_manager_lib::load_background_preferences;
use std::collections::HashSet;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

const PREFERENCE_REFRESH: Duration = Duration::from_secs(30);
const REMOTE_UPDATE_INTERVAL: Duration = Duration::from_secs(120);

fn main() {
    let mut notified_commits: HashSet<String> = HashSet::new();
    let mut last_remote_check = Instant::now() - REMOTE_UPDATE_INTERVAL;

    loop {
        match load_background_preferences() {
            Ok(preferences) => {
                // Archive watcher uses a per-user cross-process lease. If the
                // desktop process currently owns it, this call is a no-op and
                // the service retries on the next preference refresh.
                let _ = archive_watcher::restart_archive_watchers(&preferences);

                if last_remote_check.elapsed() >= REMOTE_UPDATE_INTERVAL {
                    let updates = check_all_repos_for_updates(preferences.scan_roots.clone());

                    for update in updates {
                        let auto_update_allowed = preferences
                            .repository_settings
                            .get(&update.repo_id)
                            .map(|settings| settings.auto_update)
                            .unwrap_or(false)
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
