//! Cross-platform GitManager UI supervision and background-service control.
//!
//! crash_helper supervises the desktop UI only. The independent
//! git-manager-service process is controlled over a loopback-only authenticated
//! endpoint and is never placed in the UI's process tree or termination scope.

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const DATA_ROOT: &str = "kastrick's_software";
const ENDPOINT_FILE: &str = "service-control.json";
const LOCK_FILE: &str = "service-control.lock";
const DEFAULT_SUSPENSION_SECONDS: u64 = 120;
const MIN_SUSPENSION_SECONDS: u64 = 15;
const MAX_SUSPENSION_SECONDS: u64 = 600;
const UI_STARTUP_GRACE: Duration = Duration::from_secs(90);
const UI_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
const UI_RECOVERY_GRACE: Duration = Duration::from_secs(15);
const SUPERVISOR_POLL: Duration = Duration::from_millis(400);
const MAX_MESSAGE_BYTES: u64 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Endpoint {
    version: u32,
    address: String,
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum ServiceCommand {
    Status,
    Suspend { seconds: u64 },
    Resume,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceRequest {
    token: String,
    command: ServiceCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceStatus {
    pub suspended: bool,
    pub remaining_seconds: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceResponse {
    ok: bool,
    status: ServiceStatus,
    error: Option<String>,
}

struct ServiceState {
    suspended_until: Mutex<Option<SystemTime>>,
}

impl ServiceState {
    fn status(&self, message: impl Into<String>) -> ServiceStatus {
        let now = SystemTime::now();
        let mut deadline = self.suspended_until.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if deadline.is_some_and(|until| until <= now) {
            *deadline = None;
        }

        let remaining = deadline
            .and_then(|until| until.duration_since(now).ok())
            .map(|duration| duration.as_secs().max(1))
            .unwrap_or(0);

        ServiceStatus {
            suspended: remaining > 0,
            remaining_seconds: remaining,
            message: message.into(),
        }
    }

    fn suspend(&self, seconds: u64) -> ServiceStatus {
        let seconds = seconds.clamp(MIN_SUSPENSION_SECONDS, MAX_SUSPENSION_SECONDS);
        *self.suspended_until.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(SystemTime::now() + Duration::from_secs(seconds));
        self.status(format!("Background work suspended for {seconds} seconds"))
    }

    fn resume(&self) -> ServiceStatus {
        *self.suspended_until.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        self.status("Background service active")
    }

    fn is_suspended(&self) -> bool {
        self.status("status").suspended
    }
}

/// Owns the service-control listener and single-instance lock for the service lifetime.
pub struct ServiceControlServer {
    state: Arc<ServiceState>,
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    endpoint_path: PathBuf,
    _lock_file: File,
}

impl ServiceControlServer {
    /// Suspension is cooperative: callers stop scheduling work while this is true.
    /// The deadline is checked on every call, so it expires without a UI reconnecting.
    pub fn is_suspended(&self) -> bool {
        self.state.is_suspended()
    }

    pub fn status(&self) -> ServiceStatus {
        self.state.status("Background service status")
    }
}

impl Drop for ServiceControlServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.endpoint_path);
    }
}

/// Starts a loopback-only control endpoint protected by a random per-process token.
pub fn start_service_control_server() -> Result<ServiceControlServer, String> {
    let data_dir = gitmanager_data_dir()?;
    fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Could not create service state directory: {error}"))?;

    let lock_path = data_dir.join(LOCK_FILE);
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| format!("Could not open service ownership lock: {error}"))?;
    lock_file.try_lock_exclusive()
        .map_err(|_| "Another GitManager background service instance already owns the service lock.".to_string())?;

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("Could not bind the local service-control endpoint: {error}"))?;
    listener.set_nonblocking(true)
        .map_err(|error| format!("Could not configure the service-control listener: {error}"))?;

    let endpoint_path = data_dir.join(ENDPOINT_FILE);
    let endpoint = Endpoint {
        version: 1,
        address: listener.local_addr().map_err(|error| error.to_string())?.to_string(),
        token: Uuid::new_v4().to_string(),
    };
    write_private_json(&endpoint_path, &endpoint)
        .map_err(|error| format!("Could not publish service-control endpoint: {error}"))?;

    let state = Arc::new(ServiceState { suspended_until: Mutex::new(None) });
    let running = Arc::new(AtomicBool::new(true));
    let thread_state = Arc::clone(&state);
    let thread_running = Arc::clone(&running);
    let token = endpoint.token.clone();

    let worker = thread::Builder::new()
        .name("gitmanager-service-control".to_string())
        .spawn(move || {
            while thread_running.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => handle_service_connection(stream, &token, &thread_state),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(250)),
                }
            }
        })
        .map_err(|error| format!("Could not start service-control worker: {error}"))?;

    Ok(ServiceControlServer {
        state,
        running,
        worker: Some(worker),
        endpoint_path,
        _lock_file: lock_file,
    })
}

