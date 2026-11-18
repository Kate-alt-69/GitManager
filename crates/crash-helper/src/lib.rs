//! Cross-platform GitManager UI supervision and background-service control.
//!
//! crash_helper supervises the desktop UI only. The independent
//! gitmanager.service process is controlled over a loopback-only authenticated
//! endpoint and is never placed in the UI's process tree or termination scope.

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[cfg(windows)]
mod windows_process;

#[cfg(windows)]
type ProcessContainment = windows_process::ProcessContainment;

#[cfg(not(windows))]
struct ProcessContainment;

const DATA_ROOT: &str = "kastrick's_software";
const ENDPOINT_FILE: &str = "service-control.json";
const SUSPENSION_FILE: &str = "service-suspension.json";
const LOCK_FILE: &str = "service-control.lock";
const DEFAULT_SUSPENSION_SECONDS: u64 = 120;
const MIN_SUSPENSION_SECONDS: u64 = 15;
const MAX_SUSPENSION_SECONDS: u64 = 600;
const UI_STARTUP_GRACE: Duration = Duration::from_secs(90);
const UI_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
const UI_RECOVERY_GRACE: Duration = Duration::from_secs(15);
const SUPERVISOR_POLL: Duration = Duration::from_millis(400);
const MAX_MESSAGE_BYTES: u64 = 8 * 1024 * 1024;
const SERVICE_PROTOCOL_VERSION: u32 = 2;
const MAX_CONCURRENT_SERVICE_REQUESTS: usize = 8;
pub const SERVICE_GENERATION: u32 = 3;

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
    RequestScan,
    RequestRemoteCheck,
    LoadAccounts,
    SaveAccount { payload: serde_json::Value },
    InspectRepository { repo_path: String },
    LoadCommitHistory {
        repo_path: String,
        offset: usize,
        limit: usize,
        branch_filter: Option<String>,
        search_query: Option<String>,
    },
    GetCommitDetails { repo_path: String, oid: String },
    ListRepositoryFiles {
        repo_path: String,
        relative_path: Option<String>,
        source: String,
        branch: Option<String>,
    },
    ReadRepositoryFile {
        repo_path: String,
        relative_path: String,
        source: String,
        branch: Option<String>,
    },
    SaveConflictResolution { repo_path: String, relative_path: String, content: String },
    PreviewRepositoryPull { repo_path: String },
    GetRepositoryChanges { repo_path: String },
    ReadRepositoryFileDiff { repo_path: String, relative_path: String, staged: bool },
    FetchRepositoryRemote { repo_path: String, remote_name: String },
    StageRepositoryPaths { repo_path: String, paths: Vec<String> },
    UnstageRepositoryPaths { repo_path: String, paths: Vec<String> },
    PreviewRepositoryCommit { repo_path: String },
    CreateRepositoryCommit { repo_path: String, message: String, expected_index_tree: String },
    DeleteRepositoryPath { repo_path: String, relative_path: String },
    PrepareAndPullRepositoryUpdate {
        repo_path: String,
        expected_branch: String,
        expected_local_oid: String,
        expected_remote_oid: String,
        expected_status_token: String,
        mode: String,
        stage_paths: Vec<String>,
        commit_message: String,
    },
    ListLocalRepositoryBranches { repo_path: String },
    SwitchRepositoryBranch { repo_path: String, branch_name: String },
    ListNotifications,
    MarkNotificationRead { id: String },
    DismissNotification { id: String },
    MarkAllNotificationsRead,
    LoadAppPreferences,
    SaveAppPreferences { payload: serde_json::Value },
    GetRepositoryIndexGeneration,
    LoadIndexedRepositories { filter_mode: String },
    LoadRepositoryPreferences { repo_path: String, repo_id: Option<String> },
    SaveRepositoryPreferences { repo_path: String, payload: serde_json::Value },
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
    /// Missing in legacy workers; zero identifies the pre-core-service process.
    #[serde(default)]
    pub service_generation: u32,
    #[serde(default)]
    pub process_id: u32,
    #[serde(default)]
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceResponse {
    ok: bool,
    status: ServiceStatus,
    error: Option<String>,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SuspensionRecord {
    suspended_until_unix_ms: Option<u64>,
}

struct ServiceState {
    suspended_until: Mutex<Option<SystemTime>>,
    persistence_path: PathBuf,
}

fn read_suspension_record(path: &Path) -> Option<Option<SystemTime>> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > MAX_MESSAGE_BYTES as usize {
        return Some(None);
    }
    let record: SuspensionRecord = serde_json::from_slice(&bytes).ok()?;
    Some(record.suspended_until_unix_ms.and_then(|milliseconds| {
        UNIX_EPOCH.checked_add(Duration::from_millis(milliseconds))
    }))
}

