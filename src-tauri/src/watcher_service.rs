use git2::{Direction, FetchOptions, Repository};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::scanner::{default_scan_roots, scan_repositories, ScanOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoUpdateNotification {
    pub repo_id: String,
    pub repo_name: String,
    pub repo_path: String,
    pub branch: String,
    pub local_oid: String,
    pub remote_oid: String,
    pub commits_behind: usize,
    pub prompt_message: String, // "Hay you have <X-REPO_NAME> update do you want to update it?"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatusInfo {
    pub running_in_background: bool,
    pub checking_now: bool,
    pub os_service_registered: bool,
    pub os_service_kind: String, // "Windows Service (svchost / sc.exe)" | "Linux systemd user service" | "macOS launchd agent"
    pub poll_interval_seconds: u64,
    pub pending_updates: Vec<RepoUpdateNotification>,
}

pub struct BackgroundWatcherControl {
    pub enabled: Arc<AtomicBool>,
    pub checking_now: Arc<AtomicBool>,
    pub os_service_registered: Arc<AtomicBool>,
}

impl Default for BackgroundWatcherControl {
    fn default() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(true)),
            checking_now: Arc::new(AtomicBool::new(false)),
            os_service_registered: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Checks a single local Git repository against its remote (`origin`) to see if remote has newer commits.
pub fn check_single_repo_for_remote_update(repo_path: &Path) -> Option<RepoUpdateNotification> {
    let repo = Repository::open(repo_path).ok()?;
    let head = repo.head().ok()?;
    if !head.is_branch() {
        return None;
    }

    let branch_name = head.shorthand()?.to_string();
    let local_oid = head.target()?;

    let repo_name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Repository".to_string());

    // First try fast `git ls-remote origin refs/heads/<branch>` or `libgit2` remote connection
    let mut remote = repo.find_remote("origin").ok()?;
    let mut remote_oid_opt = None;

    if remote.connect(Direction::Fetch).is_ok() {
        if let Ok(list) = remote.list() {
            let target_ref = format!("refs/heads/{}", branch_name);
            for head_item in list {
                if head_item.name() == target_ref {
                    remote_oid_opt = Some(head_item.oid());
                    break;
                }
            }
        }
        let _ = remote.disconnect();
    }

    let remote_oid = remote_oid_opt?;
    if remote_oid == local_oid {
        return None;
    }

    // Fetch latest objects into refs/remotes/origin/<branch> so we can compute exact ahead/behind count
    let mut fetch_opts = FetchOptions::new();
    let refspec = format!("refs/heads/{0}:refs/remotes/origin/{0}", branch_name);
    let _ = remote.fetch(&[&refspec], Some(&mut fetch_opts), None);

    let commits_behind = match repo.graph_ahead_behind(local_oid, remote_oid) {
        Ok((_ahead, behind)) => {
            if behind == 0 {
                return None;
            }
            behind
        }
        Err(_) => 1, // Remote OID differs and isn't in local graph yet -> at least 1 new remote commit
    };

    let canonical = std::fs::canonicalize(repo_path)
        .unwrap_or_else(|_| repo_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let prompt_message = format!(
        "Hay you have {} update do you want to update it?",
        repo_name
    );

    Some(RepoUpdateNotification {
        repo_id: format!("local::{}", canonical),
        repo_name,
        repo_path: canonical,
        branch: branch_name,
        local_oid: local_oid.to_string(),
        remote_oid: remote_oid.to_string(),
        commits_behind,
        prompt_message,
    })
}

/// Scans all local repositories that have remotes and checks if any have upstream updates available.
pub fn check_all_repos_for_updates(root_paths: Vec<String>) -> Vec<RepoUpdateNotification> {
    let roots = if root_paths.is_empty() {
        default_scan_roots()
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    } else {
        root_paths
    };

    let repos = match scan_repositories(ScanOptions {
        root_paths: roots,
        max_depth: Some(5),
        include_hidden_dirs: Some(false),
        // Background update polling is limited to primary project repositories.
        index_dependency_repositories: Some(false),
    }) {
        Ok(list) => list,
        Err(_) => return Vec::new(),
    };

    let mut updates = Vec::new();
    for repo in repos.into_iter().filter(|r| r.has_remote) {
        let p = Path::new(&repo.path);
        if let Some(notification) = check_single_repo_for_remote_update(p) {
            updates.push(notification);
        }
    }
    updates
}

/// Pulls / fast-forwards the specified local repository from `origin/<branch>` when the user clicks "Update Now".
pub fn apply_repository_update(repo_path: &Path) -> Result<String, String> {
    // Try native `git pull --ff-only` first for clean working tree update, fallback to libgit2 fast-forward
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("pull")
        .arg("--rebase=false")
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(if stdout.is_empty() {
                "Repository updated to latest remote commit.".to_string()
            } else {
                stdout
            });
        }
    }

    // Fallback using libgit2 fast-forward merge
    let repo = Repository::open(repo_path).map_err(|e| e.to_string())?;
    let head = repo.head().map_err(|e| e.to_string())?;
    let branch_name = head
        .shorthand()
        .ok_or_else(|| "Detached HEAD cannot be fast-forwarded".to_string())?
        .to_string();

    let mut remote = repo.find_remote("origin").map_err(|e| e.to_string())?;
    let mut fetch_opts = FetchOptions::new();
    let refspec = format!("refs/heads/{0}:refs/remotes/origin/{0}", branch_name);
    remote
        .fetch(&[&refspec], Some(&mut fetch_opts), None)
        .map_err(|e| format!("Fetch failed: {}", e))?;

    let fetch_head_ref = format!("refs/remotes/origin/{}", branch_name);
    let fetch_commit_oid = repo
        .refname_to_id(&fetch_head_ref)
        .map_err(|e| format!("Could not resolve {}: {}", fetch_head_ref, e))?;
    let fetch_commit = repo.find_object(fetch_commit_oid, None).map_err(|e| e.to_string())?;

    repo.checkout_tree(&fetch_commit, None)
        .map_err(|e| format!("Checkout conflict during update: {}", e))?;

    let mut head_ref = repo.head().map_err(|e| e.to_string())?;
    head_ref
        .set_target(fetch_commit_oid, "GitManager background service fast-forward update")
        .map_err(|e| e.to_string())?;

    Ok(format!(
        "Updated {} ({}) to {}",
        repo_path.display(),
        branch_name,
        &fetch_commit_oid.to_string()[..7]
    ))
}

