// Standalone Background Service / Daemon Binary (`git-manager-service`)
// Runs headlessly in the background (via Windows Service / svchost worker, systemd user service, or launchd)
// to poll local Git repositories for remote updates and display native OS notifications:
// "Hay you have <X-REPO_NAME> update do you want to update it?"

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use git_manager_lib::watcher_service::{
    check_all_repos_for_updates, send_os_desktop_notification,
};
use std::collections::HashSet;
use std::thread;
use std::time::Duration;

fn main() {
    let mut notified_commits: HashSet<String> = HashSet::new();

    loop {
        let updates = check_all_repos_for_updates(Vec::new());
        for upd in updates {
            let key = format!("{}::{}", upd.repo_path, upd.remote_oid);
            if notified_commits.insert(key) {
                send_os_desktop_notification(&upd);
            }
        }
        thread::sleep(Duration::from_secs(120));
    }
}