fn deadline_to_unix_ms(until: SystemTime) -> std::io::Result<u64> {
    let milliseconds = until
        .duration_since(UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_millis();
    milliseconds
        .try_into()
        .map_err(|_| std::io::Error::other("Suspension deadline is out of range."))
}

impl ServiceState {
    fn load(persistence_path: PathBuf) -> Self {
        let now = SystemTime::now();
        let maximum_deadline = now + Duration::from_secs(MAX_SUSPENSION_SECONDS);
        let suspended_until = read_suspension_record(&persistence_path)
            .flatten()
            .filter(|deadline| *deadline > now)
            .map(|deadline| deadline.min(maximum_deadline));

        Self {
            suspended_until: Mutex::new(suspended_until),
            persistence_path,
        }
    }

    fn persist_locked(&self, deadline: Option<SystemTime>) -> std::io::Result<()> {
        let record = SuspensionRecord {
            suspended_until_unix_ms: deadline.map(deadline_to_unix_ms).transpose()?,
        };
        write_private_json(&self.persistence_path, &record)
    }

    fn status(&self, message: impl Into<String>) -> ServiceStatus {
        let now = SystemTime::now();
        let mut deadline = self.suspended_until.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // The helper can queue a suspend/resume request while this service is
        // starting or its IPC endpoint is briefly unavailable. Re-read the
        // tiny state file so a running service observes those external changes.
        if let Some(persisted_deadline) = read_suspension_record(&self.persistence_path) {
            *deadline = persisted_deadline
                .filter(|until| *until > now)
                .map(|until| until.min(now + Duration::from_secs(MAX_SUSPENSION_SECONDS)));
        }

        if deadline.as_ref().is_some_and(|until| *until <= now) {
            *deadline = None;
            let _ = self.persist_locked(None);
        }

        let remaining = deadline
            .as_ref()
            .and_then(|until| (*until).duration_since(now).ok())
            .map(|duration| duration.as_secs().max(1))
            .unwrap_or(0);

        ServiceStatus {
            suspended: remaining > 0,
            remaining_seconds: remaining,
            message: message.into(),
            service_generation: SERVICE_GENERATION,
            process_id: std::process::id(),
            protocol_version: SERVICE_PROTOCOL_VERSION,
        }
    }

    fn suspend(&self, seconds: u64) -> ServiceStatus {
        let seconds = seconds.clamp(MIN_SUSPENSION_SECONDS, MAX_SUSPENSION_SECONDS);
        let until = SystemTime::now() + Duration::from_secs(seconds);
        let mut deadline = self.suspended_until.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *deadline = Some(until);

        let message = match self.persist_locked(Some(until)) {
            Ok(()) => format!("Background work suspended for {seconds} seconds"),
            Err(error) => format!(
                "Background work suspended for {seconds} seconds, but its restart-recovery timer could not be saved: {error}"
            ),
        };
        drop(deadline);
        self.status(message)
    }

    fn resume(&self) -> ServiceStatus {
        let mut deadline = self.suspended_until.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *deadline = None;
        let message = match self.persist_locked(None) {
            Ok(()) => "Background service active".to_string(),
            Err(error) => format!(
                "Background service resumed, but its saved suspension state could not be cleared: {error}"
            ),
        };
        drop(deadline);
        self.status(message)
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
    scan_requested: Arc<AtomicBool>,
    remote_check_requested: Arc<AtomicBool>,
    _lock_file: File,
}

impl ServiceControlServer {
    /// Suspension is cooperative: callers stop scheduling work while this is true.
    /// The deadline is checked on every call, so it expires without a UI reconnecting.
    pub fn is_suspended(&self) -> bool {
        self.state.is_suspended()
    }

    pub fn status(&self) -> ServiceStatus {
        self.state.status("GitManager core service status")
    }

    /// Returns true once per queued repository-index refresh request.
    pub fn take_scan_request(&self) -> bool {
        self.scan_requested.swap(false, Ordering::AcqRel)
    }

    /// Allows the service loop to wake early without consuming a request.
    pub fn scan_request_pending(&self) -> bool {
        self.scan_requested.load(Ordering::Acquire)
    }

    /// Returns true once per queued immediate remote-update check.
    pub fn take_remote_check_request(&self) -> bool {
        self.remote_check_requested.swap(false, Ordering::AcqRel)
    }

    /// Allows the service loop to wake early when the UI requests a remote check.
    pub fn remote_check_pending(&self) -> bool {
        self.remote_check_requested.load(Ordering::Acquire)
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

fn reserve_service_request_slot(active: &AtomicUsize) -> bool {
    loop {
        let current = active.load(Ordering::Acquire);
        if current >= MAX_CONCURRENT_SERVICE_REQUESTS {
            return false;
        }
        if active.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ).is_ok() {
            return true;
        }
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
        version: SERVICE_PROTOCOL_VERSION,
        address: listener.local_addr().map_err(|error| error.to_string())?.to_string(),
        token: Uuid::new_v4().to_string(),
    };
    write_private_json(&endpoint_path, &endpoint)
        .map_err(|error| format!("Could not publish service-control endpoint: {error}"))?;

    let state = Arc::new(ServiceState::load(data_dir.join(SUSPENSION_FILE)));
    let running = Arc::new(AtomicBool::new(true));
    let scan_requested = Arc::new(AtomicBool::new(false));
    let remote_check_requested = Arc::new(AtomicBool::new(false));
    let active_requests = Arc::new(AtomicUsize::new(0));
    let thread_state = Arc::clone(&state);
    let thread_running = Arc::clone(&running);
    let thread_scan_requested = Arc::clone(&scan_requested);
    let thread_remote_check_requested = Arc::clone(&remote_check_requested);
    let thread_active_requests = Arc::clone(&active_requests);
    let token = endpoint.token.clone();

    let worker = thread::Builder::new()
        .name("gitmanager-service-control".to_string())
        .spawn(move || {
            while thread_running.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if !reserve_service_request_slot(&thread_active_requests) {
                            // Reject excess work instead of creating unbounded
                            // threads or blocking the accept loop.
                            drop(stream);
                            continue;
                        }

                        let request_token = token.clone();
                        let request_state = Arc::clone(&thread_state);
                        let request_scan = Arc::clone(&thread_scan_requested);
                        let request_remote_check = Arc::clone(&thread_remote_check_requested);
                        let request_counter = Arc::clone(&thread_active_requests);
                        if let Err(error) = thread::Builder::new()
                            .name("gitmanager-service-request".to_string())
                            .spawn(move || {
                                handle_service_connection(
                                    stream,
                                    &request_token,
                                    &request_state,
                                    &request_scan,
                                    &request_remote_check,
                                );
                                request_counter.fetch_sub(1, Ordering::Release);
                            })
                        {
                            thread_active_requests.fetch_sub(1, Ordering::Release);
                            eprintln!("GitManager service could not dispatch a control request: {error}");
                        }
                    }
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
        scan_requested,
        remote_check_requested,
        _lock_file: lock_file,
    })
}

#[cfg(feature = "account-storage")]
fn load_account_storage(state: &ServiceState) -> ServiceResponse {
    match gitmanager_core::account_store::load_accounts()
        .and_then(|accounts| serde_json::to_value(accounts)
            .map_err(|error| format!("Could not encode saved accounts: {error}")))
    {
        Ok(data) => ServiceResponse {
            ok: true,
            status: state.status("Account metadata loaded from the core service"),
            error: None,
            data: Some(data),
        },
        Err(error) => ServiceResponse {
            ok: false,
            status: state.status("Could not load account metadata"),
            error: Some(error),
            data: None,
        },
    }
}

#[cfg(not(feature = "account-storage"))]
fn load_account_storage(state: &ServiceState) -> ServiceResponse {
    ServiceResponse {
        ok: false,
        status: state.status("Account storage is not enabled in this process"),
        error: Some("Account persistence is only available through gitmanager.service.".to_string()),
        data: None,
    }
}

#[cfg(feature = "account-storage")]
fn save_account_storage(state: &ServiceState, payload: serde_json::Value) -> ServiceResponse {
    let result = (|| {
        let metadata_value = payload.get("metadata")
            .cloned()
            .ok_or_else(|| "Account save request is missing metadata.".to_string())?;
        let repositories_value = payload.get("repositories")
            .cloned()
            .ok_or_else(|| "Account save request is missing repository data.".to_string())?;
        let metadata: gitmanager_core::account_store::AccountMetadata =
            serde_json::from_value(metadata_value)
                .map_err(|error| format!("Account metadata payload is invalid: {error}"))?;
        let repositories: Vec<gitmanager_core::git_info::GitRepositoryInfo> =
            serde_json::from_value(repositories_value)
                .map_err(|error| format!("Account repository cache payload is invalid: {error}"))?;
        gitmanager_core::account_store::save_account(&metadata, &repositories)
    })();

    match result {
        Ok(()) => ServiceResponse {
            ok: true,
            status: state.status("Account metadata saved by the core service"),
            error: None,
            data: None,
        },
        Err(error) => ServiceResponse {
            ok: false,
            status: state.status("Could not save account metadata"),
            error: Some(error),
            data: None,
        },
    }
}

#[cfg(not(feature = "account-storage"))]
fn save_account_storage(state: &ServiceState, _payload: serde_json::Value) -> ServiceResponse {
    ServiceResponse {
        ok: false,
        status: state.status("Account storage is not enabled in this process"),
        error: Some("Account persistence is only available through gitmanager.service.".to_string()),
        data: None,
    }
}

#[cfg(feature = "repository-data")]
fn repository_data_response<T: Serialize>(
    state: &ServiceState,
    description: &str,
    result: Result<T, String>,
) -> ServiceResponse {
    let data = result.and_then(|value| serde_json::to_value(value)
        .map_err(|error| format!("Could not encode {description}: {error}")));
    match data {
        Ok(data) => ServiceResponse {
            ok: true,
            status: state.status(format!("{description} loaded by the core service")),
            error: None,
            data: Some(data),
        },
        Err(error) => ServiceResponse {
            ok: false,
            status: state.status(format!("Could not load {description}")),
            error: Some(error),
            data: None,
        },
    }
}

#[cfg(feature = "repository-data")]
fn load_repository_overview(state: &ServiceState, repo_path: String) -> ServiceResponse {
    let workdir = Path::new(&repo_path);
    let git_dir = workdir.join(".git");
    repository_data_response(
        state,
        "Repository overview",
        gitmanager_core::git_info::extract_git_repository_overview(workdir, &git_dir),
    )
}

#[cfg(feature = "repository-data")]
fn load_commit_history(
    state: &ServiceState,
    repo_path: String,
    offset: usize,
    limit: usize,
    branch_filter: Option<String>,
    search_query: Option<String>,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Commit history page",
        gitmanager_core::git_info::load_repository_commit_history(
            Path::new(&repo_path),
            offset,
            limit,
            branch_filter.as_deref(),
            search_query.as_deref(),
        ),
    )
}

#[cfg(feature = "repository-data")]
fn load_commit_details(
    state: &ServiceState,
    repo_path: String,
    oid: String,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Commit details",
        gitmanager_core::git_info::load_commit_details(Path::new(&repo_path), &oid),
    )
}

#[cfg(feature = "repository-data")]
fn list_repository_files(
    state: &ServiceState,
    repo_path: String,
    relative_path: Option<String>,
    source: String,
    branch: Option<String>,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Repository directory",
        gitmanager_core::repository_browser::list_repository_files(
            &repo_path,
            relative_path.as_deref(),
            &source,
            branch.as_deref(),
        ),
    )
}

#[cfg(feature = "repository-data")]
fn read_repository_file(
    state: &ServiceState,
    repo_path: String,
    relative_path: String,
    source: String,
    branch: Option<String>,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Repository file preview",
        gitmanager_core::repository_browser::read_repository_file(
            &repo_path,
            &relative_path,
            &source,
            branch.as_deref(),
        ),
    )
}

#[cfg(feature = "repository-data")]
fn get_repository_index_generation(state: &ServiceState) -> ServiceResponse {
    repository_data_response(
        state,
        "Repository index generation",
        gitmanager_core::repository_index::repository_index_generation()
            .map(|generation| generation.to_string()),
    )
}

#[cfg(feature = "repository-data")]
fn load_indexed_repositories(state: &ServiceState, filter_mode: String) -> ServiceResponse {
    let result = gitmanager_core::repository_index::load_persisted_repository_index()
        .map(|index| {
            let (repositories, truncated) = index
                .map(|index| (index.repositories, index.truncated))
                .unwrap_or_else(|| (Vec::new(), false));
            gitmanager_core::repository_index::build_indexed_summary_with_scan_status(
                repositories,
                &filter_mode,
                truncated,
            )
        });
    repository_data_response(state, "Indexed repository snapshot", result)
}