/// Registers or starts the OS-level background watcher service:
/// - Windows 10/11: registers `GitManagerUpdateSvc` via `sc.exe` (hosted alongside Windows services / `svchost` model)
///   or launches the background service daemon `git-manager-service.exe --background-service`.
/// - Linux: writes `~/.config/systemd/user/gitmanager-updater.service` and starts it via `systemctl --user`.
/// - macOS: writes `~/Library/LaunchAgents/com.gitmanager.updater.plist` and loads it via `launchctl`.
pub fn register_os_background_service() -> Result<String, String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let service_bin = current_exe
        .parent()
        .map(|p| {
            let candidate = if cfg!(windows) {
                p.join("git-manager-service.exe")
            } else {
                p.join("git-manager-service")
            };
            if candidate.exists() {
                candidate
            } else {
                current_exe.clone()
            }
        })
        .unwrap_or(current_exe);

    #[cfg(target_os = "windows")]
    {
        let bin_quoted = format!("\"{}\" --svchost-worker", service_bin.display());
        let sc_out = Command::new("sc.exe")
            .args([
                "create",
                "GitManagerUpdateSvc",
                &format!("binPath= {}", bin_quoted),
                "start= auto",
                "DisplayName= \"GitManager Background Repository Update Service\"",
            ])
            .output();

        // Also ensure background worker process is spawned immediately even without Administrator elevation
        let _ = Command::new(&service_bin)
            .arg("--svchost-worker")
            .spawn();

        match sc_out {
            Ok(o) if o.status.success() => {
                let _ = Command::new("sc.exe").args(["start", "GitManagerUpdateSvc"]).output();
                return Ok("Registered & started Windows Service 'GitManagerUpdateSvc' (sc.exe / svchost background model).".to_string());
            }
            _ => {
                return Ok("Started GitManager background update service worker process (Windows user-session daemon).".to_string());
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            let systemd_dir = home.join(".config/systemd/user");
            let _ = std::fs::create_dir_all(&systemd_dir);
            let unit_path = systemd_dir.join("gitmanager-updater.service");
            let unit_content = format!(
                "[Unit]\nDescription=GitManager Background Repository Update Watcher\nAfter=network.target\n\n[Service]\nExecStart=\"{}\" --svchost-worker\nRestart=always\nRestartSec=60\n\n[Install]\nWantedBy=default.target\n",
                service_bin.display()
            );
            let _ = std::fs::write(&unit_path, unit_content);
            let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).output();
            let _ = Command::new("systemctl")
                .args(["--user", "enable", "--now", "gitmanager-updater.service"])
                .output();
        }
        return Ok("Registered & started Linux systemd user service (gitmanager-updater.service).".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let agents_dir = home.join("Library/LaunchAgents");
            let _ = std::fs::create_dir_all(&agents_dir);
            let plist_path = agents_dir.join("com.gitmanager.updater.plist");
            let plist_content = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>com.gitmanager.updater</string>\n  <key>ProgramArguments</key>\n  <array><string>{}</string><string>--svchost-worker</string></array>\n  <key>RunAtLoad</key><true/>\n  <key>StartInterval</key><integer>180</integer>\n</dict>\n</plist>\n",
                service_bin.display()
            );
            let _ = std::fs::write(&plist_path, plist_content);
            let _ = Command::new("launchctl").args(["load", "-w", &plist_path.to_string_lossy()]).output();
        }
        return Ok("Registered & started macOS LaunchAgent (com.gitmanager.updater).".to_string());
    }

    #[allow(unreachable_code)]
    Ok("Background update watcher service active.".to_string())
}

