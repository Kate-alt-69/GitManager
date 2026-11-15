use git2::{Direction, FetchOptions, Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::process_utils::hidden_command;
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
    pub commits_ahead: usize,
    pub working_tree_clean: bool,
    pub prompt_message: String, // "Hay you have <X-REPO_NAME> update do you want to update it?"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatusInfo {
    pub running_in_background: bool,
    pub checking_now: bool,
    pub os_service_registered: bool,
    pub os_service_kind: String, // "Windows Task Scheduler (user logon)" | "Linux systemd user service" | "macOS launchd agent"
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

fn repository_worktree_is_clean(repo: &Repository) -> Result<bool, String> {
    let mut status_options = StatusOptions::new();
    status_options.include_untracked(true).recurse_untracked_dirs(true);
    repo.statuses(Some(&mut status_options))
        .map(|statuses| statuses.is_empty())
        .map_err(|error| format!("Could not inspect repository status: {error}"))
}

#[cfg(test)]
mod tests {
    use super::repository_worktree_is_clean;
    use git2::Repository;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn untracked_files_make_a_worktree_ineligible_for_auto_update() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("gitmanager-update-check-{nonce}"));
        fs::create_dir_all(&path).expect("create temporary repository directory");
        let repo = Repository::init(&path).expect("initialize temporary Git repository");
        assert!(repository_worktree_is_clean(&repo).expect("inspect empty worktree"));
        fs::write(path.join("untracked.txt"), "keep this file").expect("write untracked fixture");
        assert!(!repository_worktree_is_clean(&repo).expect("inspect untracked worktree"));
        drop(repo);
        fs::remove_dir_all(&path).expect("remove temporary repository directory");
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

    let (commits_ahead, commits_behind) = match repo.graph_ahead_behind(local_oid, remote_oid) {
        Ok((ahead, behind)) => {
            if behind == 0 {
                return None;
            }
            (ahead, behind)
        }
        // Unknown ancestry is never eligible for automatic updates.
        Err(_) => (usize::MAX, 1),
    };

    let working_tree_clean = repository_worktree_is_clean(&repo).unwrap_or(false);

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
        commits_ahead,
        working_tree_clean,
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
    let repo = Repository::open(repo_path).map_err(|error| error.to_string())?;
    let head = repo.head().map_err(|error| error.to_string())?;
    if !head.is_branch() {
        return Err("Detached HEAD cannot be automatically updated.".to_string());
    }
    if !repository_worktree_is_clean(&repo)? {
        return Err("GitManager did not update this repository because it has uncommitted or untracked files. Commit or stash them first.".to_string());
    }

    // Prefer Git's explicit fast-forward-only pull. It must never create a merge commit.
    let output = hidden_command("git")
        .arg("-C")
        .arg(repo_path)
        .arg("pull")
        .arg("--ff-only")
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
    let local_oid = repo.head().map_err(|error| error.to_string())?
        .target().ok_or_else(|| "Current branch has no commit.".to_string())?;
    let (ahead, behind) = repo.graph_ahead_behind(local_oid, fetch_commit_oid)
        .map_err(|error| format!("Could not verify fast-forward ancestry: {error}"))?;
    if ahead > 0 {
        return Err("GitManager refused to update because the local branch contains commits that are not on origin.".to_string());
    }
    if behind == 0 {
        return Ok("Repository is already up to date.".to_string());
    }

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

/// Registers or starts the per-user OS background watcher:
/// - Windows 10/11: creates and runs a current-user Task Scheduler logon task.
/// - Linux: writes `~/.config/systemd/user/gitmanager-updater.service` and starts it via `systemctl --user`.
/// - macOS: writes `~/Library/LaunchAgents/com.gitmanager.updater.plist` and loads it via `launchctl`.
///
/// The worker must run as the interactive user so it reads the same per-user
/// preferences and holds the same archive-watcher lease as the desktop app.
#[cfg(target_os = "linux")]
fn systemd_quote(value: &str) -> String {
    format!(
        "\\"{}\\"",
        value
            .replace('\\\\', "\\\\\\\\")
            .replace('"', "\\\\\"")
            .replace('%', "%%")
            .replace('\\n', "")
    )
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\\'', "&apos;")
}