fn handle_service_connection(mut stream: TcpStream, token: &str, state: &ServiceState) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let mut bytes = Vec::new();
    let read_result = (&mut stream).take(MAX_MESSAGE_BYTES).read_to_end(&mut bytes);
    let response = if read_result.is_err() || bytes.len() as u64 >= MAX_MESSAGE_BYTES {
        ServiceResponse {
            ok: false,
            status: state.status("Invalid service-control request"),
            error: Some("Invalid or oversized request".to_string()),
        }
    } else {
        match serde_json::from_slice::<ServiceRequest>(&bytes) {
            Ok(request) if request.token == token => match request.command {
                ServiceCommand::Status => ServiceResponse {
                    ok: true,
                    status: state.status("Background service status"),
                    error: None,
                },
                ServiceCommand::Suspend { seconds } => ServiceResponse {
                    ok: true,
                    status: state.suspend(seconds),
                    error: None,
                },
                ServiceCommand::Resume => ServiceResponse {
                    ok: true,
                    status: state.resume(),
                    error: None,
                },
            },
            Ok(_) => ServiceResponse {
                ok: false,
                status: state.status("Unauthorized service-control request"),
                error: Some("Authentication failed".to_string()),
            },
            Err(error) => ServiceResponse {
                ok: false,
                status: state.status("Invalid service-control request"),
                error: Some(format!("Invalid request: {error}")),
            },
        }
    };

    if let Ok(encoded) = serde_json::to_vec(&response) {
        let _ = stream.write_all(&encoded);
        let _ = stream.flush();
    }
}