#[cfg(not(feature = "repository-data"))]
fn get_repository_index_generation(state: &ServiceState) -> ServiceResponse {
    service_data_unavailable(state)
}
#[cfg(not(feature = "repository-data"))]
fn load_indexed_repositories(state: &ServiceState, _: String) -> ServiceResponse {
    service_data_unavailable(state)
}

#[cfg(feature = "preferences-storage")]
fn load_app_preferences(state: &ServiceState) -> ServiceResponse {
    repository_data_response(
        state,
        "Application preferences",
        gitmanager_core::load_background_preferences(),
    )
}

#[cfg(feature = "preferences-storage")]
fn save_app_preferences(state: &ServiceState, payload: serde_json::Value) -> ServiceResponse {
    let result = serde_json::from_value::<gitmanager_core::AppPreferences>(payload)
        .map_err(|error| format!("Application preference payload is invalid: {error}"))
        .and_then(|preferences| {
            gitmanager_core::app_preferences::save_background_preferences(&preferences)?;
            Ok(preferences)
        });
    repository_data_response(state, "Saved application preferences", result)
}

#[cfg(not(feature = "preferences-storage"))]
fn preference_storage_unavailable(state: &ServiceState) -> ServiceResponse {
    ServiceResponse {
        ok: false,
        status: state.status("Application preference storage is not enabled in this process"),
        error: Some("Application preference storage is available only through gitmanager.service.".to_string()),
        data: None,
    }
}
#[cfg(not(feature = "preferences-storage"))]
fn load_app_preferences(state: &ServiceState) -> ServiceResponse { preference_storage_unavailable(state) }
#[cfg(not(feature = "preferences-storage"))]
fn save_app_preferences(state: &ServiceState, _: serde_json::Value) -> ServiceResponse { preference_storage_unavailable(state) }

#[cfg(feature = "notification-storage")]
fn list_notifications(state: &ServiceState) -> ServiceResponse {
    repository_data_response(
        state,
        "Notification history",
        gitmanager_core::notification_center::list_notifications(),
    )
}

#[cfg(feature = "notification-storage")]
fn mark_notification_read(state: &ServiceState, id: String) -> ServiceResponse {
    repository_data_response(
        state,
        "Notification read update",
        gitmanager_core::notification_center::mark_notification_read(&id),
    )
}

#[cfg(feature = "notification-storage")]
fn dismiss_notification(state: &ServiceState, id: String) -> ServiceResponse {
    repository_data_response(
        state,
        "Notification dismissal",
        gitmanager_core::notification_center::dismiss_notification(&id),
    )
}

#[cfg(feature = "notification-storage")]
fn mark_all_notifications_read(state: &ServiceState) -> ServiceResponse {
    repository_data_response(
        state,
        "Bulk notification read update",
        gitmanager_core::notification_center::mark_all_notifications_read(),
    )
}

#[cfg(not(feature = "notification-storage"))]
fn notification_storage_unavailable(state: &ServiceState) -> ServiceResponse {
    ServiceResponse {
        ok: false,
        status: state.status("Notification storage is not enabled in this process"),
        error: Some("Notification storage is available only through gitmanager.service.".to_string()),
        data: None,
    }
}
#[cfg(not(feature = "notification-storage"))]
fn list_notifications(state: &ServiceState) -> ServiceResponse { notification_storage_unavailable(state) }
#[cfg(not(feature = "notification-storage"))]
fn mark_notification_read(state: &ServiceState, _: String) -> ServiceResponse { notification_storage_unavailable(state) }
#[cfg(not(feature = "notification-storage"))]
fn dismiss_notification(state: &ServiceState, _: String) -> ServiceResponse { notification_storage_unavailable(state) }
#[cfg(not(feature = "notification-storage"))]
fn mark_all_notifications_read(state: &ServiceState) -> ServiceResponse { notification_storage_unavailable(state) }

#[cfg(feature = "repository-data")]
fn load_repository_preferences(
    state: &ServiceState,
    repo_path: String,
    repo_id: Option<String>,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Repository preferences",
        gitmanager_core::repository_preferences::load_repository_preferences(
            &repo_path,
            repo_id.as_deref(),
        ),
    )
}

#[cfg(feature = "repository-data")]
fn save_repository_preferences(
    state: &ServiceState,
    repo_path: String,
    payload: serde_json::Value,
) -> ServiceResponse {
    let result = serde_json::from_value::<gitmanager_core::RepositoryPreferences>(payload)
        .map_err(|error| format!("Repository preference payload is invalid: {error}"))
        .and_then(|settings| {
            gitmanager_core::repository_preferences::save_repository_preferences(&repo_path, settings)
        });
    repository_data_response(state, "Saved repository preferences", result)
}

#[cfg(not(feature = "repository-data"))]
fn load_repository_preferences(
    state: &ServiceState, _: String, _: Option<String>,
) -> ServiceResponse {
    service_workspace_unavailable(state)
}
#[cfg(not(feature = "repository-data"))]
fn save_repository_preferences(
    state: &ServiceState, _: String, _: serde_json::Value,
) -> ServiceResponse {
    service_workspace_unavailable(state)
}

#[cfg(feature = "repository-data")]
fn save_conflict_resolution(
    state: &ServiceState,
    repo_path: String,
    relative_path: String,
    content: String,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Conflict resolution",
        gitmanager_core::repository_workspace::save_conflict_resolution(
            &repo_path, &relative_path, &content,
        ),
    )
}

#[cfg(feature = "repository-data")]
fn preview_repository_pull(state: &ServiceState, repo_path: String) -> ServiceResponse {
    repository_data_response(
        state,
        "Pull preview",
        gitmanager_core::repository_workspace::preview_repository_pull(&repo_path),
    )
}

#[cfg(feature = "repository-data")]
fn get_repository_changes(state: &ServiceState, repo_path: String) -> ServiceResponse {
    repository_data_response(
        state,
        "Repository changes",
        gitmanager_core::repository_workspace::get_repository_changes(&repo_path),
    )
}

#[cfg(feature = "repository-data")]
fn read_repository_file_diff(
    state: &ServiceState,
    repo_path: String,
    relative_path: String,
    staged: bool,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Repository file diff",
        gitmanager_core::repository_workspace::read_repository_file_diff(
            &repo_path, &relative_path, staged,
        ),
    )
}

#[cfg(feature = "repository-data")]
fn fetch_repository_remote(
    state: &ServiceState,
    repo_path: String,
    remote_name: String,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Remote fetch",
        gitmanager_core::repository_workspace::fetch_repository_remote(&repo_path, &remote_name),
    )
}

#[cfg(feature = "repository-data")]
fn stage_repository_paths(
    state: &ServiceState,
    repo_path: String,
    paths: Vec<String>,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Staged repository changes",
        gitmanager_core::repository_workspace::stage_repository_paths(&repo_path, &paths),
    )
}

#[cfg(feature = "repository-data")]
fn unstage_repository_paths(
    state: &ServiceState,
    repo_path: String,
    paths: Vec<String>,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Unstaged repository changes",
        gitmanager_core::repository_workspace::unstage_repository_paths(&repo_path, &paths),
    )
}

#[cfg(feature = "repository-data")]
fn preview_repository_commit(state: &ServiceState, repo_path: String) -> ServiceResponse {
    repository_data_response(
        state,
        "Commit preview",
        gitmanager_core::repository_workspace::preview_repository_commit(&repo_path),
    )
}

#[cfg(feature = "repository-data")]
fn create_repository_commit(
    state: &ServiceState,
    repo_path: String,
    message: String,
    expected_index_tree: String,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Created commit",
        gitmanager_core::repository_workspace::create_repository_commit(
            &repo_path, &message, &expected_index_tree,
        ),
    )
}

#[cfg(feature = "repository-data")]
fn delete_repository_path(
    state: &ServiceState,
    repo_path: String,
    relative_path: String,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Repository deletion",
        gitmanager_core::repository_workspace::delete_repository_path(
            &repo_path, &relative_path,
        ),
    )
}

#[cfg(feature = "repository-data")]
fn prepare_and_pull_repository_update(
    state: &ServiceState,
    repo_path: String,
    expected_branch: String,
    expected_local_oid: String,
    expected_remote_oid: String,
    expected_status_token: String,
    mode: String,
    stage_paths: Vec<String>,
    commit_message: String,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Pull execution",
        gitmanager_core::repository_workspace::prepare_and_pull_repository_update(
            &repo_path,
            &expected_branch,
            &expected_local_oid,
            &expected_remote_oid,
            &expected_status_token,
            &mode,
            &stage_paths,
            &commit_message,
        ),
    )
}

#[cfg(feature = "repository-data")]
fn list_local_repository_branches(state: &ServiceState, repo_path: String) -> ServiceResponse {
    repository_data_response(
        state,
        "Local branches",
        gitmanager_core::git_info::list_local_repository_branches(Path::new(&repo_path)),
    )
}

