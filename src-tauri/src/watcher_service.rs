use git2::{Direction, FetchOptions, Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use crate::process_utils::hidden_command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

use crate::repository_index::load_persisted_repository_index;

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
    use super::{is_legacy_service_command, repository_worktree_is_clean};

    #[test]
    fn legacy_worker_names_are_distinguished_from_the_current_service() {
        assert!(is_legacy_service_command(r"C:\GitManager\git-manager-service.exe"));
        assert!(is_legacy_service_command("/opt/gitmanager/git-manager-service"));
        assert!(!is_legacy_service_command(r"C:\GitManager\gitmanager-service.exe"));
    }

    #[cfg(unix)]
    #[test]
    fn current_core_service_executable_name_is_detected() {
        assert!(super::is_core_service_command(r"/opt/gitmanager/gitmanager-service"));
        assert!(super::is_core_service_command(r"/opt/gitmanager/gitmanager-service --background"));
        assert!(!super::is_core_service_command(r"/opt/gitmanager/git-manager-service"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn flatpak_background_service_is_started_inside_its_sandbox() {
        use super::{linux_background_service_command, systemd_exec_start};
        use std::path::Path;

        let command = linux_background_service_command(
            Path::new("/app/bin/gitmanager.service"),
            Some("com.gitmanager.desktop"),
            Some(Path::new("flatpak")),
        )
        .expect("construct Flatpak service command");

        assert_eq!(
            command,
            vec![
                "flatpak".to_string(),
                "run".to_string(),
                "--command=gitmanager.service".to_string(),
                "com.gitmanager.desktop".to_string(),
            ]
        );
        let exec_start = systemd_exec_start(&command);
        assert!(exec_start.starts_with("\"flatpak\""));
        assert!(!exec_start.contains("/app/bin/gitmanager.service"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_linux_background_service_uses_its_packaged_executable() {
        use super::linux_background_service_command;
        use std::path::Path;

        let command = linux_background_service_command(
            Path::new("/usr/lib/gitmanager/gitmanager.service"),
            None,
            None,
        )
        .expect("construct native Linux service command");

        assert_eq!(
            command,
            vec!["/usr/lib/gitmanager/gitmanager.service".to_string()]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn flatpak_service_registration_fails_clearly_without_flatpak_cli() {
        use super::linux_background_service_command;
        use std::path::Path;

        let error = linux_background_service_command(
            Path::new("/app/bin/gitmanager.service"),
            Some("com.gitmanager.desktop"),
            None,
        )
        .expect_err("missing host Flatpak CLI must not create a broken unit");

        assert!(error.contains("Flatpak command was not found"));
    }
    #[test]
    fn legacy_worker_detection_matches_only_the_old_executable_identity() {
        assert!(is_legacy_service_command("/opt/gitmanager/git-manager-service"));
        assert!(is_legacy_service_command("--command=git-manager-service"));
        assert!(is_legacy_service_command(r"C:\\GitManager\\git-manager-service.exe"));
        assert!(!is_legacy_service_command("/opt/gitmanager/gitmanager.service"));
        assert!(!is_legacy_service_command("gitmanager-service"));
    }

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

/// Checks the service-owned repository index for upstream updates.
/// This path deliberately never walks scan roots; repository discovery belongs
/// to gitmanager.service and is refreshed independently of the UI.
pub fn check_all_repos_for_updates(_root_paths: Vec<String>) -> Vec<RepoUpdateNotification> {
    let repositories = match load_persisted_repository_index() {
        Ok(Some(index)) => index.repositories,
        Ok(None) | Err(_) => return Vec::new(),
    };

    let mut updates = Vec::new();
    for repo in repositories.into_iter().filter(|repository| repository.has_remote) {
        if let Some(notification) = check_single_repo_for_remote_update(Path::new(&repo.path)) {
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
/// - Linux: writes `~/.config/systemd/user/gitmanager-core.service` and starts it via `systemctl --user`.
/// - macOS: writes `~/Library/LaunchAgents/com.gitmanager.core.plist` and loads it via `launchctl`.
///
/// The worker must run as the interactive user so it reads the same per-user
/// preferences and holds the same archive-watcher lease as the desktop app.
#[cfg(target_os = "linux")]
const GITMANAGER_FLATPAK_APP_ID: &str = "com.gitmanager.desktop";

#[cfg(target_os = "linux")]
fn systemd_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('\n', "");
    format!("\"{escaped}\"")
}

#[cfg(target_os = "linux")]
fn linux_background_service_command(
    service_executable: &Path,
    flatpak_id: Option<&str>,
    flatpak_executable: Option<&Path>,
) -> Result<Vec<String>, String> {
    if flatpak_id == Some(GITMANAGER_FLATPAK_APP_ID) {
        let flatpak = flatpak_executable.ok_or_else(|| {
            "GitManager is running as a Flatpak, but the host Flatpak command was not found on PATH."
                .to_string()
        })?;
        return Ok(vec![
            flatpak.to_string_lossy().into_owned(),
            "run".to_string(),
            "--command=gitmanager.service".to_string(),
            GITMANAGER_FLATPAK_APP_ID.to_string(),
        ]);
    }

    Ok(vec![service_executable.to_string_lossy().into_owned()])
}

#[cfg(target_os = "linux")]
fn systemd_exec_start(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| systemd_quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn resolve_background_service_executable(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Could not resolve GitManager's executable: {error}"))?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let mut directories = Vec::new();

    if let Some(parent) = current_exe.parent() {
        directories.push(parent.to_path_buf());
    }
    if let Ok(resources) = app_handle.path().resource_dir() {
        directories.push(resources);
    }
    if let Some(parent) = current_exe.parent().and_then(Path::parent) {
        directories.push(parent.join("Resources"));
        directories.push(parent.join("resources"));
    }

    for directory in directories {
        let direct = directory.join(format!("gitmanager.service{suffix}"));
        if direct.is_file() {
            return Ok(direct);
        }
        if let Ok(entries) = std::fs::read_dir(&directory) {
            let mut candidates = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("gitmanager.service-") && name.ends_with(suffix))
                })
                .collect::<Vec<_>>();
            candidates.sort();
            if let Some(path) = candidates.into_iter().next() {
                return Ok(path);
            }
        }
    }

    Err("The packaged gitmanager.service executable was not found. Rebuild the installer with the service sidecar included.".to_string())
}

const SERVICE_LOG_LIMIT_BYTES: u64 = 4 * 1024 * 1024;
const SERVICE_LOG_TAIL_BYTES: u64 = 64 * 1024;

fn service_log_path() -> Result<PathBuf, String> {
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the user's local data directory for service logs.".to_string())?;
    let directory = local_data
        .join("kastrick's_software")
        .join("data")
        .join("GitManager")
        .join("log");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create GitManager service log directory: {error}"))?;
    Ok(directory.join("service.log"))
}

fn open_service_log_streams() -> Result<(Stdio, Stdio), String> {
    let path = service_log_path()?;
    if fs::metadata(&path).map(|metadata| metadata.len() > SERVICE_LOG_LIMIT_BYTES).unwrap_or(false) {
        let rotated = path.with_extension("log.1");
        if rotated.exists() {
            fs::remove_file(&rotated)
                .map_err(|error| format!("Could not remove previous service log: {error}"))?;
        }
        fs::rename(&path, &rotated)
            .map_err(|error| format!("Could not rotate GitManager service log: {error}"))?;
    }
    let output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Could not open GitManager service log {}: {error}", path.display()))?;
    let errors = output.try_clone()
        .map_err(|error| format!("Could not duplicate GitManager service log handle: {error}"))?;
    Ok((Stdio::from(output), Stdio::from(errors)))
}

/// Returns the most recent 64 KiB of the required service's log for Task Center diagnostics.
pub fn read_service_log_tail() -> Result<String, String> {
    let path = service_log_path()?;
    if !path.is_file() {
        return Ok("The core service has not written a log yet.".to_string());
    }
    let mut file = File::open(&path)
        .map_err(|error| format!("Could not open GitManager service log: {error}"))?;
    let length = file.metadata()
        .map_err(|error| format!("Could not inspect GitManager service log: {error}"))?
        .len();
    let offset = length.saturating_sub(SERVICE_LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Could not seek GitManager service log: {error}"))?;
    let mut bytes = Vec::with_capacity((length - offset) as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read GitManager service log: {error}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_legacy_service_command(command: &str) -> bool {
    let mut arguments = command.split_whitespace();
    let executable = arguments.next().unwrap_or("");
    let normalized = executable.replace('\\', "/");
    let executable_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    matches!(executable_name, "git-manager-service" | "git-manager-service.exe")
        || arguments.any(|argument| argument == "--command=git-manager-service")
}

#[cfg(unix)]
fn stop_legacy_worker_processes() {
    #[cfg(target_os = "linux")]
    let output = hidden_command("ps").args(["-eo", "pid=,args="]).output();
    #[cfg(target_os = "macos")]
    let output = hidden_command("ps").args(["-axo", "pid=,command="]).output();

    let Ok(output) = output else { return; };
    let own_pid = std::process::id();
    let mut stopped_any = false;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.trim().splitn(2, char::is_whitespace);
        let Some(pid_text) = fields.next() else { continue; };
        let Ok(pid) = pid_text.trim().parse::<u32>() else { continue; };
        let command = fields.next().unwrap_or("").trim();
        if pid > 1 && pid != own_pid && is_legacy_service_command(command) {
            let _ = hidden_command("kill").args(["-TERM", &pid.to_string()]).output();
            stopped_any = true;
        }
    }
    if stopped_any {
        // The old worker uses the same per-user lock and endpoint files. Give
        // the OS a moment to release its lock before the replacement starts.
        thread::sleep(Duration::from_millis(300));
    }
}

/// Stops PR #9's optional updater before the required core service starts.
/// The old process used the same IPC endpoint and lock file, so accepting its
/// status response would make the new UI believe an obsolete worker was ready.
fn stop_legacy_background_service() {
    #[cfg(windows)]
    {
        const LEGACY_TASK: &str = "GitManager Background Service";
        let _ = hidden_command("schtasks.exe")
            .args(["/End", "/TN", LEGACY_TASK])
            .output();
        let _ = hidden_command("schtasks.exe")
            .args(["/Delete", "/TN", LEGACY_TASK, "/F"])
            .output();
        let _ = hidden_command("taskkill.exe")
            .args(["/IM", "git-manager-service.exe", "/T", "/F"])
            .output();
        let _ = hidden_command("sc.exe").args(["stop", "GitManagerUpdateSvc"]).output();
        let _ = hidden_command("sc.exe").args(["delete", "GitManagerUpdateSvc"]).output();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = hidden_command("systemctl")
            .args(["--user", "stop", "gitmanager-updater.service"])
            .output();
        let _ = hidden_command("systemctl")
            .args(["--user", "disable", "gitmanager-updater.service"])
            .output();
        if let Some(home) = dirs::home_dir() {
            let legacy_unit = home
                .join(".config")
                .join("systemd")
                .join("user")
                .join("gitmanager-updater.service");
            let _ = fs::remove_file(legacy_unit);
        }
        let _ = hidden_command("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
        stop_legacy_worker_processes();
    }

    #[cfg(target_os = "macos")]
    {
        let uid = hidden_command("id")
            .arg("-u")
            .output()
            .ok()
            .filter(|result| result.status.success())
            .map(|result| String::from_utf8_lossy(&result.stdout).trim().to_string())
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()));
        if let Some(uid) = uid {
            let domain = format!("gui/{uid}");
            let service = format!("{domain}/com.gitmanager.updater");
            let _ = hidden_command("launchctl").args(["bootout", &service]).output();
            let _ = hidden_command("launchctl").args(["disable", &service]).output();
        }
        if let Some(home) = dirs::home_dir() {
            let legacy_agent = home
                .join("Library")
                .join("LaunchAgents")
                .join("com.gitmanager.updater.plist");
            let _ = fs::remove_file(legacy_agent);
        }
        stop_legacy_worker_processes();
    }
}

#[cfg(unix)]
fn is_core_service_command(command: &str) -> bool {
    let mut arguments = command.split_whitespace();
    let executable = arguments.next().unwrap_or("");
    let normalized = executable.replace('\\', "/");
    let executable_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    matches!(executable_name, "gitmanager-service" | "gitmanager-service.exe")
}

#[cfg(windows)]
fn stop_outdated_core_service(reported_pid: u32) {
    if reported_pid > 0 {
        let _ = hidden_command("taskkill.exe")
            .args(["/PID", &reported_pid.to_string(), "/T", "/F"])
            .output();
    } else {
        // Protocol-v1 workers do not report a PID. Restrict fallback cleanup
        // to this interactive Windows account rather than other user sessions.
        let username = hidden_command("whoami.exe")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(username) = username {
            let filter = format!("USERNAME eq {username}");
            let _ = hidden_command("taskkill.exe")
                .args(["/FI", &filter, "/IM", "gitmanager-service.exe", "/T", "/F"])
                .output();
        }
    }
    thread::sleep(Duration::from_millis(500));
}

#[cfg(unix)]
fn stop_outdated_core_service(reported_pid: u32) {
    let mut stopped_any = false;
    if reported_pid > 1 && reported_pid != std::process::id() {
        let _ = hidden_command("kill")
            .args(["-TERM", &reported_pid.to_string()])
            .output();
        stopped_any = true;
    } else {
        let username = std::env::var("USER").ok().filter(|value| !value.is_empty())
            .or_else(|| hidden_command("id").arg("-un").output().ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .filter(|value| !value.is_empty()));
        let Some(username) = username else { return; };
        #[cfg(target_os = "linux")]
        let output = hidden_command("ps").args(["-u", &username, "-o", "pid=,args="]).output();
        #[cfg(target_os = "macos")]
        let output = hidden_command("ps").args(["-u", &username, "-o", "pid=,command="]).output();
        let Ok(output) = output else { return; };

        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut fields = line.trim().splitn(2, char::is_whitespace);
            let Some(pid_text) = fields.next() else { continue; };
            let Ok(pid) = pid_text.trim().parse::<u32>() else { continue; };
            let command = fields.next().unwrap_or("").trim();
            if pid > 1 && pid != std::process::id() && is_core_service_command(command) {
                let _ = hidden_command("kill").args(["-TERM", &pid.to_string()]).output();
                stopped_any = true;
            }
        }
    }

    if stopped_any {
        // Let the service release its single-instance lock and remove its endpoint.
        thread::sleep(Duration::from_millis(500));
    }
}

/// Ensures the required repository/data engine is running before the UI becomes
/// functional. The service is launched as a detached per-user process and uses
/// its own authenticated local control endpoint for its readiness handshake.
pub fn ensure_required_service(app_handle: AppHandle) -> Result<String, String> {
    // Always remove the legacy logon registration, even when a current core
    // service is already running, so the old worker cannot return next login.
    stop_legacy_background_service();

    if let Ok(status) = crash_helper::request_service_status() {
        if status.service_generation >= crash_helper::SERVICE_GENERATION
            && status.protocol_version >= 2
        {
            return Ok(status.message);
        }
        if status.service_generation > 0 || status.protocol_version > 0 {
            stop_outdated_core_service(status.process_id);
        }
    }

    let service_exe = resolve_background_service_executable(&app_handle)?;
    let (service_stdout, service_stderr) = open_service_log_streams()?;
    let mut command = Command::new(&service_exe);
    command.stdin(Stdio::null())
        .stdout(service_stdout)
        .stderr(service_stderr);

    // The UI is isolated in its own process group on Unix and a kill-on-close
    // Job Object on Windows. Break away so the long-lived data engine does not
    // inherit the UI's crash/exit termination boundary.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        command.creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command.spawn().map_err(|error| {
        format!(
            "Could not start required GitManager core service at {}: {error}",
            service_exe.display()
        )
    })?;

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(status) = crash_helper::request_service_status() {
            if status.service_generation >= crash_helper::SERVICE_GENERATION
                && status.protocol_version >= 2
            {
                return Ok(status.message);
            }
        }
        if let Ok(Some(exit)) = child.try_wait() {
            return Err(format!(
                "GitManager core service exited before becoming ready ({exit}). Check Task Center → Service diagnostics."
            ));
        }
        if Instant::now() >= deadline {
            return Err(
                "GitManager core service did not become ready within 20 seconds. The UI is waiting for its repository engine; open Service diagnostics or retry."
                    .to_string()
            );
        }
        thread::sleep(Duration::from_millis(150));
    }
}

/// Registers or starts the dedicated service executable. The service remains
/// independent from the Tauri UI and crash_helper process.
pub fn register_os_background_service(app_handle: AppHandle) -> Result<String, String> {
    let service_exe = resolve_background_service_executable(&app_handle)?;

    #[cfg(target_os = "windows")]
    {
        let task_name = "GitManager Core Service";
        let task_command = format!("\"{}\"", service_exe.display());
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
            return Err(format!("Could not register GitManager Core Service: {}", detail.trim()));
        }

        let start = hidden_command("schtasks.exe")
            .args(["/Run", "/TN", task_name])
            .output()
            .map_err(|error| format!("Could not start GitManager Core Service: {error}"))?;
        if !start.status.success() {
            let detail = String::from_utf8_lossy(&start.stderr);
            let detail = if detail.trim().is_empty() {
                String::from_utf8_lossy(&start.stdout).into_owned()
            } else {
                detail.into_owned()
            };
            return Err(format!("GitManager Core Service was registered but could not start: {}", detail.trim()));
        }

        let _ = hidden_command("sc.exe").args(["stop", "GitManagerUpdateSvc"]).output();
        let _ = hidden_command("sc.exe").args(["delete", "GitManagerUpdateSvc"]).output();

        return Ok(format!(
            "Registered and started the independent Windows background service at {}.",
            service_exe.display()
        ));
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir()
            .ok_or_else(|| "Could not resolve the user's home directory for systemd.".to_string())?;
        let systemd_dir = home.join(".config/systemd/user");
        std::fs::create_dir_all(&systemd_dir)
            .map_err(|error| format!("Could not create {}: {error}", systemd_dir.display()))?;
        let unit_path = systemd_dir.join("gitmanager-core.service");
        let flatpak_id = std::env::var("FLATPAK_ID").ok();
        // This code runs inside the Flatpak sandbox, where the host's Flatpak CLI
        // is not visible on PATH. A bare command name lets the host systemd user
        // manager resolve it using its normal host executable search path.
        let flatpak_executable = if flatpak_id.as_deref() == Some(GITMANAGER_FLATPAK_APP_ID) {
            Some(PathBuf::from("flatpak"))
        } else {
            None
        };
        let service_command = linux_background_service_command(
            &service_exe,
            flatpak_id.as_deref(),
            flatpak_executable.as_deref(),
        )?;
        let exec_start = systemd_exec_start(&service_command);
        let unit_content = format!(
            r#"[Unit]
Description=GitManager Background Repository and Archive Watcher
Wants=network-online.target
After=network-online.target
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=simple
ExecStart={}
Restart=on-failure
RestartSec=10
UMask=0077

[Install]
WantedBy=default.target
"#,
            exec_start
        );
        std::fs::write(&unit_path, unit_content)
            .map_err(|error| format!("Could not write {}: {error}", unit_path.display()))?;

        let reload = hidden_command("systemctl")
            .args(["--user", "daemon-reload"])
            .output()
            .map_err(|error| format!("Could not reload the systemd user manager: {error}"))?;
        if !reload.status.success() {
            return Err(format!("Could not reload systemd user manager: {}", String::from_utf8_lossy(&reload.stderr).trim()));
        }
        let enable = hidden_command("systemctl")
            .args(["--user", "enable", "--now", "gitmanager-core.service"])
            .output()
            .map_err(|error| format!("Could not enable GitManager's systemd user service: {error}"))?;
        if !enable.status.success() {
            return Err(format!("Could not enable/start GitManager's systemd user service: {}", String::from_utf8_lossy(&enable.stderr).trim()));
        }
        if flatpak_id.as_deref() == Some(GITMANAGER_FLATPAK_APP_ID) {
            return Ok(
                "Registered and started the GitManager Flatpak background worker through the host systemd user manager."
                    .to_string(),
            );
        }
        return Ok(format!("Registered and started the independent Linux systemd service at {}.", service_exe.display()));
    }

    #[cfg(target_os = "macos")]
    {
        let uid_result = hidden_command("id").arg("-u").output()
            .map_err(|error| format!("Could not determine the current macOS user ID: {error}"))?;
        if !uid_result.status.success() {
            return Err("Could not determine the current macOS user ID.".to_string());
        }
        let uid = String::from_utf8_lossy(&uid_result.stdout).trim().to_string();
        if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("The current macOS user ID was invalid.".to_string());
        }
        let agents_dir = dirs::home_dir()
            .ok_or_else(|| "Could not resolve the user's home directory for LaunchAgents.".to_string())?
            .join("Library/LaunchAgents");
        std::fs::create_dir_all(&agents_dir)
            .map_err(|error| format!("Could not create {}: {error}", agents_dir.display()))?;
        let label = "com.gitmanager.core";
        let plist_path = agents_dir.join(format!("{label}.plist"));
        let executable = xml_escape(&service_exe.to_string_lossy());
        let plist_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.gitmanager.core</string>
  <key>ProgramArguments</key>
  <array><string>{executable}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>30</integer>
  <key>ProcessType</key><string>Background</string>
</dict>
</plist>
"#
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
            return Err(format!("Could not load the GitManager LaunchAgent: {}", String::from_utf8_lossy(&bootstrap.stderr).trim()));
        }
        let enable = hidden_command("launchctl")
            .args(["enable", &format!("{domain}/{label}")])
            .output()
            .map_err(|error| format!("Could not enable the GitManager LaunchAgent: {error}"))?;
        if !enable.status.success() {
            return Err(format!("Could not enable the GitManager LaunchAgent: {}", String::from_utf8_lossy(&enable.stderr).trim()));
        }
        let start = hidden_command("launchctl")
            .args(["kickstart", "-k", &format!("{domain}/{label}")])
            .output()
            .map_err(|error| format!("Could not start the GitManager LaunchAgent: {error}"))?;
        if !start.status.success() {
            return Err(format!("Could not start the GitManager LaunchAgent: {}", String::from_utf8_lossy(&start.stderr).trim()));
        }
        return Ok(format!("Registered and started the independent macOS LaunchAgent at {}.", service_exe.display()));
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
