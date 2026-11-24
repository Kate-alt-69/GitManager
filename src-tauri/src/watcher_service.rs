use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use crate::process_utils::hidden_command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};


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

#[cfg(test)]
mod tests {
    use super::is_legacy_service_command;

    #[test]
    fn legacy_worker_names_are_distinguished_from_the_current_service() {
        assert!(is_legacy_service_command(r"C:\GitManager\git-manager-service.exe"));
        assert!(is_legacy_service_command("/opt/gitmanager/git-manager-service"));
        assert!(!is_legacy_service_command(r"C:\GitManager\gitmanager-service.exe"));
    }

    #[cfg(unix)]
    #[test]
    fn current_core_service_executable_names_are_detected() {
        assert!(super::is_core_service_command(r"/opt/gitmanager/gitmanager.service"));
        assert!(super::is_core_service_command(r"/opt/gitmanager/gitmanager.service --background"));
        assert!(super::is_core_service_command(r"/opt/gitmanager/gitmanager.service.exe"));
        assert!(super::is_core_service_command(r"/opt/gitmanager/gitmanager-service"));
        assert!(super::is_core_service_command(r"/opt/gitmanager/gitmanager-service.exe"));
        assert!(super::is_core_service_command(r"flatpak run --command=gitmanager.service com.gitmanager.desktop"));
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


}

#[cfg(any(unix, test))]
fn is_legacy_service_command(command: &str) -> bool {
    command.split_whitespace().any(|argument| {
        let argument = argument.trim_matches('"').trim_matches('\'');
        if argument == "--command=git-manager-service" {
            return true;
        }
        let normalized = argument.replace('\\', "/");
        let executable_name = normalized.rsplit('/').next().unwrap_or(&normalized);
        matches!(executable_name, "git-manager-service" | "git-manager-service.exe")
    })
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
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
	let expected_name = format!("gitmanager.service{suffix}");
	let mut directories = Vec::new();

	if let Some(parent) = current_exe.parent() {
		directories.push(parent.to_path_buf());
		directories.push(parent.join("resources"));
		directories.push(parent.join("Resources"));
		directories.push(parent.join("binaries"));
		directories.push(parent.join("resources").join("binaries"));
	}
	if let Ok(resources) = app_handle.path().resource_dir() {
		directories.push(resources.clone());
		directories.push(resources.join("binaries"));
	}
	if let Some(grandparent) = current_exe.parent().and_then(Path::parent) {
		directories.push(grandparent.join("Resources"));
		directories.push(grandparent.join("resources"));
		directories.push(grandparent.join("binaries"));
	}

	for directory in &directories {
		for direct_name in [
			format!("gitmanager.service{suffix}"),
			format!("gitmanager-service{suffix}"),
		] {
			let direct = directory.join(direct_name);
			if direct.is_file() {
				return Ok(direct);
			}
		}
		if let Ok(entries) = std::fs::read_dir(directory) {
			let mut candidates = entries
				.flatten()
				.map(|entry| entry.path())
				.filter(|path| {
					path.is_file()
						&& path.file_name()
							.and_then(|name| name.to_str())
							.is_some_and(|raw| {
								let name = raw.to_ascii_lowercase();
								(name == format!("gitmanager.service{suffix}")
									|| name == format!("gitmanager-service{suffix}")
									|| name.starts_with("gitmanager.service-")
									|| name.starts_with("gitmanager-service-"))
									&& name.ends_with(suffix)
							})
				})
				.collect::<Vec<_>>();
			candidates.sort();
			if let Some(path) = candidates.into_iter().next() {
				return Ok(path);
			}
		}
	}

	Err(format!(
		"The packaged {expected_name} executable was not found. Rebuild the installer with the service sidecar included."
	))
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

/// Returns the most recent 64 KiB of the required service log for Task Center diagnostics.
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

#[cfg(unix)]
fn is_core_service_command(command: &str) -> bool {
    let mut arguments = command.split_whitespace();
    let executable = arguments.next().unwrap_or("");
    let normalized = executable.replace('\\', "/");
    let executable_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    matches!(
        executable_name,
        "gitmanager.service"
            | "gitmanager.service.exe"
            | "gitmanager-service"
            | "gitmanager-service.exe"
    ) || arguments.any(|argument| argument == "--command=gitmanager.service")
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
            // Installed builds use the dotted sidecar name; older development
            // builds used the hyphenated Cargo binary name.
            for image_name in ["gitmanager.service.exe", "gitmanager-service.exe"] {
                let _ = hidden_command("taskkill.exe")
                    .args(["/FI", &filter, "/IM", image_name, "/T", "/F"])
                    .output();
            }
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

fn service_startup_diagnostics(last_probe_error: Option<&str>) -> String {
    let mut details = Vec::new();
    if let Some(error) = last_probe_error {
        details.push(format!("Last authenticated health check: {error}"));
    }
    match read_service_log_tail() {
        Ok(log) if !log.trim().is_empty() => {
            let tail = log.lines().rev().take(14).collect::<Vec<_>>()
                .into_iter().rev().collect::<Vec<_>>().join("\n");
            details.push(format!("Recent gitmanager.service log:\n{tail}"));
        }
        Ok(_) => details.push("The core service log is empty.".to_string()),
        Err(error) => details.push(format!("Could not read core service log: {error}")),
    }
    details.join("\n\n")
}

/// Ensures the required repository/data engine is running before the UI becomes
/// functional. crash_helper normally starts and supervises the service first;
/// this command verifies that startup and provides a fallback for direct debug
/// launches where the companion supervisor is not present.
pub fn ensure_required_service(app_handle: AppHandle) -> Result<String, String> {
    // Always remove the legacy logon registration, even when a current core
    // service is already running, so the old worker cannot return next login.
    stop_legacy_background_service();

    // crash_helper owns the service lifecycle for a supervised UI child.
    // Starting another copy here races the helper's child and produces a
    // misleading exit-code-2 failure when the first process owns the lock.
    let supervisor_managed = std::env::var_os("GITMANAGER_CRASH_HELPER_ADDRESS").is_some()
        || std::env::var_os("GITMANAGER_SERVICE_SUPERVISOR").is_some();
    let startup_wait = if supervisor_managed {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(3)
    };
    let initial_deadline = Instant::now() + startup_wait;
    let mut last_probe_error = None;
    while Instant::now() < initial_deadline {
        match crash_helper::request_service_status() {
            Ok(status)
                if status.service_generation >= crash_helper::SERVICE_GENERATION
                    && status.protocol_version >= crash_helper::SERVICE_PROTOCOL_VERSION =>
            {
                return Ok(status.message);
            }
            Ok(status) if status.service_generation > 0 || status.protocol_version > 0 => {
                last_probe_error = Some(format!(
                    "A service answered with generation {} and protocol {}; required {} and {}.",
                    status.service_generation,
                    status.protocol_version,
                    crash_helper::SERVICE_GENERATION,
                    crash_helper::SERVICE_PROTOCOL_VERSION,
                ));
                stop_outdated_core_service(status.process_id);
                if !supervisor_managed {
                    break;
                }
            }
            Ok(_) => {
                last_probe_error = Some(
                    "A legacy service answered, but it does not implement the required core-service generation."
                        .to_string(),
                );
                if supervisor_managed {
                    // The helper will retry after the legacy process is stopped.
                    thread::sleep(Duration::from_millis(250));
                } else {
                    break;
                }
            }
            Err(error) => last_probe_error = Some(error),
        }
        thread::sleep(Duration::from_millis(150));
    }

    if supervisor_managed {
        let ownership = if crash_helper::service_control_lock_is_held() {
            let owner = crash_helper::service_control_lock_owner_pid()
                .map(|pid| format!("PID {pid}"))
                .unwrap_or_else(|| "PID unavailable (service predates owner metadata)".to_string());
            format!(
                "The exclusive service lock is held by {owner}, but its authenticated status pipe is not completing responses. This indicates an existing service process, not a missing executable. crash_helper will not start a duplicate."
            )
        } else {
            "No process currently owns the service lock; crash_helper can retry service startup.".to_string()
        };
        let error = format!(
            "GitManager core service did not become IPC-ready within {} seconds.\n\n{ownership}\n\n{}\n\nThe UI will not launch a duplicate service process.",
            startup_wait.as_secs(),
            service_startup_diagnostics(last_probe_error.as_deref()),
        );
        crash_helper::report_service_start_failure(&error);
        return Err(error);
    }

    // Only direct development/debug launches without the supervisor may own
    // the fallback service process themselves.
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
            "Could not start required GitManager core service at {}: {error}\n\n{}",
            service_exe.display(),
            service_startup_diagnostics(last_probe_error.as_deref()),
        )
    })?;

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match crash_helper::request_service_status() {
            Ok(status)
                if status.service_generation >= crash_helper::SERVICE_GENERATION
                    && status.protocol_version >= crash_helper::SERVICE_PROTOCOL_VERSION =>
            {
                return Ok(status.message);
            }
            Ok(status) => {
                last_probe_error = Some(format!(
                    "Service answered with generation {} and protocol {}; required {} and {}.",
                    status.service_generation,
                    status.protocol_version,
                    crash_helper::SERVICE_GENERATION,
                    crash_helper::SERVICE_PROTOCOL_VERSION,
                ));
            }
            Err(error) => last_probe_error = Some(error),
        }