fn send_service_command(command: ServiceCommand) -> Result<ServiceStatus, String> {
    let endpoint_path = gitmanager_data_dir()?.join(ENDPOINT_FILE);
    let content = fs::read(&endpoint_path)
        .map_err(|error| format!("GitManager background service is not reachable: {error}"))?;
    if content.len() > 4096 {
        return Err("Service-control endpoint file is invalid.".to_string());
    }

    let endpoint: Endpoint = serde_json::from_slice(&content)
        .map_err(|error| format!("Service-control endpoint is invalid: {error}"))?;
    if endpoint.version != 1 {
        return Err("Service-control protocol version is unsupported.".to_string());
    }

    let address: SocketAddr = endpoint.address.parse()
        .map_err(|_| "Service-control address is invalid.".to_string())?;
    if !address.ip().is_loopback() {
        return Err("Service-control endpoint is not a loopback address.".to_string());
    }

    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("Could not connect to GitManager background service: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok();

    let request = ServiceRequest { token: endpoint.token, command };
    let encoded = serde_json::to_vec(&request)
        .map_err(|error| format!("Could not encode service-control request: {error}"))?;
    stream.write_all(&encoded)
        .map_err(|error| format!("Could not send service-control request: {error}"))?;
    stream.shutdown(std::net::Shutdown::Write).ok();

    let mut response_bytes = Vec::new();
    stream.take(MAX_MESSAGE_BYTES).read_to_end(&mut response_bytes)
        .map_err(|error| format!("Could not read service-control response: {error}"))?;
    let response: ServiceResponse = serde_json::from_slice(&response_bytes)
        .map_err(|error| format!("Service-control response was invalid: {error}"))?;
    if !response.ok {
        return Err(response.error.unwrap_or_else(|| "Service-control request was rejected.".to_string()));
    }
    Ok(response.status)
}

pub fn request_service_status() -> Result<ServiceStatus, String> {
    send_service_command(ServiceCommand::Status)
}

pub fn request_service_suspend(seconds: u64) -> Result<ServiceStatus, String> {
    send_service_command(ServiceCommand::Suspend { seconds })
}

pub fn request_service_resume() -> Result<ServiceStatus, String> {
    send_service_command(ServiceCommand::Resume)
}

#[derive(Debug, Serialize, Deserialize)]
struct UiMessage {
    token: String,
    kind: String,
}

/// Called periodically by the Tauri frontend after it has mounted.
pub fn report_ui_heartbeat_from_env() -> Result<(), String> {
    send_ui_message_from_env("heartbeat")
}

/// Called when the Tauri event loop exits normally.
pub fn report_ui_shutdown_from_env() {
    let _ = send_ui_message_from_env("shutdown");
}

fn send_ui_message_from_env(kind: &str) -> Result<(), String> {
    let address = match std::env::var("GITMANAGER_CRASH_HELPER_ADDRESS") {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    let token = std::env::var("GITMANAGER_CRASH_HELPER_TOKEN")
        .map_err(|_| "Crash-helper session token is missing.".to_string())?;
    let address: SocketAddr = address.parse()
        .map_err(|_| "Crash-helper session address is invalid.".to_string())?;
    if !address.ip().is_loopback() {
        return Err("Crash-helper endpoint is not a loopback address.".to_string());
    }

    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))
        .map_err(|error| format!("Crash helper is not reachable: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok();
    let message = UiMessage { token, kind: kind.to_string() };
    let encoded = serde_json::to_vec(&message)
        .map_err(|error| format!("Could not encode UI heartbeat: {error}"))?;
    stream.write_all(&encoded)
        .map_err(|error| format!("Could not send UI heartbeat: {error}"))?;
    stream.shutdown(std::net::Shutdown::Write).ok();
    Ok(())
}

pub fn launch_supervisor(current_exe: &Path, args: &[std::ffi::OsString]) -> Result<bool, String> {
    if args.iter().any(|argument| argument.to_string_lossy() == "--crash-helper-child") {
        return Ok(false);
    }
    let helper = match find_crash_helper(current_exe) {
        Some(path) => path,
        None => return Ok(false),
    };

    let mut command = Command::new(helper);
    command.arg("--supervise").arg(current_exe).arg("--").args(args);
    configure_detached_command(&mut command);
    command.spawn().map_err(|error| format!("Could not start crash_helper: {error}"))?;
    Ok(true)
}

fn find_crash_helper(current_exe: &Path) -> Option<PathBuf> {
    let executable_dir = current_exe.parent()?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let triple = host_target_triple();

    let mut directories = vec![
        executable_dir.to_path_buf(),
        executable_dir.join("resources"),
        executable_dir.join("Resources"),
        executable_dir.join("../Resources"),
        executable_dir.join("../resources"),
    ];

    // Linux packages may place Tauri resources outside the executable's
    // directory. Match the productName used by tauri.conf.json, while also
    // accepting normalized package-directory variants from distro tooling.
    #[cfg(target_os = "linux")]
    {
        const PRODUCT_DIRS: &[&str] = &["GitManager", "gitmanager", "git-manager"];

        if let Some(prefix) = executable_dir.parent() {
            for product in PRODUCT_DIRS {
                directories.push(prefix.join("lib").join(product));
            }
        }

        if let Some(app_dir) = std::env::var_os("APPDIR") {
            let app_resources = PathBuf::from(app_dir).join("usr").join("lib");
            for product in PRODUCT_DIRS {
                directories.push(app_resources.join(product));
            }
        }

        for product in PRODUCT_DIRS {
            directories.push(PathBuf::from("/usr/lib").join(product));
        }
    }

    let names = [
        format!("crash_helper{suffix}"),
        format!("crash_helper-{triple}{suffix}"),
    ];

    for directory in directories {
        for name in &names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if let Ok(entries) = fs::read_dir(&directory) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("crash_helper-") && name.ends_with(suffix) && entry.path().is_file() {
                    return Some(entry.path());
                }
            }
        }
    }
    None
}

fn host_target_triple() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => "unknown",
    }
}