#[cfg(feature = "repository-data")]
fn switch_repository_branch(
    state: &ServiceState,
    repo_path: String,
    branch_name: String,
) -> ServiceResponse {
    repository_data_response(
        state,
        "Branch switch",
        gitmanager_core::git_info::switch_repository_branch(
            Path::new(&repo_path), &branch_name,
        ),
    )
}

#[cfg(not(feature = "repository-data"))]
fn service_workspace_unavailable(state: &ServiceState) -> ServiceResponse {
    ServiceResponse {
        ok: false,
        status: state.status("Repository workspace IPC is not enabled in this process"),
        error: Some("Repository workspace operations are available only through gitmanager.service.".to_string()),
        data: None,
    }
}

#[cfg(not(feature = "repository-data"))]
fn save_conflict_resolution(state: &ServiceState, _: String, _: String, _: String) -> ServiceResponse {
    service_workspace_unavailable(state)
}
#[cfg(not(feature = "repository-data"))]
fn preview_repository_pull(state: &ServiceState, _: String) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn get_repository_changes(state: &ServiceState, _: String) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn read_repository_file_diff(state: &ServiceState, _: String, _: String, _: bool) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn fetch_repository_remote(state: &ServiceState, _: String, _: String) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn stage_repository_paths(state: &ServiceState, _: String, _: Vec<String>) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn unstage_repository_paths(state: &ServiceState, _: String, _: Vec<String>) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn preview_repository_commit(state: &ServiceState, _: String) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn create_repository_commit(state: &ServiceState, _: String, _: String, _: String) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn delete_repository_path(state: &ServiceState, _: String, _: String) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn prepare_and_pull_repository_update(
    state: &ServiceState, _: String, _: String, _: String, _: String, _: String, _: String, _: Vec<String>, _: String,
) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn list_local_repository_branches(state: &ServiceState, _: String) -> ServiceResponse { service_workspace_unavailable(state) }
#[cfg(not(feature = "repository-data"))]
fn switch_repository_branch(state: &ServiceState, _: String, _: String) -> ServiceResponse { service_workspace_unavailable(state) }

#[cfg(not(feature = "repository-data"))]
fn list_repository_files(
    state: &ServiceState,
    _repo_path: String,
    _relative_path: Option<String>,
    _source: String,
    _branch: Option<String>,
) -> ServiceResponse {
    service_data_unavailable(state)
}

#[cfg(not(feature = "repository-data"))]
fn read_repository_file(
    state: &ServiceState,
    _repo_path: String,
    _relative_path: String,
    _source: String,
    _branch: Option<String>,
) -> ServiceResponse {
    service_data_unavailable(state)
}

#[cfg(not(feature = "repository-data"))]
fn load_repository_overview(state: &ServiceState, _repo_path: String) -> ServiceResponse {
    service_data_unavailable(state)
}

#[cfg(not(feature = "repository-data"))]
fn load_commit_history(
    state: &ServiceState,
    _repo_path: String,
    _offset: usize,
    _limit: usize,
    _branch_filter: Option<String>,
    _search_query: Option<String>,
) -> ServiceResponse {
    service_data_unavailable(state)
}

#[cfg(not(feature = "repository-data"))]
fn load_commit_details(
    state: &ServiceState,
    _repo_path: String,
    _oid: String,
) -> ServiceResponse {
    service_data_unavailable(state)
}

#[cfg(not(feature = "repository-data"))]
fn service_data_unavailable(state: &ServiceState) -> ServiceResponse {
    ServiceResponse {
        ok: false,
        status: state.status("Repository data IPC is not enabled in this process"),
        error: Some("Repository data is available only through gitmanager.service.".to_string()),
        data: None,
    }
}

pub fn with_repository_operation_lock<T>(
    repo_path: &str,
    operation: impl FnOnce() -> T,
) -> T {
    static REPOSITORY_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let key = fs::canonicalize(repo_path).unwrap_or_else(|_| PathBuf::from(repo_path));
    let lock = {
        let locks = REPOSITORY_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut locks = locks.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // Drop idle lock entries so visiting repositories over time cannot
        // grow this registry without bound.
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        Arc::clone(locks.entry(key).or_insert_with(|| Arc::new(Mutex::new(()))))
    };
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    operation()
}

fn handle_service_connection(
    mut stream: TcpStream,
    token: &str,
    state: &ServiceState,
    scan_requested: &AtomicBool,
    remote_check_requested: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let mut bytes = Vec::new();
    let read_result = (&mut stream).take(MAX_MESSAGE_BYTES).read_to_end(&mut bytes);
    let response = if read_result.is_err() || bytes.len() as u64 >= MAX_MESSAGE_BYTES {
        ServiceResponse {
            ok: false,
            status: state.status("Invalid service-control request"),
            error: Some("Invalid or oversized request".to_string()),
            data: None,
        }
    } else {
        match serde_json::from_slice::<ServiceRequest>(&bytes) {
            Ok(request) if request.token == token => match request.command {
                ServiceCommand::Status => ServiceResponse {
                    ok: true,
                    status: state.status("Background service status"),
                    error: None,
                    data: None,
                },
                ServiceCommand::Suspend { seconds } => ServiceResponse {
                    ok: true,
                    status: state.suspend(seconds),
                    error: None,
                    data: None,
                },
                ServiceCommand::Resume => ServiceResponse {
                    ok: true,
                    status: state.resume(),
                    error: None,
                    data: None,
                },
                ServiceCommand::RequestScan => {
                    scan_requested.store(true, Ordering::Release);
                    ServiceResponse {
                        ok: true,
                        status: state.status("Repository index refresh queued"),
                        error: None,
                        data: None,
                    }
                },
                ServiceCommand::RequestRemoteCheck => {
                    remote_check_requested.store(true, Ordering::Release);
                    ServiceResponse {
                        ok: true,
                        status: state.status("Remote repository check queued"),
                        error: None,
                        data: None,
                    }
                },
                ServiceCommand::LoadAccounts => load_account_storage(state),
                ServiceCommand::SaveAccount { payload } => save_account_storage(state, payload),
                ServiceCommand::InspectRepository { repo_path } =>
                    with_repository_operation_lock(&repo_path, || load_repository_overview(state, repo_path)),
                ServiceCommand::LoadCommitHistory {
                    repo_path,
                    offset,
                    limit,
                    branch_filter,
                    search_query,
                } => with_repository_operation_lock(&repo_path, || load_commit_history(
                    state,
                    repo_path,
                    offset,
                    limit,
                    branch_filter,
                    search_query,
                )),
                ServiceCommand::GetCommitDetails { repo_path, oid } =>
                    with_repository_operation_lock(&repo_path, || load_commit_details(state, repo_path, oid)),
                ServiceCommand::ListRepositoryFiles {
                    repo_path,
                    relative_path,
                    source,
                    branch,
                } => with_repository_operation_lock(&repo_path, || list_repository_files(state, repo_path, relative_path, source, branch)),
                ServiceCommand::ReadRepositoryFile {
                    repo_path,
                    relative_path,
                    source,
                    branch,
                } => with_repository_operation_lock(&repo_path, || read_repository_file(state, repo_path, relative_path, source, branch)),
                ServiceCommand::SaveConflictResolution { repo_path, relative_path, content } =>
                    with_repository_operation_lock(&repo_path, || save_conflict_resolution(state, repo_path, relative_path, content)),
                ServiceCommand::PreviewRepositoryPull { repo_path } =>
                    with_repository_operation_lock(&repo_path, || preview_repository_pull(state, repo_path)),
                ServiceCommand::GetRepositoryChanges { repo_path } =>
                    with_repository_operation_lock(&repo_path, || get_repository_changes(state, repo_path)),
                ServiceCommand::ReadRepositoryFileDiff { repo_path, relative_path, staged } =>
                    with_repository_operation_lock(&repo_path, || read_repository_file_diff(state, repo_path, relative_path, staged)),
                ServiceCommand::FetchRepositoryRemote { repo_path, remote_name } =>
                    with_repository_operation_lock(&repo_path, || fetch_repository_remote(state, repo_path, remote_name)),
                ServiceCommand::StageRepositoryPaths { repo_path, paths } =>
                    with_repository_operation_lock(&repo_path, || stage_repository_paths(state, repo_path, paths)),
                ServiceCommand::UnstageRepositoryPaths { repo_path, paths } =>
                    with_repository_operation_lock(&repo_path, || unstage_repository_paths(state, repo_path, paths)),
                ServiceCommand::PreviewRepositoryCommit { repo_path } =>
                    with_repository_operation_lock(&repo_path, || preview_repository_commit(state, repo_path)),
                ServiceCommand::CreateRepositoryCommit { repo_path, message, expected_index_tree } =>
                    with_repository_operation_lock(&repo_path, || create_repository_commit(state, repo_path, message, expected_index_tree)),
                ServiceCommand::DeleteRepositoryPath { repo_path, relative_path } =>
                    with_repository_operation_lock(&repo_path, || delete_repository_path(state, repo_path, relative_path)),
                ServiceCommand::PrepareAndPullRepositoryUpdate {
                    repo_path, expected_branch, expected_local_oid, expected_remote_oid,
                    expected_status_token, mode, stage_paths, commit_message,
                } => with_repository_operation_lock(&repo_path, || prepare_and_pull_repository_update(
                    state, repo_path, expected_branch, expected_local_oid, expected_remote_oid,
                    expected_status_token, mode, stage_paths, commit_message,
                )),
                ServiceCommand::ListLocalRepositoryBranches { repo_path } =>
                    with_repository_operation_lock(&repo_path, || list_local_repository_branches(state, repo_path)),
                ServiceCommand::SwitchRepositoryBranch { repo_path, branch_name } =>
                    with_repository_operation_lock(&repo_path, || switch_repository_branch(state, repo_path, branch_name)),
                ServiceCommand::ListNotifications => list_notifications(state),
                ServiceCommand::MarkNotificationRead { id } => mark_notification_read(state, id),
                ServiceCommand::DismissNotification { id } => dismiss_notification(state, id),
                ServiceCommand::MarkAllNotificationsRead => mark_all_notifications_read(state),
                ServiceCommand::LoadAppPreferences => load_app_preferences(state),
                ServiceCommand::SaveAppPreferences { payload } => save_app_preferences(state, payload),
                ServiceCommand::GetRepositoryIndexGeneration => get_repository_index_generation(state),
                ServiceCommand::LoadIndexedRepositories { filter_mode } =>
                    load_indexed_repositories(state, filter_mode),
                ServiceCommand::LoadRepositoryPreferences { repo_path, repo_id } =>
                    with_repository_operation_lock(&repo_path, || load_repository_preferences(state, repo_path, repo_id)),
                ServiceCommand::SaveRepositoryPreferences { repo_path, payload } =>
                    with_repository_operation_lock(&repo_path, || save_repository_preferences(state, repo_path, payload)),
            },
            Ok(_) => ServiceResponse {
                ok: false,
                status: state.status("Unauthorized service-control request"),
                error: Some("Authentication failed".to_string()),
                data: None,
            },
            Err(error) => ServiceResponse {
                ok: false,
                status: state.status("Invalid service-control request"),
                error: Some(format!("Invalid request: {error}")),
                data: None,
            },
        }
    };

    if let Ok(encoded) = serde_json::to_vec(&response) {
        let response_bytes = if encoded.len() as u64 >= MAX_MESSAGE_BYTES {
            serde_json::to_vec(&ServiceResponse {
                ok: false,
                status: state.status("Service response exceeded its size limit"),
                error: Some("The service response is too large to send over local IPC.".to_string()),
                data: None,
            }).unwrap_or_default()
        } else {
            encoded
        };
        let _ = stream.write_all(&response_bytes);
        let _ = stream.flush();
    }
}