pub fn register_os_background_service() -> Result<String, String> {
    // Tauri bundles the main executable, not an unbundled sibling BGW binary.
    // The installed application enters its shared headless worker when given
    // --background-service, so every platform launches the exact bundled file.
    let current_exe = std::env::current_exe().map_err(|error| {
        format!("Could not resolve the installed GitManager executable: {error}")
    })?;

    #[cfg(target_os = "windows")]
    {
        let task_name = "GitManager Background Service";
        let task_command = format!(
            "\\\"{}\\\" --background-service",
            current_exe.display()
        );
        let create = hidden_command("schtasks.exe")
            .args(["/Create", "/SC", "ONLOGON", "/TN", task_name, "/TR"])
            .arg(&task_command)
            .args(["/RL", "LIMITED", "/F"])
            .output()
            .map_err(|error| format!("Could not invoke Windows Task Scheduler: {error}"))?;
        if !create.status.success() {
            let detail = String::from_utf8_lossy(&create.stderr);
            let detail = if detail.trim().is_empty() {
                String::from_utf8_lossy(&create.stdout).into_owned()
            } else {
                detail.into_owned()
            };
            return Err(format!(
                "Could not register the current-user GitManager background task: {}",
                detail.trim()
            ));
        }

        let start = hidden_command("schtasks.exe")
            .args(["/Run", "/TN", task_name])
            .output()
            .map_err(|error| format!("Could not start the GitManager background task: {error}"))?;
        if !start.status.success() {
            let detail = String::from_utf8_lossy(&start.stderr);
            let detail = if detail.trim().is_empty() {
                String::from_utf8_lossy(&start.stdout).into_owned()
            } else {
                detail.into_owned()
            };
            return Err(format!(
                "The current-user GitManager task was registered but could not be started: {}",
                detail.trim()
            ));
        }

        // Best-effort cleanup of the previous experimental LocalSystem service.
        let _ = hidden_command("sc.exe")
            .args(["stop", "GitManagerUpdateSvc"])
            .output();
        let _ = hidden_command("sc.exe")
            .args(["delete", "GitManagerUpdateSvc"])
            .output();

        return Ok(
            "Registered and started the current-user Windows Task Scheduler job; it resumes at user logon."
                .to_string(),
        );
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir()
            .ok_or_else(|| "Could not resolve the user's home directory for systemd.".to_string())?;
        let systemd_dir = home.join(".config/systemd/user");
        std::fs::create_dir_all(&systemd_dir)
            .map_err(|error| format!("Could not create {}: {error}", systemd_dir.display()))?;
        let unit_path = systemd_dir.join("gitmanager-updater.service");
        let unit_content = format!(
            "[Unit]\\nDescription=GitManager Background Repository and Archive Watcher\\nWants=network-online.target\\nAfter=network-online.target\\n\\n[Service]\\nType=simple\\nExecStart={} --background-service\\nRestart=always\\nRestartSec=10\\n\\n[Install]\\nWantedBy=default.target\\n",
            systemd_quote(&current_exe.to_string_lossy())
        );
        std::fs::write(&unit_path, unit_content)
            .map_err(|error| format!("Could not write {}: {error}", unit_path.display()))?;

        let reload = hidden_command("systemctl")
            .args(["--user", "daemon-reload"])
            .output()
            .map_err(|error| format!("Could not reload the systemd user manager: {error}"))?;
        if !reload.status.success() {
            return Err(format!(
                "Could not reload the systemd user manager: {}",
                String::from_utf8_lossy(&reload.stderr).trim()
            ));
        }
        let enable = hidden_command("systemctl")
            .args(["--user", "enable", "--now", "gitmanager-updater.service"])
            .output()
            .map_err(|error| format!("Could not enable GitManager's systemd user service: {error}"))?;
        if !enable.status.success() {
            return Err(format!(
                "Could not enable/start GitManager's systemd user service: {}",
                String::from_utf8_lossy(&enable.stderr).trim()
            ));
        }
        return Ok("Registered and started the Linux per-user systemd service.".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let uid = unsafe { libc::getuid() };
        let agents_dir = dirs::home_dir()
            .ok_or_else(|| "Could not resolve the user's home directory for LaunchAgents.".to_string())?
            .join("Library/LaunchAgents");
        std::fs::create_dir_all(&agents_dir)
            .map_err(|error| format!("Could not create {}: {error}", agents_dir.display()))?;
        let label = "com.gitmanager.updater";
        let plist_path = agents_dir.join(format!("{label}.plist"));
        let executable = xml_escape(&current_exe.to_string_lossy());
        let plist_content = format!(
            "<?xml version=\\\"1.0\\\" encoding=\\\"UTF-8\\\"?>\\n<!DOCTYPE plist PUBLIC \\\"-//Apple//DTD PLIST 1.0//EN\\\" \\\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\\\">\\n<plist version=\\\"1.0\\\">\\n<dict>\\n  <key>Label</key><string>{label}</string>\\n  <key>ProgramArguments</key>\\n  <array><string>{executable}</string><string>--background-service</string></array>\\n  <key>RunAtLoad</key><true/>\\n  <key>KeepAlive</key><true/>\\n  <key>ProcessType</key><string>Background</string>\\n</dict>\\n</plist>\\n"
        );
        let temporary = plist_path.with_extension("plist.tmp");
        std::fs::write(&temporary, plist_content)
            .map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("Could not restrict LaunchAgent permissions: {error}"))?;
        }
        std::fs::rename(&temporary, &plist_path)
            .map_err(|error| format!("Could not publish {}: {error}", plist_path.display()))?;

        let domain = format!("gui/{uid}");
        let _ = hidden_command("launchctl")
            .args(["bootout", &format!("{domain}/{label}")])
            .output();
        let bootstrap = hidden_command("launchctl")
            .args(["bootstrap", &domain, &plist_path.to_string_lossy()])
            .output()
            .map_err(|error| format!("Could not load the GitManager LaunchAgent: {error}"))?;
        if !bootstrap.status.success() {
            return Err(format!(
                "Could not load the GitManager LaunchAgent: {}",
                String::from_utf8_lossy(&bootstrap.stderr).trim()
            ));
        }
        let enable = hidden_command("launchctl")
            .args(["enable", &format!("{domain}/{label}")])
            .output()
            .map_err(|error| format!("Could not enable the GitManager LaunchAgent: {error}"))?;
        if !enable.status.success() {
            return Err(format!(
                "Could not enable the GitManager LaunchAgent: {}",
                String::from_utf8_lossy(&enable.stderr).trim()
            ));
        }
        let start = hidden_command("launchctl")
            .args(["kickstart", "-k", &format!("{domain}/{label}")])
            .output()
            .map_err(|error| format!("Could not start the GitManager LaunchAgent: {error}"))?;
        if !start.status.success() {
            return Err(format!(
                "Could not start the GitManager LaunchAgent: {}",
                String::from_utf8_lossy(&start.stderr).trim()
            ));
        }
        return Ok("Registered and started the macOS per-user LaunchAgent.".to_string());
    }

    #[allow(unreachable_code)]
    Err("Background service registration is unavailable on this platform.".to_string())
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
        let _ = hidden_command("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_script])
            .spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = hidden_command("notify-send")
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
        let _ = hidden_command("osascript").args(["-e", &apple_script]).spawn();
    }
}

/// Spawns the in-app background Tokio loop that periodically checks all indexed repositories
/// for remote updates and emits real-time events to the Tauri UI (with loading spinner state).
pub fn spawn_background_update_watcher(
    app_handle: AppHandle,
    enabled: Arc<AtomicBool>,
    checking_now: Arc<AtomicBool>,
    scan_roots: Vec<String>,
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

            let roots = scan_roots.clone();
            let found_updates = tokio::task::spawn_blocking(move || {
                check_all_repos_for_updates(roots)
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