        if let Ok(Some(exit)) = child.try_wait() {
            // Exit code 2 can mean the helper-started service already owns the
            // single-instance lock. Probe briefly for that process before
            // treating this fallback child as the failed service.
            let duplicate_deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < duplicate_deadline {
                if let Ok(status) = crash_helper::request_service_status() {
                    if status.service_generation >= crash_helper::SERVICE_GENERATION
                        && status.protocol_version >= crash_helper::SERVICE_PROTOCOL_VERSION
                    {
                        return Ok(status.message);
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }

            let error = format!(
                "GitManager core service process exited before becoming ready ({exit}).\n\n{}\n\nOpen Task Center → Service diagnostics for the full incident record.",
                service_startup_diagnostics(last_probe_error.as_deref()),
            );
            crash_helper::report_service_start_failure(&error);
            return Err(error);
        }

        if Instant::now() >= deadline {
            let error = format!(
                "GitManager core service did not become ready within 20 seconds.\n\n{}\n\nThe UI will keep its existing repository index and allow a service retry.",
                service_startup_diagnostics(last_probe_error.as_deref()),
            );
            crash_helper::report_service_start_failure(&error);
            return Err(error);
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
		const TASK_NAME: &str = "GitManager Core Service";
		const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
		const RUN_VALUE: &str = "GitManagerCoreService";

		// Clean up the legacy PR #9 Windows SCM service name if present.
		let _ = hidden_command("sc.exe").args(["stop", "GitManagerUpdateSvc"]).output();
		let _ = hidden_command("sc.exe").args(["delete", "GitManagerUpdateSvc"]).output();

		// Build a Task Scheduler 1.2 XML definition (compatible with Windows 7, 8,
		// 10, and 11) that disables the default 72-hour execution timeout and
		// battery-kill settings while XML-escaping paths like `kastrick's_software`.
		let escaped_exe = xml_escape(&service_exe.to_string_lossy());
		let working_dir = service_exe
			.parent()
			.map(|parent| xml_escape(&parent.to_string_lossy()))
			.unwrap_or_default();
		let task_xml = format!(
			r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>GitManager per-user background repository, watcher, and archive service.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{escaped_exe}</Command>
      <WorkingDirectory>{working_dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
		);

		let xml_path = std::env::temp_dir().join(format!(
			"gitmanager-core-task-{}.xml",
			std::process::id()
		));
		// Task Scheduler XML imports reliably across Windows 7–11 when UTF-16LE BOM-encoded.
		let mut utf16_bytes = Vec::with_capacity(2 + task_xml.len() * 2);
		utf16_bytes.extend_from_slice(&[0xFF, 0xFE]);
		for unit in task_xml.encode_utf16() {
			utf16_bytes.extend_from_slice(&unit.to_le_bytes());
		}
		let task_created = if fs::write(&xml_path, &utf16_bytes).is_ok() {
			let output = hidden_command("schtasks.exe")
				.args(["/Create", "/TN", TASK_NAME, "/XML"])
				.arg(&xml_path)
				.arg("/F")
				.output();
			let _ = fs::remove_file(&xml_path);
			output.map(|result| result.status.success()).unwrap_or(false)
		} else {
			false
		};

		let service_already_running = crash_helper::service_control_lock_is_held();

		if task_created {
			// Remove any older HKCU\Run fallback entry so the service is not launched twice.
			let _ = hidden_command("reg.exe")
				.args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])
				.output();
			if !service_already_running {
				let _ = hidden_command("schtasks.exe")
					.args(["/Run", "/TN", TASK_NAME])
					.output();
			}
			return Ok(format!(
				"Registered GitManager Core Service via Windows Task Scheduler (unlimited runtime, battery-safe) at {}.",
				service_exe.display()
			));
		}

		// Standard (non-admin) Windows 7–11 accounts often cannot create ONLOGON
		// tasks via schtasks.exe. Fall back cleanly to HKCU\...\Run so sign-in
		// startup works on every Windows PC without requiring UAC elevation.
		let run_value_data = format!("\"{}\"", service_exe.display());
		let reg_add = hidden_command("reg.exe")
			.args([
				"add",
				RUN_KEY,
				"/v",
				RUN_VALUE,
				"/t",
				"REG_SZ",
				"/d",
				&run_value_data,
				"/f",
			])
			.output()
			.map_err(|error| format!("Could not invoke Windows registry utility: {error}"))?;
		if !reg_add.status.success() {
			let detail = String::from_utf8_lossy(&reg_add.stderr);
			let detail = if detail.trim().is_empty() {
				String::from_utf8_lossy(&reg_add.stdout).into_owned()
			} else {
				detail.into_owned()
			};
			return Err(format!(
				"Could not register GitManager Core Service for Windows sign-in: {}",
				detail.trim()
			));
		}

		return Ok(format!(
			"Registered GitManager Core Service via Windows User Startup (HKCU Run) at {}.",
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


/// Queries the actual OS service/startup manager rather than relying on an
/// in-memory boolean. Returns `(registered, human_readable_kind)`.
pub fn is_os_service_registered() -> (bool, String) {
	#[cfg(target_os = "windows")]
	{
		if hidden_command("sc.exe")
			.args(["query", "GitManagerCoreService"])
			.output()
			.map(|out| out.status.success())
			.unwrap_or(false)
		{
			return (true, "Windows Service (SCM)".to_string());
		}
		if hidden_command("schtasks.exe")
			.args(["/Query", "/TN", "GitManager Core Service"])
			.output()
			.map(|out| out.status.success())
			.unwrap_or(false)
		{
			return (true, "Windows Task Scheduler (user logon)".to_string());
		}
		if hidden_command("reg.exe")
			.args([
				"query",
				r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
				"/v",
				"GitManagerCoreService",
			])
			.output()
			.map(|out| out.status.success())
			.unwrap_or(false)
		{
			return (true, "Windows User Startup (HKCU Run)".to_string());
		}
		return (false, "Windows Service / User Startup".to_string());
	}

	#[cfg(target_os = "linux")]
	{
		let enabled = hidden_command("systemctl")
			.args(["--user", "is-enabled", "gitmanager-core.service"])
			.output()
			.map(|out| out.status.success())
			.unwrap_or(false);
		return (enabled, "Linux systemd User Service".to_string());
	}

	#[cfg(target_os = "macos")]
	{
		let plist_exists = dirs::home_dir()
			.map(|home| home.join("Library/LaunchAgents/com.gitmanager.core.plist").is_file())
			.unwrap_or(false);
		return (plist_exists, "macOS LaunchAgent (launchd)".to_string());
	}

	#[allow(unreachable_code)]
	(false, "OS Background Service".to_string())
}

/// Cleanly unregisters GitManager's OS sign-in service on Windows, Linux, or macOS.
pub fn unregister_os_background_service() -> Result<String, String> {
	#[cfg(target_os = "windows")]
	{
		let _ = hidden_command("schtasks.exe")
			.args(["/End", "/TN", "GitManager Core Service"])
			.output();
		let _ = hidden_command("schtasks.exe")
			.args(["/Delete", "/TN", "GitManager Core Service", "/F"])
			.output();
		let _ = hidden_command("reg.exe")
			.args([
				"delete",
				r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
				"/v",
				"GitManagerCoreService",
				"/f",
			])
			.output();
		let _ = hidden_command("sc.exe")
			.args(["stop", "GitManagerCoreService"])
			.output();
		let _ = hidden_command("sc.exe")
			.args(["delete", "GitManagerCoreService"])
			.output();
		return Ok("Removed GitManager Core Service Windows startup registration.".to_string());
	}

	#[cfg(target_os = "linux")]
	{
		let _ = hidden_command("systemctl")
			.args(["--user", "disable", "--now", "gitmanager-core.service"])
			.output();
		if let Some(home) = dirs::home_dir() {
			let unit = home.join(".config/systemd/user/gitmanager-core.service");
			let _ = fs::remove_file(unit);
		}
		let _ = hidden_command("systemctl")
			.args(["--user", "daemon-reload"])
			.output();
		return Ok("Removed GitManager Linux systemd user service registration.".to_string());
	}

	#[cfg(target_os = "macos")]
	{
		if let Ok(uid_out) = hidden_command("id").arg("-u").output() {
			let uid = String::from_utf8_lossy(&uid_out.stdout).trim().to_string();
			if !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()) {
				let target = format!("gui/{uid}/com.gitmanager.core");
				let _ = hidden_command("launchctl").args(["bootout", &target]).output();
				let _ = hidden_command("launchctl").args(["disable", &target]).output();
			}
		}
		if let Some(home) = dirs::home_dir() {
			let plist = home.join("Library/LaunchAgents/com.gitmanager.core.plist");
			let _ = fs::remove_file(plist);
		}
		return Ok("Removed GitManager macOS LaunchAgent registration.".to_string());
	}

	#[allow(unreachable_code)]
	Err("Background service unregistration is unavailable on this platform.".to_string())
}