fn configure_detached_command(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

/// Terminate only the UI process tree. The background service is not a child
/// of this process group and is never included in this operation.
fn terminate_ui_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let pid_text = pid.to_string();
        let _ = Command::new("taskkill.exe")
            .args(["/PID", pid_text.as_str(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(unix)]
    {
        let group = -(pid as libc::pid_t);
        unsafe {
            let _ = libc::kill(group, libc::SIGTERM);
        }
        thread::sleep(Duration::from_millis(750));
        unsafe {
            let _ = libc::kill(group, libc::SIGKILL);
        }
    }
}

fn gitmanager_data_dir() -> Result<PathBuf, String> {
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the user's local application-data directory.".to_string())?;
    Ok(local_data.join(DATA_ROOT).join("data").join("GitManager"))
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let mut file = OpenOptions::new().create_new(true).write(true).open(&temporary)?;
    serde_json::to_writer(&mut file, value).map_err(std::io::Error::other)?;
    file.sync_all()?;
    set_private_permissions(&temporary)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

fn set_private_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn timestamp_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

fn append_incident(reason: &str, pid: u32, exit_code: Option<i32>, service_result: Result<ServiceStatus, String>) {
    let id = Uuid::new_v4().to_string();
    let service = match service_result {
        Ok(status) => serde_json::json!({
            "suspended": status.suspended,
            "remainingSeconds": status.remaining_seconds,
            "message": status.message
        }),
        Err(error) => serde_json::json!({ "error": error }),
    };
    let incident = serde_json::json!({
        "incidentId": id,
        "timestampUnixMs": timestamp_ms(),
        "process": { "pid": pid, "executable": "git-manager" },
        "reason": reason,
        "exitCode": exit_code.map(|code| format!("0x{:08X}", code as u32)),
        "service": service
    });

    if let Ok(data_dir) = gitmanager_data_dir() {
        let log_dir = data_dir.join("log");
        if fs::create_dir_all(&log_dir).is_ok() {
            let path = log_dir.join("crash_helper.jsonl");
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
                let _ = writeln!(file, "{incident}");
                let _ = file.flush();
                let _ = set_private_permissions(&path);
            }
            let incidents = log_dir.join("incidents");
            if fs::create_dir_all(&incidents).is_ok() {
                let path = incidents.join(format!("incident-{id}.json"));
                if let Ok(mut file) = File::create(&path) {
                    let _ = serde_json::to_writer_pretty(&mut file, &incident);
                    let _ = file.flush();
                    let _ = set_private_permissions(&path);
                }
            }
        }
    }
}

fn read_ui_message(mut stream: TcpStream) -> Option<UiMessage> {
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    let mut bytes = Vec::new();
    stream.take(MAX_MESSAGE_BYTES).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 >= MAX_MESSAGE_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn run_supervisor(arguments: Vec<std::ffi::OsString>) -> i32 {
    if arguments.len() < 2 {
        return 2;
    }
    let application = PathBuf::from(&arguments[0]);
    let separator = arguments.iter().position(|argument| argument.to_string_lossy() == "--");
    let app_args = separator
        .map(|index| arguments.iter().skip(index + 1).cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            append_incident(
                &format!("Could not open UI heartbeat listener: {error}"),
                0, None, Err("Service suspension not attempted".to_string())
            );
            return 2;
        }
    };
    if listener.set_nonblocking(true).is_err() {
        return 2;
    }
    let address = match listener.local_addr() {
        Ok(address) => address.to_string(),
        Err(_) => return 2,
    };
    let token = Uuid::new_v4().to_string();

    let mut child_command = Command::new(&application);
    child_command
        .args(app_args)
        .arg("--crash-helper-child")
        .env("GITMANAGER_CRASH_HELPER_ADDRESS", &address)
        .env("GITMANAGER_CRASH_HELPER_TOKEN", &token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached_command(&mut child_command);

    let mut child: Child = match child_command.spawn() {
        Ok(child) => child,
        Err(error) => {
            append_incident(
                &format!("Could not start GitManager UI: {error}"),
                0, None, Err("Service suspension not attempted".to_string())
            );
            return 1;
        }
    };

    let pid = child.id();
    let started = Instant::now();
    let mut last_heartbeat: Option<Instant> = None;
    let mut stale_since: Option<Instant> = None;
    let mut suspended = false;
    let mut clean_shutdown = false;

    loop {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Some(message) = read_ui_message(stream) {
                        if message.token == token {
                            match message.kind.as_str() {
                                "heartbeat" => {
                                    let first_heartbeat = last_heartbeat.is_none();
                                    last_heartbeat = Some(Instant::now());
                                    stale_since = None;
                                    if suspended || first_heartbeat {
                                        let reason = if suspended {
                                            "UI heartbeat recovered; service resumed"
                                        } else {
                                            "UI became responsive; service activated"
                                        };
                                        append_incident(reason, pid, None, request_service_resume());
                                        suspended = false;
                                    }
                                }
                                "shutdown" => {
                                    clean_shutdown = true;
                                    stale_since = None;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    terminate_ui_tree(pid);
                    append_incident(
                        "GitManager UI exited unexpectedly",
                        pid,
                        status.code(),
                        request_service_suspend(DEFAULT_SUSPENSION_SECONDS)
                    );
                }
                return 0;
            }
            Ok(None) => {}
            Err(error) => {
                let service_result = request_service_suspend(DEFAULT_SUSPENSION_SECONDS);
                append_incident(&format!("Could not query GitManager UI process: {error}"), pid, None, service_result);
                terminate_ui_tree(pid);
                let _ = child.kill();
                let _ = child.wait();
                return 1;
            }
        }

        let stale = !clean_shutdown && match last_heartbeat {
            Some(last) => last.elapsed() > UI_HEARTBEAT_TIMEOUT,
            None => started.elapsed() > UI_STARTUP_GRACE,
        };
        if stale {
            if stale_since.is_none() {
                append_incident(
                    if last_heartbeat.is_some() {
                        "GitManager UI heartbeat stopped; service temporarily suspended"
                    } else {
                        "GitManager UI did not become responsive during startup; service temporarily suspended"
                    },
                    pid,
                    None,
                    request_service_suspend(DEFAULT_SUSPENSION_SECONDS)
                );
                suspended = true;
                stale_since = Some(Instant::now());
            } else if stale_since.is_some_and(|since| since.elapsed() >= UI_RECOVERY_GRACE) {
                terminate_ui_tree(pid);
                let _ = child.kill();
                let status = child.wait().ok();
                append_incident(
                    "GitManager UI remained unresponsive and was terminated by crash_helper",
                    pid,
                    status.and_then(|value| value.code()),
                    request_service_suspend(DEFAULT_SUSPENSION_SECONDS)
                );
                return 0;
            }
        }
        thread::sleep(SUPERVISOR_POLL);
    }
}

pub fn run_helper_from_args() -> i32 {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.first().is_some_and(|argument| argument.to_string_lossy() == "--supervise") {
        run_supervisor(arguments.into_iter().skip(1).collect())
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ServiceState, DEFAULT_SUSPENSION_SECONDS, MAX_SUSPENSION_SECONDS,
        MIN_SUSPENSION_SECONDS,
    };
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    #[test]
    fn suspension_window_is_bounded() {
        assert_eq!(DEFAULT_SUSPENSION_SECONDS, 120);
        assert!(MIN_SUSPENSION_SECONDS >= 10);
        assert!(MAX_SUSPENSION_SECONDS <= 600);
        assert!(DEFAULT_SUSPENSION_SECONDS <= MAX_SUSPENSION_SECONDS);
    }

    #[test]
    fn service_suspension_can_be_resumed_and_expires() {
        let state = ServiceState { suspended_until: Mutex::new(None) };
        assert!(!state.is_suspended());

        let status = state.suspend(0);
        assert!(status.suspended);
        assert!(status.remaining_seconds <= MIN_SUSPENSION_SECONDS);
        assert!(status.remaining_seconds > 0);

        assert!(!state.resume().suspended);

        *state.suspended_until.lock().unwrap() =
            Some(SystemTime::now() - Duration::from_secs(1));
        assert!(!state.is_suspended());
    }

    #[test]
    fn suspension_requests_are_clamped_to_the_maximum() {
        let state = ServiceState { suspended_until: Mutex::new(None) };
        let status = state.suspend(u64::MAX);
        assert!(status.suspended);
        assert!(status.remaining_seconds <= MAX_SUSPENSION_SECONDS);
        assert!(status.remaining_seconds >= MAX_SUSPENSION_SECONDS - 1);
    }
}