fn send_service_request(command: ServiceCommand) -> Result<ServiceResponse, String> {
    let endpoint_path = gitmanager_data_dir()?.join(ENDPOINT_FILE);
    let content = fs::read(&endpoint_path)
        .map_err(|error| format!("GitManager background service is not reachable: {error}"))?;
    if content.len() > 4096 {
        return Err("Service-control endpoint file is invalid.".to_string());
    }

    let endpoint: Endpoint = serde_json::from_slice(&content)
        .map_err(|error| format!("Service-control endpoint is invalid: {error}"))?;
    // Protocol v1 remains readable for status-only upgrade negotiation.
    // Account-cache messages are v2-only and are rejected by an old worker.
    if endpoint.version != 1 && endpoint.version != SERVICE_PROTOCOL_VERSION {
        return Err("Service-control protocol version is unsupported.".to_string());
    }

    let address: SocketAddr = endpoint.address.parse()
        .map_err(|_| "Service-control address is invalid.".to_string())?;
    if !address.ip().is_loopback() {
        return Err("Service-control endpoint is not a loopback address.".to_string());
    }

    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| format!("Could not connect to GitManager background service: {error}"))?;
    // Git fetch/pull and deep-history reads can legitimately outlast a
    // short health-check timeout. The caller remains asynchronous in Tauri.
    stream.set_read_timeout(Some(Duration::from_secs(90))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(15))).ok();

    let request = ServiceRequest { token: endpoint.token, command };
    let encoded = serde_json::to_vec(&request)
        .map_err(|error| format!("Could not encode service-control request: {error}"))?;
    if encoded.len() as u64 >= MAX_MESSAGE_BYTES {
        return Err("Account request is too large for the local service protocol.".to_string());
    }
    stream.write_all(&encoded)
        .map_err(|error| format!("Could not send service-control request: {error}"))?;
    stream.shutdown(std::net::Shutdown::Write).ok();

    let mut response_bytes = Vec::new();
    stream.take(MAX_MESSAGE_BYTES).read_to_end(&mut response_bytes)
        .map_err(|error| format!("Could not read service-control response: {error}"))?;
    if response_bytes.len() as u64 >= MAX_MESSAGE_BYTES {
        return Err("Service-control response exceeded its size limit.".to_string());
    }
    let mut response: ServiceResponse = serde_json::from_slice(&response_bytes)
        .map_err(|error| format!("Service-control response was invalid: {error}"))?;
    if response.status.protocol_version == 0 {
        response.status.protocol_version = endpoint.version;
    }
    if !response.ok {
        return Err(response.error.unwrap_or_else(|| "Service-control request was rejected.".to_string()));
    }
    Ok(response)
}

fn send_service_command(command: ServiceCommand) -> Result<ServiceStatus, String> {
    send_service_request(command).map(|response| response.status)
}

pub fn load_service_account_cache() -> Result<serde_json::Value, String> {
    let response = send_service_request(ServiceCommand::LoadAccounts)?;
    response.data.ok_or_else(|| "The service returned no saved account metadata.".to_string())
}

pub fn save_service_account_cache(payload: serde_json::Value) -> Result<(), String> {
    send_service_request(ServiceCommand::SaveAccount { payload }).map(|_| ())
}

fn service_response_data(command: ServiceCommand, description: &str) -> Result<serde_json::Value, String> {
    let response = send_service_request(command)?;
    response.data.ok_or_else(|| format!("The core service returned no {description} payload."))
}

pub fn load_service_repository_overview(repo_path: String) -> Result<serde_json::Value, String> {
    service_response_data(
        ServiceCommand::InspectRepository { repo_path },
        "repository overview",
    )
}

pub fn load_service_commit_history(
    repo_path: String,
    offset: usize,
    limit: usize,
    branch_filter: Option<String>,
    search_query: Option<String>,
) -> Result<serde_json::Value, String> {
    service_response_data(
        ServiceCommand::LoadCommitHistory {
            repo_path,
            offset,
            limit,
            branch_filter,
            search_query,
        },
        "commit history",
    )
}

pub fn load_service_commit_details(
    repo_path: String,
    oid: String,
) -> Result<serde_json::Value, String> {
    service_response_data(
        ServiceCommand::GetCommitDetails { repo_path, oid },
        "commit details",
    )
}

pub fn load_service_repository_files(
    repo_path: String,
    relative_path: Option<String>,
    source: String,
    branch: Option<String>,
) -> Result<serde_json::Value, String> {
    service_response_data(
        ServiceCommand::ListRepositoryFiles {
            repo_path,
            relative_path,
            source,
            branch,
        },
        "repository directory",
    )
}

pub fn read_service_repository_file(
    repo_path: String,
    relative_path: String,
    source: String,
    branch: Option<String>,
) -> Result<serde_json::Value, String> {
    service_response_data(
        ServiceCommand::ReadRepositoryFile {
            repo_path,
            relative_path,
            source,
            branch,
        },
        "repository file preview",
    )
}