/// Triggers an OS-level desktop toast notification when the background service finds a repo update
pub fn send_os_desktop_notification(notification: &RepoUpdateNotification) {
    let title = format!("GitManager — {} Update Available", notification.repo_name);
    let body = &notification.prompt_message;

    #[cfg(target_os = "windows")]
    {
        let ps_script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
             $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $textNodes = $template.GetElementsByTagName('text'); \
             $textNodes.Item(0).AppendChild($template.CreateTextNode('{}')) > $null; \
             $textNodes.Item(1).AppendChild($template.CreateTextNode('{}')) > $null; \
             $toast = [Windows.UI.Notifications.ToastNotification]::new($template); \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('GitManager').Show($toast);",
            title.replace('\'', ""),
            body.replace('\'', "")
        );
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_script])
            .spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("notify-send")
            .args(["-a", "GitManager", &title, body])
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let apple_script = format!(
            "display notification \"{}\" with title \"{}\"",
            body.replace('"', "\\\""),
            title.replace('"', "\\\"")
        );
        let _ = Command::new("osascript").args(["-e", &apple_script]).spawn();
    }
}

/// Spawns the in-app background Tokio loop that periodically checks all indexed repositories
/// for remote updates and emits real-time events to the Tauri UI (with loading spinner state).
pub fn spawn_background_update_watcher(
    app_handle: AppHandle,
    enabled: Arc<AtomicBool>,
    checking_now: Arc<AtomicBool>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(120));
        loop {
            interval.tick().await;
            if !enabled.load(Ordering::Relaxed) {
                continue;
            }

            checking_now.store(true, Ordering::Relaxed);
            let _ = app_handle.emit("gitmanager://update-check-state", serde_json::json!({
                "checking": true,
                "status": "Background Service scanning remotes for updates..."
            }));

            let found_updates = tokio::task::spawn_blocking(|| {
                check_all_repos_for_updates(Vec::new())
            })
            .await
            .unwrap_or_default();

            checking_now.store(false, Ordering::Relaxed);
            let _ = app_handle.emit("gitmanager://update-check-state", serde_json::json!({
                "checking": false,
                "updates_count": found_updates.len()
            }));

            for upd in found_updates {
                send_os_desktop_notification(&upd);
                let _ = app_handle.emit("gitmanager://repo-update-available", &upd);
            }
        }
    });
}
