// Prevents additional console window on Windows 10 and 11 in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // If launched by Windows Service / svchost worker flag (`--svchost-worker`), run the background update loop
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--svchost-worker") {
        loop {
            let updates = git_manager_lib::watcher_service::check_all_repos_for_updates(Vec::new());
            for upd in updates {
                git_manager_lib::watcher_service::send_os_desktop_notification(&upd);
            }
            std::thread::sleep(std::time::Duration::from_secs(120));
        }
    }

    git_manager_lib::run();
}