pub fn service_save_conflict_resolution(
    repo_path: String, relative_path: String, content: String,
) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::SaveConflictResolution { repo_path, relative_path, content }, "conflict resolution")
}
pub fn service_preview_repository_pull(repo_path: String) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::PreviewRepositoryPull { repo_path }, "pull preview")
}
pub fn service_get_repository_changes(repo_path: String) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::GetRepositoryChanges { repo_path }, "repository changes")
}
pub fn service_read_repository_file_diff(
    repo_path: String, relative_path: String, staged: bool,
) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::ReadRepositoryFileDiff { repo_path, relative_path, staged }, "repository file diff")
}
pub fn service_fetch_repository_remote(
    repo_path: String, remote_name: String,
) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::FetchRepositoryRemote { repo_path, remote_name }, "remote fetch result")
}
pub fn service_stage_repository_paths(
    repo_path: String, paths: Vec<String>,
) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::StageRepositoryPaths { repo_path, paths }, "staged changes")
}
pub fn service_unstage_repository_paths(
    repo_path: String, paths: Vec<String>,
) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::UnstageRepositoryPaths { repo_path, paths }, "unstaged changes")
}
pub fn service_preview_repository_commit(repo_path: String) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::PreviewRepositoryCommit { repo_path }, "commit preview")
}
pub fn service_create_repository_commit(
    repo_path: String, message: String, expected_index_tree: String,
) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::CreateRepositoryCommit { repo_path, message, expected_index_tree }, "commit result")
}
pub fn service_delete_repository_path(
    repo_path: String, relative_path: String,
) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::DeleteRepositoryPath { repo_path, relative_path }, "deletion result")
}
pub fn service_prepare_and_pull_repository_update(
    repo_path: String,
    expected_branch: String,
    expected_local_oid: String,
    expected_remote_oid: String,
    expected_status_token: String,
    mode: String,
    stage_paths: Vec<String>,
    commit_message: String,
) -> Result<serde_json::Value, String> {
    service_response_data(
        ServiceCommand::PrepareAndPullRepositoryUpdate {
            repo_path, expected_branch, expected_local_oid, expected_remote_oid,
            expected_status_token, mode, stage_paths, commit_message,
        },
        "pull result",
    )
}
pub fn service_list_local_repository_branches(repo_path: String) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::ListLocalRepositoryBranches { repo_path }, "branch list")
}
pub fn service_switch_repository_branch(
    repo_path: String, branch_name: String,
) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::SwitchRepositoryBranch { repo_path, branch_name }, "branch switch result")
}

pub fn service_list_notifications() -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::ListNotifications, "notification history")
}
pub fn service_mark_notification_read(id: String) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::MarkNotificationRead { id }, "notification read update")
}
pub fn service_dismiss_notification(id: String) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::DismissNotification { id }, "notification dismissal")
}
pub fn service_mark_all_notifications_read() -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::MarkAllNotificationsRead, "bulk notification read update")
}

pub fn service_load_app_preferences() -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::LoadAppPreferences, "application preferences")
}
pub fn service_save_app_preferences(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::SaveAppPreferences { payload }, "saved application preferences")
}

pub fn service_get_repository_index_generation() -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::GetRepositoryIndexGeneration, "repository index generation")
}
pub fn service_load_indexed_repositories(filter_mode: String) -> Result<serde_json::Value, String> {
    service_response_data(ServiceCommand::LoadIndexedRepositories { filter_mode }, "indexed repository snapshot")
}

pub fn service_load_repository_preferences(
    repo_path: String,
    repo_id: Option<String>,
) -> Result<serde_json::Value, String> {
    service_response_data(
        ServiceCommand::LoadRepositoryPreferences { repo_path, repo_id },
        "repository preferences",
    )
}
pub fn service_save_repository_preferences(
    repo_path: String,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    service_response_data(
        ServiceCommand::SaveRepositoryPreferences { repo_path, payload },
        "saved repository preferences",
    )
}

pub fn request_service_status() -> Result<ServiceStatus, String> {
    send_service_command(ServiceCommand::Status)
}

/// Asks the required service process to refresh the persisted repository index.
/// The UI never performs the directory walk itself.
pub fn request_service_scan() -> Result<ServiceStatus, String> {
    send_service_command(ServiceCommand::RequestScan)
}

/// Requests an immediate remote-update check in the required service.
pub fn request_service_remote_check() -> Result<ServiceStatus, String> {
    send_service_command(ServiceCommand::RequestRemoteCheck)
}

/// Persists the suspension request before attempting IPC. If the service is
/// not running yet, its next startup loads the deadline and remains paused.
pub fn request_service_suspend(seconds: u64) -> Result<ServiceStatus, String> {
    let seconds = seconds.clamp(MIN_SUSPENSION_SECONDS, MAX_SUSPENSION_SECONDS);
    let until = SystemTime::now() + Duration::from_secs(seconds);
    let data_dir = gitmanager_data_dir()?;
    fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Could not create service state directory: {error}"))?;
    let path = data_dir.join(SUSPENSION_FILE);
    let record = SuspensionRecord {
        suspended_until_unix_ms: Some(deadline_to_unix_ms(until)
            .map_err(|error| format!("Could not encode suspension deadline: {error}"))?),
    };
    write_private_json(&path, &record)
        .map_err(|error| format!("Could not queue background-service suspension: {error}"))?;

    match send_service_command(ServiceCommand::Suspend { seconds }) {
        Ok(status) => Ok(status),
        Err(error) => Ok(ServiceStatus {
            suspended: true,
            remaining_seconds: seconds,
            message: format!("Background-service suspension queued for {seconds} seconds; the service will observe the persisted state on its next poll: {error}"),
            service_generation: 0,
            process_id: 0,
            protocol_version: 0,
        }),
    }
}

/// Persists activation first, so a service that starts after the UI can see
/// that it should be active rather than applying an old suspension request.
pub fn request_service_resume() -> Result<ServiceStatus, String> {
    let data_dir = gitmanager_data_dir()?;
    fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Could not create service state directory: {error}"))?;
    let path = data_dir.join(SUSPENSION_FILE);
    write_private_json(&path, &SuspensionRecord { suspended_until_unix_ms: None })
        .map_err(|error| format!("Could not queue background-service activation: {error}"))?;

    match send_service_command(ServiceCommand::Resume) {
        Ok(status) => Ok(status),
        Err(error) => Ok(ServiceStatus {
            suspended: false,
            remaining_seconds: 0,
            message: format!("Background-service activation queued for its next control poll: {error}"),
            service_generation: 0,
            process_id: 0,
            protocol_version: 0,
        }),
    }
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

/// The internal child marker alone is insufficient: a normal user launch
/// cannot bypass the supervisor merely by supplying the same command-line flag.
pub fn is_supervised_child(args: &[std::ffi::OsString]) -> bool {
    args.iter().any(|argument| argument.to_string_lossy() == "--crash-helper-child")
        && std::env::var_os("GITMANAGER_CRASH_HELPER_ADDRESS").is_some()
        && std::env::var_os("GITMANAGER_CRASH_HELPER_TOKEN").is_some()
}

fn record_supervisor_start_failure(reason: &str) {
    let service_result = request_service_suspend(DEFAULT_SUSPENSION_SECONDS);
    append_incident(reason, 0, None, service_result);
}

pub fn report_supervisor_launch_failure(reason: &str) {
    // Persist the suspension intent even when the helper sidecar itself is
    // missing or cannot be started. The independent service observes the file.
    record_supervisor_start_failure(reason);
}

pub fn launch_supervisor(current_exe: &Path, args: &[std::ffi::OsString]) -> Result<bool, String> {
    if is_supervised_child(args) {
        return Ok(false);
    }
    let helper = match find_crash_helper(current_exe) {
        Some(path) => path,
        None => return Ok(false),
    };

    let mut command = Command::new(helper);
    command.arg("--supervise").arg(current_exe).arg("--").args(args);
    configure_detached_command(&mut command);
    let mut helper_process = command.spawn()
        .map_err(|error| format!("Could not start crash_helper: {error}"))?;

    // Keep the original GitManager process alive as the process-tree root.
    // This makes crash_helper.exe visible beneath the launched app while it
    // supervises the UI; the independent service remains outside this tree.
    let status = helper_process.wait()
        .map_err(|error| format!("Could not wait for crash_helper: {error}"))?;
    if !status.success() {
        return Err(format!("crash_helper exited unexpectedly with status {status}."));
    }
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
        // Keep the helper in the launched GitManager process tree. Job Object
        // containment below remains the mechanism that terminates the UI.
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn configure_supervised_ui_command(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_SUSPENDED: u32 = 0x0000_0004;
        // The visible UI remains a descendant of crash_helper.exe. Its Job
        // Object is still the authoritative crash-containment boundary.
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn take_disable_webview_gpu_flag(arguments: &mut Vec<std::ffi::OsString>) -> bool {
    let flag = std::ffi::OsStr::new("--disable-webview-gpu");
    let disabled = arguments.iter().any(|argument| argument.as_os_str() == flag);
    arguments.retain(|argument| argument.as_os_str() != flag);
    disabled
}

fn configure_webview_gpu_mode(command: &mut Command, disabled: bool) {
    #[cfg(windows)]
    if disabled {
        let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
        let arguments = if existing.split_whitespace().any(|argument| argument == "--disable-gpu") {
            existing
        } else if existing.trim().is_empty() {
            "--disable-gpu".to_string()
        } else {
            format!("{} --disable-gpu", existing.trim())
        };
        command.env("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", arguments);
    }

    #[cfg(not(windows))]
    {
        let _ = (command, disabled);
    }
}

fn attach_ui_process(pid: u32) -> Result<ProcessContainment, String> {
    #[cfg(windows)]
    {
        windows_process::attach_and_resume(pid)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Ok(ProcessContainment)
    }
}

/// Release only the supervised UI process group. On Windows, dropping the
/// containment handle closes a kill-on-job-close Job Object; on Unix, the UI
/// owns a dedicated process group. The background service is never included.
fn terminate_ui_tree(pid: u32, containment: ProcessContainment) {
    #[cfg(windows)]
    {
        let _ = pid;
        drop(containment);
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
        drop(containment);
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

#[cfg(windows)]
fn suppress_windows_application_error_dialogs() {
    // These process error-mode flags are inherited by the UI child process.
    // SEM_NOGPFAULTERRORBOX suppresses the Windows Error Reporting dialog;
    // SEM_FAILCRITICALERRORS prevents critical-error message boxes.
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
    }

    unsafe {
        let _previous = SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    }
}

#[cfg(not(windows))]
fn suppress_windows_application_error_dialogs() {}

fn run_supervisor(arguments: Vec<std::ffi::OsString>) -> i32 {
    suppress_windows_application_error_dialogs();
    if arguments.len() < 2 {
        return 2;
    }
    let application = PathBuf::from(&arguments[0]);
    let separator = arguments.iter().position(|argument| argument.to_string_lossy() == "--");
    let mut app_args = separator
        .map(|index| arguments.iter().skip(index + 1).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let disable_webview_gpu = take_disable_webview_gpu_flag(&mut app_args);

    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            record_supervisor_start_failure(&format!("Could not open UI heartbeat listener: {error}"));
            return 2;
        }
    };
    if let Err(error) = listener.set_nonblocking(true) {
        record_supervisor_start_failure(&format!("Could not configure UI heartbeat listener: {error}"));
        return 2;
    }
    let address = match listener.local_addr() {
        Ok(address) => address.to_string(),
        Err(error) => {
            record_supervisor_start_failure(&format!("Could not resolve UI heartbeat listener address: {error}"));
            return 2;
        }
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
    configure_webview_gpu_mode(&mut child_command, disable_webview_gpu);
    configure_supervised_ui_command(&mut child_command);

    let mut child: Child = match child_command.spawn() {
        Ok(child) => child,
        Err(error) => {
            record_supervisor_start_failure(&format!("Could not start GitManager UI: {error}"));
            return 1;
        }
    };

    let pid = child.id();
    let containment = match attach_ui_process(pid) {
        Ok(containment) => containment,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            record_supervisor_start_failure(&format!(
                "Could not safely contain the GitManager UI process: {error}"
            ));
            return 1;
        }
    };
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
                terminate_ui_tree(pid, containment);
                if !status.success() {
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
                terminate_ui_tree(pid, containment);
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
                terminate_ui_tree(pid, containment);
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
        ServiceCommand, ServiceRequest, ServiceState, SuspensionRecord,
        DEFAULT_SUSPENSION_SECONDS, MAX_SUSPENSION_SECONDS, MIN_SUSPENSION_SECONDS, SERVICE_GENERATION,
    };
    use super::Uuid;
    use std::fs;
    use std::time::{Duration, SystemTime};

    #[test]
    fn legacy_service_status_defaults_to_generation_zero() {
        let legacy = serde_json::json!({
            "suspended": false,
            "remaining_seconds": 0,
            "message": "legacy worker"
        });
        let status: super::ServiceStatus =
            serde_json::from_value(legacy).expect("deserialize legacy status");
        assert_eq!(status.service_generation, 0);
    }

    #[test]
    fn new_service_status_reports_the_required_generation() {
        let path = std::env::temp_dir()
            .join(format!("gitmanager-service-generation-{}.json", Uuid::new_v4()));
        let status = ServiceState::load(path.clone()).status("test");
        assert_eq!(status.service_generation, SERVICE_GENERATION);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn account_storage_commands_use_the_versioned_authenticated_protocol() {
        let save = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::SaveAccount {
                payload: serde_json::json!({
                    "metadata": { "provider": "github", "account_login": "example" },
                    "repositories": []
                }),
            },
        };
        let encoded_save = serde_json::to_value(save).expect("encode account save request");
        assert_eq!(encoded_save["token"], "test-token");
        assert_eq!(encoded_save["command"]["action"], "save_account");
        assert!(encoded_save["command"].get("payload").is_some());
        assert!(encoded_save["command"]["payload"].get("access_token").is_none());

        let load = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::LoadAccounts,
        };
        let encoded_load = serde_json::to_value(load).expect("encode account load request");
        assert_eq!(encoded_load["command"]["action"], "load_accounts");
    }

    #[test]
    fn legacy_service_response_defaults_to_empty_data_payload() {
        let legacy = serde_json::json!({
            "ok": true,
            "status": {
                "suspended": false,
                "remaining_seconds": 0,
                "message": "legacy worker"
            },
            "error": null
        });
        let response: super::ServiceResponse =
            serde_json::from_value(legacy).expect("deserialize legacy service response");
        assert!(response.ok);
        assert!(response.data.is_none());
        assert_eq!(response.status.service_generation, 0);
    }

    #[test]
    fn service_control_request_slots_are_bounded_and_released() {
        let active = AtomicUsize::new(0);
        for expected in 1..=MAX_CONCURRENT_SERVICE_REQUESTS {
            assert!(reserve_service_request_slot(&active));
            assert_eq!(active.load(Ordering::Acquire), expected);
        }
        assert!(!reserve_service_request_slot(&active));
        active.fetch_sub(1, Ordering::Release);
        assert!(reserve_service_request_slot(&active));
        assert_eq!(active.load(Ordering::Acquire), MAX_CONCURRENT_SERVICE_REQUESTS);
    }

    #[test]
    fn repository_data_commands_use_bounded_versioned_request_shapes() {
        let overview = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::InspectRepository {
                repo_path: "/workspace/repository".to_string(),
            },
        };
        let overview = serde_json::to_value(overview).expect("encode repository overview request");
        assert_eq!(overview["command"]["action"], "inspect_repository");
        assert_eq!(overview["command"]["repo_path"], "/workspace/repository");

        let history = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::LoadCommitHistory {
                repo_path: "/workspace/repository".to_string(),
                offset: 50,
                limit: 50,
                branch_filter: Some("feature/topic".to_string()),
                search_query: Some("needle".to_string()),
            },
        };
        let history = serde_json::to_value(history).expect("encode commit history request");
        assert_eq!(history["command"]["action"], "load_commit_history");
        assert_eq!(history["command"]["offset"], 50);
        assert_eq!(history["command"]["limit"], 50);
        assert_eq!(history["command"]["branch_filter"], "feature/topic");
        assert_eq!(history["command"]["search_query"], "needle");

        let details = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::GetCommitDetails {
                repo_path: "/workspace/repository".to_string(),
                oid: "0123456789abcdef".to_string(),
            },
        };
        let details = serde_json::to_value(details).expect("encode commit details request");
        assert_eq!(details["command"]["action"], "get_commit_details");
        assert_eq!(details["command"]["oid"], "0123456789abcdef");

        let list_files = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::ListRepositoryFiles {
                repo_path: "/workspace/repository".to_string(),
                relative_path: Some("src".to_string()),
                source: "remote".to_string(),
                branch: Some("origin/main".to_string()),
            },
        };
        let list_files = serde_json::to_value(list_files).expect("encode repository file list request");
        assert_eq!(list_files["command"]["action"], "list_repository_files");
        assert_eq!(list_files["command"]["relative_path"], "src");
        assert_eq!(list_files["command"]["source"], "remote");
        assert_eq!(list_files["command"]["branch"], "origin/main");

        let read_file = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::ReadRepositoryFile {
                repo_path: "/workspace/repository".to_string(),
                relative_path: "src/main.rs".to_string(),
                source: "local".to_string(),
                branch: None,
            },
        };
        let read_file = serde_json::to_value(read_file).expect("encode repository file read request");
        assert_eq!(read_file["command"]["action"], "read_repository_file");
        assert_eq!(read_file["command"]["relative_path"], "src/main.rs");
    }

    #[test]
    fn preference_and_repository_index_commands_use_stable_wire_names() {
        let requests = vec![
            ServiceCommand::LoadAppPreferences,
            ServiceCommand::SaveAppPreferences {
                payload: serde_json::json!({
                    "scanRoots": ["/workspace"],
                    "maxScanDepth": 8,
                    "firstRunComplete": true
                }),
            },
            ServiceCommand::GetRepositoryIndexGeneration,
            ServiceCommand::LoadIndexedRepositories {
                filter_mode: "grouped".to_string(),
            },
        ];
        let expected = [
            "load_app_preferences",
            "save_app_preferences",
            "get_repository_index_generation",
            "load_indexed_repositories",
        ];
        for (command, action) in requests.into_iter().zip(expected) {
            let request = ServiceRequest {
                token: "authenticated-test-token".to_string(),
                command,
            };
            let encoded = serde_json::to_value(request).expect("encode service storage request");
            assert_eq!(encoded["token"], "authenticated-test-token");
            assert_eq!(encoded["command"]["action"], action);
        }
    }

    #[test]
    fn repository_preference_commands_use_stable_wire_names() {
        let requests = vec![
            ServiceCommand::LoadRepositoryPreferences {
                repo_path: "/workspace/repo".to_string(),
                repo_id: Some("local::repo-id".to_string()),
            },
            ServiceCommand::SaveRepositoryPreferences {
                repo_path: "/workspace/repo".to_string(),
                payload: serde_json::json!({
                    "autoUpdate": false,
                    "cleanerExcludePatterns": ["target"]
                }),
            },
        ];
        let expected = ["load_repository_preferences", "save_repository_preferences"];
        for (command, action) in requests.into_iter().zip(expected) {
            let request = ServiceRequest {
                token: "authenticated-test-token".to_string(),
                command,
            };
            let encoded = serde_json::to_value(request).expect("encode repository preference request");
            assert_eq!(encoded["token"], "authenticated-test-token");
            assert_eq!(encoded["command"]["action"], action);
            assert_eq!(encoded["command"]["repo_path"], "/workspace/repo");
        }
    }

    #[test]
    fn notification_commands_use_stable_service_wire_names() {
        let requests = vec![
            ServiceCommand::ListNotifications,
            ServiceCommand::MarkNotificationRead { id: "notification-1".to_string() },
            ServiceCommand::DismissNotification { id: "notification-2".to_string() },
            ServiceCommand::MarkAllNotificationsRead,
        ];
        let expected = [
            "list_notifications",
            "mark_notification_read",
            "dismiss_notification",
            "mark_all_notifications_read",
        ];
        for (command, action) in requests.into_iter().zip(expected) {
            let request = ServiceRequest {
                token: "authenticated-test-token".to_string(),
                command,
            };
            let encoded = serde_json::to_value(request).expect("encode notification IPC request");
            assert_eq!(encoded["token"], "authenticated-test-token");
            assert_eq!(encoded["command"]["action"], action);
        }
    }

    #[test]
    fn worktree_and_commit_commands_use_stable_service_wire_names() {
        let requests = vec![
            ServiceCommand::SaveConflictResolution {
                repo_path: "/repo".to_string(), relative_path: "src/main.rs".to_string(), content: "fixed".to_string(),
            },
            ServiceCommand::PreviewRepositoryPull { repo_path: "/repo".to_string() },
            ServiceCommand::GetRepositoryChanges { repo_path: "/repo".to_string() },
            ServiceCommand::ReadRepositoryFileDiff {
                repo_path: "/repo".to_string(), relative_path: "src/main.rs".to_string(), staged: true,
            },
            ServiceCommand::FetchRepositoryRemote {
                repo_path: "/repo".to_string(), remote_name: "origin".to_string(),
            },
            ServiceCommand::StageRepositoryPaths {
                repo_path: "/repo".to_string(), paths: vec!["src/main.rs".to_string()],
            },
            ServiceCommand::UnstageRepositoryPaths {
                repo_path: "/repo".to_string(), paths: vec!["src/main.rs".to_string()],
            },
            ServiceCommand::PreviewRepositoryCommit { repo_path: "/repo".to_string() },
            ServiceCommand::CreateRepositoryCommit {
                repo_path: "/repo".to_string(), message: "test".to_string(), expected_index_tree: "tree".to_string(),
            },
            ServiceCommand::DeleteRepositoryPath {
                repo_path: "/repo".to_string(), relative_path: "old.txt".to_string(),
            },
            ServiceCommand::PrepareAndPullRepositoryUpdate {
                repo_path: "/repo".to_string(), expected_branch: "main".to_string(),
                expected_local_oid: "local".to_string(), expected_remote_oid: "remote".to_string(),
                expected_status_token: "status".to_string(), mode: "stash".to_string(),
                stage_paths: vec![], commit_message: String::new(),
            },
            ServiceCommand::ListLocalRepositoryBranches { repo_path: "/repo".to_string() },
            ServiceCommand::SwitchRepositoryBranch {
                repo_path: "/repo".to_string(), branch_name: "feature/topic".to_string(),
            },
        ];
        let expected = [
            "save_conflict_resolution", "preview_repository_pull", "get_repository_changes",
            "read_repository_file_diff", "fetch_repository_remote", "stage_repository_paths",
            "unstage_repository_paths", "preview_repository_commit", "create_repository_commit",
            "delete_repository_path", "prepare_and_pull_repository_update",
            "list_local_repository_branches", "switch_repository_branch",
        ];
        for (command, action) in requests.into_iter().zip(expected) {
            let request = ServiceRequest {
                token: "authenticated-test-token".to_string(),
                command,
            };
            let encoded = serde_json::to_value(request).expect("encode service request");
            assert_eq!(encoded["token"], "authenticated-test-token");
            assert_eq!(encoded["command"]["action"], action);
            assert!(encoded["command"]["repo_path"].is_string());
        }
    }

    #[test]
    fn remote_check_request_uses_the_authenticated_control_protocol() {
        let request = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::RequestRemoteCheck,
        };
        let encoded = serde_json::to_value(request).expect("encode remote check request");
        assert_eq!(encoded["token"], "test-token");
        assert_eq!(encoded["command"]["action"], "request_remote_check");
    }

    #[test]
    fn repository_scan_request_uses_the_authenticated_control_protocol() {
        let request = ServiceRequest {
            token: "test-token".to_string(),
            command: ServiceCommand::RequestScan,
        };
        let encoded = serde_json::to_value(request).expect("encode scan request");
        assert_eq!(encoded["token"], "test-token");
        assert_eq!(encoded["command"]["action"], "request_scan");
    }

    #[test]
    fn gpu_compatibility_switch_is_consumed_without_losing_other_arguments() {
        let mut arguments = vec![
            std::ffi::OsString::from("--setup"),
            std::ffi::OsString::from("--disable-webview-gpu"),
            std::ffi::OsString::from("--profile"),
            std::ffi::OsString::from("test"),
        ];

        assert!(super::take_disable_webview_gpu_flag(&mut arguments));
        assert_eq!(
            arguments,
            vec![
                std::ffi::OsString::from("--setup"),
                std::ffi::OsString::from("--profile"),
                std::ffi::OsString::from("test"),
            ]
        );
        assert!(!super::take_disable_webview_gpu_flag(&mut arguments));
    }

    #[test]
    fn suspension_window_is_bounded() {
        assert_eq!(DEFAULT_SUSPENSION_SECONDS, 120);
        assert!(MIN_SUSPENSION_SECONDS >= 10);
        assert!(MAX_SUSPENSION_SECONDS <= 600);
        assert!(DEFAULT_SUSPENSION_SECONDS <= MAX_SUSPENSION_SECONDS);
    }

    #[test]
    fn service_suspension_can_be_resumed_and_expires() {
        let path = std::env::temp_dir()
            .join(format!("gitmanager-crash-helper-{}.json", Uuid::new_v4()));
        let state = ServiceState::load(path.clone());
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
    fn suspension_deadline_survives_a_service_restart() {
        let path = std::env::temp_dir()
            .join(format!("gitmanager-crash-helper-{}.json", Uuid::new_v4()));

        let first_process = ServiceState::load(path.clone());
        let suspended = first_process.suspend(60);
        assert!(suspended.suspended);
        drop(first_process);

        let restarted_process = ServiceState::load(path.clone());
        let recovered_status = restarted_process.status("recovered");
        assert!(recovered_status.suspended);
        assert!(recovered_status.remaining_seconds > 0);
        assert!(recovered_status.remaining_seconds <= 60);

        assert!(!restarted_process.resume().suspended);
        assert!(!ServiceState::load(path.clone()).is_suspended());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn running_service_observes_a_queued_external_suspend_and_resume() {
        let path = std::env::temp_dir()
            .join(format!("gitmanager-crash-helper-{}.json", Uuid::new_v4()));
        let service = ServiceState::load(path.clone());
        assert!(!service.is_suspended());

        let deadline = SystemTime::now() + Duration::from_secs(45);
        let queued = SuspensionRecord {
            suspended_until_unix_ms: Some(super::deadline_to_unix_ms(deadline).unwrap()),
        };
        super::write_private_json(&path, &queued).unwrap();
        assert!(service.is_suspended());

        let active = SuspensionRecord { suspended_until_unix_ms: None };
        super::write_private_json(&path, &active).unwrap();
        assert!(!service.is_suspended());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn suspension_requests_are_clamped_to_the_maximum() {
        let path = std::env::temp_dir()
            .join(format!("gitmanager-crash-helper-{}.json", Uuid::new_v4()));
        let state = ServiceState::load(path.clone());
        let status = state.suspend(u64::MAX);
        assert!(status.suspended);
        assert!(status.remaining_seconds <= MAX_SUSPENSION_SECONDS);
        assert!(status.remaining_seconds >= MAX_SUSPENSION_SECONDS - 1);
    }
}
