mod process_utils;
pub use gitmanager_core::notification_center;
pub use gitmanager_core::{AppPreferences, RepositoryPreferences};
pub mod remote_providers;
pub use gitmanager_core::repository_index;
pub mod watcher_service;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;
use serde::{Deserialize, Serialize};
use crate::process_utils::hidden_command;
use serde_json::Value;
use tauri::path::BaseDirectory;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::ShellExt;

use gitmanager_core::models::GitRepositoryInfo;
use remote_providers::{ConnectedAccountSummary, ProviderAccountRequest};
use repository_index::{default_scan_roots, IndexedRepoSummary};
use watcher_service::{
    register_os_background_service, BackgroundWatcherControl,
    RepoUpdateNotification, ServiceStatusInfo,
};

#[derive(Default)]
pub struct AppState {
    pub cached_repos: Mutex<Vec<GitRepositoryInfo>>,
    pub connected_accounts: Mutex<Vec<ConnectedAccountSummary>>,
    pub watcher: BackgroundWatcherControl,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationDirectories {
    software_root: String,
    install_dir: String,
    config_dir: String,
    data_dir: String,
    repo_metadata_dir: String,
    git_data_dir: String,
    account_metadata_dir: String,
    account_git_data_dir: String,
    remote_repo_metadata_dir: String,
    remote_repo_git_data_dir: String,
    cache_dir: String,
}

fn application_directory_layout(local_data: &Path) -> ApplicationDirectories {
    let software_root = local_data.join("kastrick's_software");
    let install_dir = software_root.join("GitManager");
    let config_dir = software_root.join("config").join("GitManager");
    let data_dir = software_root.join("data").join("GitManager");
    let repo_metadata_dir = data_dir.join("repo").join("metadata");
    let git_data_dir = data_dir.join("git_data");
    let account_metadata_dir = data_dir.join("account").join("metadata");
    let account_git_data_dir = data_dir.join("account").join("git_data");
    let remote_repo_metadata_dir = data_dir.join("repo").join("remote").join("metadata");
    let remote_repo_git_data_dir = data_dir.join("repo").join("remote").join("git_data");
    let cache_dir = data_dir.join(".cache");
    ApplicationDirectories {
        software_root: software_root.to_string_lossy().to_string(),
        install_dir: install_dir.to_string_lossy().to_string(),
        config_dir: config_dir.to_string_lossy().to_string(),
        data_dir: data_dir.to_string_lossy().to_string(),
        repo_metadata_dir: repo_metadata_dir.to_string_lossy().to_string(),
        git_data_dir: git_data_dir.to_string_lossy().to_string(),
        account_metadata_dir: account_metadata_dir.to_string_lossy().to_string(),
        account_git_data_dir: account_git_data_dir.to_string_lossy().to_string(),
        remote_repo_metadata_dir: remote_repo_metadata_dir.to_string_lossy().to_string(),
        remote_repo_git_data_dir: remote_repo_git_data_dir.to_string_lossy().to_string(),
        cache_dir: cache_dir.to_string_lossy().to_string(),
    }
}

fn application_directories() -> Result<ApplicationDirectories, String> {
    // Resolve paths for Settings/diagnostics only. The required service owns
    // directory creation through gitmanager_core::storage::ensure_data_layout.
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the current user's local application-data directory.".to_string())?;
    Ok(application_directory_layout(&local_data))
}

#[tauri::command]
fn get_application_directories() -> Result<ApplicationDirectories, String> {
    application_directories()
}

fn preferences_file(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let directories = application_directories()?;
    let path = PathBuf::from(directories.config_dir).join("preferences.json");

    // Migrate the previous Tauri-managed preferences file once, without
    // deleting the old copy. The service creates this directory on all normal
    // launches; Tauri creates only this config parent when an actual legacy
    // settings file needs to be copied before the service starts.
    if !path.exists() {
        if let Ok(legacy_directory) = app_handle.path().app_config_dir() {
            let legacy_file = legacy_directory.join("preferences.json");
            if legacy_file.is_file() {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| format!("Could not prepare the GitManager preferences migration directory: {error}"))?;
                }
                fs::copy(&legacy_file, &path)
                    .map_err(|error| format!("Could not migrate existing GitManager preferences: {error}"))?;
            }
        }
    }

    Ok(path)
}

#[tauri::command]
async fn load_app_preferences() -> Result<AppPreferences, String> {
    request_core_service(
        crash_helper::service_load_app_preferences,
        "application preferences",
    ).await
}

#[tauri::command]
async fn load_repository_settings(
    repo_path: String,
    repo_id: Option<String>,
) -> Result<RepositoryPreferences, String> {
    request_core_service(
        move || crash_helper::service_load_repository_preferences(repo_path, repo_id),
        "repository preferences",
    ).await
}

#[tauri::command]
async fn save_repository_settings(
    repo_path: String,
    settings: RepositoryPreferences,
) -> Result<RepositoryPreferences, String> {
    let payload = serde_json::to_value(settings)
        .map_err(|error| format!("Could not encode repository preferences: {error}"))?;
    request_core_service(
        move || crash_helper::service_save_repository_preferences(repo_path, payload),
        "save repository preferences",
    ).await
}

#[tauri::command]
async fn get_commit_details(repo_path: String, oid: String) -> Result<gitmanager_core::models::CommitDetails, String> {
    let data = tokio::task::spawn_blocking(move || {
        crash_helper::load_service_commit_details(repo_path, oid)
    })
    .await
    .map_err(|error| format!("Commit detail service request task failed: {error}"))??;
    serde_json::from_value(data)
        .map_err(|error| format!("Core service returned invalid commit details: {error}"))
}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliToolAvailability {
    git: bool,
    github: bool,
}

fn cli_tool_available(name: &str) -> bool {
    let output = hidden_command(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    output.map(|status| status.success()).unwrap_or(false)
}

#[tauri::command]
fn check_cli_tools() -> CliToolAvailability {
    CliToolAvailability {
        git: cli_tool_available("git"),
        github: cli_tool_available("gh"),
    }
}

#[tauri::command]
fn exit_application(app_handle: AppHandle) {
    app_handle.exit(0);
}

const FLATPAK_GITM_WRAPPER_MARKER: &str = "# Managed by GitManager Flatpak CLI integration.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlatpakCliIntegrationStatus {
    is_flatpak: bool,
    installed: bool,
    wrapper_path: Option<String>,
}

fn flatpak_gitm_wrapper_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local").join("bin").join("gitm"))
}

fn is_gitmanager_flatpak() -> bool {
    std::env::var("FLATPAK_ID")
        .map(|value| value == "com.gitmanager.desktop")
        .unwrap_or(false)
}

#[tauri::command]
fn get_flatpak_cli_integration_status() -> FlatpakCliIntegrationStatus {
    let is_flatpak = is_gitmanager_flatpak();
    let wrapper_path = if is_flatpak { flatpak_gitm_wrapper_path() } else { None };
    let installed = wrapper_path.as_ref().map(|path| {
        fs::read_to_string(path)
            .map(|content| content.lines().nth(1) == Some(FLATPAK_GITM_WRAPPER_MARKER))
            .unwrap_or(false)
    }).unwrap_or(false);
    FlatpakCliIntegrationStatus {
        is_flatpak,
        installed,
        wrapper_path: wrapper_path.map(|path| path.to_string_lossy().to_string()),
    }
}

#[tauri::command]
fn install_flatpak_gitm_cli() -> Result<String, String> {
    if !is_gitmanager_flatpak() {
        return Err("The gitm Flatpak wrapper can only be installed from the GitManager Flatpak.".to_string());
    }
    let path = flatpak_gitm_wrapper_path()
        .ok_or_else(|| "The user's home directory could not be resolved.".to_string())?;
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "The existing path at {} is not a regular GitManager wrapper file. It was not changed.",
                path.display()
            ));
        }
        let existing = fs::read_to_string(&path)
            .map_err(|error| format!("Could not inspect the existing gitm command: {error}"))?;
        if existing.lines().nth(1) != Some(FLATPAK_GITM_WRAPPER_MARKER) {
            return Err(format!(
                "A different file already exists at {}. It was not overwritten.",
                path.display()
            ));
        }
    }
    let parent = path.parent()
        .ok_or_else(|| "The gitm wrapper directory could not be resolved.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let wrapper = format!(
        "#!/bin/sh\n{marker}\nexec flatpak run --command=gitm --filesystem=home com.gitmanager.desktop \"$@\"\n",
        marker = FLATPAK_GITM_WRAPPER_MARKER
    );
    fs::write(&path, wrapper)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("Could not make {} executable: {error}", path.display()))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn remove_flatpak_gitm_cli() -> Result<(), String> {
    if !is_gitmanager_flatpak() {
        return Err("The gitm Flatpak wrapper can only be managed from the GitManager Flatpak.".to_string());
    }
    let path = flatpak_gitm_wrapper_path()
        .ok_or_else(|| "The user's home directory could not be resolved.".to_string())?;
    if !path.exists() {
        return Ok(());
    }
    let existing = fs::read_to_string(&path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    if existing.lines().nth(1) != Some(FLATPAK_GITM_WRAPPER_MARKER) {
        return Err(format!(
            "The file at {} is not managed by GitManager and was not removed.",
            path.display()
        ));
    }
    fs::remove_file(&path)
        .map_err(|error| format!("Could not remove {}: {error}", path.display()))
}

fn normalize_scan_roots(roots: Vec<String>) -> Result<Vec<String>, String> {
    if roots.len() > 32 {
        return Err("GitManager supports up to 32 scan roots.".to_string());
    }

    let mut normalized = Vec::new();
    for root in roots {
        let raw = root.trim();
        if raw.is_empty() {
            continue;
        }
        let path = PathBuf::from(raw);
        if !path.is_dir() {
            return Err(format!("Scan root is not an existing directory: {raw}"));
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|error| format!("Could not resolve scan root {raw}: {error}"))?;
        let value = canonical.to_string_lossy().to_string();
        if !normalized.iter().any(|existing: &String| existing.eq_ignore_ascii_case(&value)) {
            normalized.push(value);
        }
    }

    if normalized.is_empty() {
        return Err("Add at least one directory to scan.".to_string());
    }
    Ok(normalized)
}

#[tauri::command]
async fn save_cloud_node_address(address: String) -> Result<String, String> {
    let address = address.trim();
    let normalized = if address.is_empty() {
        String::new()
    } else {
        validate_rbe_node_base_url(address)?.to_string()
    };
    let mut preferences: AppPreferences = request_core_service(
        crash_helper::service_load_app_preferences,
        "application preferences",
    ).await?;
    preferences.cloud_node_address = normalized.clone();
    let payload = serde_json::to_value(preferences)
        .map_err(|error| format!("Could not encode GitManager preferences: {error}"))?;
    let _: AppPreferences = request_core_service(
        move || crash_helper::service_save_app_preferences(payload),
        "save application preferences",
    ).await?;
    Ok(normalized)
}

#[tauri::command]
async fn save_app_preferences(
    mut preferences: AppPreferences,
) -> Result<AppPreferences, String> {
    preferences.scan_roots = normalize_scan_roots(preferences.scan_roots)?;
    if !(2..=16).contains(&preferences.max_scan_depth) {
        return Err("Scan depth must be between 2 and 16.".to_string());
    }
    preferences.archive_trigger = preferences.archive_trigger.trim().to_ascii_lowercase();
    if !matches!(preferences.archive_trigger.as_str(), "manual" | "commit" | "change") {
        return Err("Archive trigger must be manual, commit, or change.".to_string());
    }
    if !(250..=60_000).contains(&preferences.archive_quiet_ms) {
        return Err("Archive quiet period must be between 250 and 60000 milliseconds.".to_string());
    }
    let saved_node_address = preferences.cloud_node_address.trim();
    preferences.cloud_node_address = if saved_node_address.is_empty() {
        String::new()
    } else {
        validate_rbe_node_base_url(saved_node_address)?.to_string()
    };

    let payload = serde_json::to_value(preferences)
        .map_err(|error| format!("Could not encode GitManager preferences: {error}"))?;
    request_core_service(
        move || crash_helper::service_save_app_preferences(payload),
        "save application preferences",
    ).await
}

fn default_archive_root_path() -> Result<PathBuf, String> {
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the current user's local application-data directory.".to_string())?;
    Ok(local_data
        .join("kastrick's_software")
        .join("data")
        .join("GitManager")
        .join("archives"))
}

#[tauri::command]
fn get_default_archive_directory() -> Result<String, String> {
    default_archive_root_path().map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
async fn get_archive_cloud_storage_info(archive_root: String) -> Result<Value, String> {
    request_core_service(
        move || crash_helper::service_get_archive_cloud_storage_info(archive_root),
        "archive cloud storage information",
    ).await
}

#[tauri::command]
async fn configure_archive_cloud_storage(
    archive_root: String,
    provider_json: String,
) -> Result<Value, String> {
    request_core_service(
        move || crash_helper::service_configure_archive_cloud_storage(archive_root, provider_json),
        "archive cloud storage configuration",
    ).await
}

#[tauri::command]
async fn disable_archive_cloud_storage(archive_root: String) -> Result<Value, String> {
    request_core_service(
        move || crash_helper::service_disable_archive_cloud_storage(archive_root),
        "archive cloud storage configuration",
    ).await
}

#[tauri::command]
async fn sync_archive_cloud_storage(archive_root: String) -> Result<Value, String> {
    request_core_service(
        move || crash_helper::service_sync_archive_cloud_storage(archive_root),
        "archive cloud storage synchronization",
    ).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RbeNodeStatusInfo {
    address: String,
    reachable: bool,
    backend_http_status: Option<u16>,
    api_route_http_status: Option<u16>,
    api_status_http_status: Option<u16>,
    backend_health: Option<Value>,
    api_route_health: Option<Value>,
    api_status_health: Option<Value>,
    backend_error: Option<String>,
    api_route_error: Option<String>,
    api_status_error: Option<String>,
    message: String,
}

fn validate_rbe_node_base_url(address: &str) -> Result<url::Url, String> {
    let input = address.trim();
    if input.is_empty() {
        return Err("Enter the NAS IP address and optional port.".to_string());
    }
    let candidate = if input.contains("://") {
        input.to_string()
    } else {
        format!("http://{input}")
    };
    let mut parsed = url::Url::parse(&candidate)
        .map_err(|error| format!("NAS address is not a valid URL or IP: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !matches!(parsed.path(), "" | "/")
    {
        return Err(
            "Use an HTTP or HTTPS LAN IP with an optional port only; paths and embedded credentials are not accepted."
                .to_string(),
        );
    }

    let host = parsed
        .host()
        .ok_or_else(|| "NAS address must contain an IP address.".to_string())?;
    let is_lan_ip = match host {
        url::Host::Ipv4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || (ip.octets()[0] == 169 && ip.octets()[1] == 254)
        }
        url::Host::Ipv6(ip) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
        url::Host::Domain(_) => false,
    };
    if !is_lan_ip {
        return Err(
            "For this LAN status check, enter a private, link-local, or loopback IP address—not a public host."
                .to_string(),
        );
    }

    if parsed.port() == Some(0) {
        return Err("NAS backend port must be between 1 and 65535.".to_string());
    }
    if parsed.port().is_none() {
        let port = if parsed.scheme() == "https" { 443 } else { 8080 };
        parsed
            .set_port(Some(port))
            .map_err(|_| "Could not apply the default RBE backend port.".to_string())?;
    }
    Ok(parsed)
}

async fn fetch_rbe_health(
    client: &reqwest::Client,
    endpoint: url::Url,
) -> (Option<u16>, Option<Value>, Option<String>) {
    let response = match client
        .get(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return (None, None, Some(error.to_string())),
    };
    let status = response.status().as_u16();
    let mut response = response;
    let mut body = Vec::new();
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(error) => return (Some(status), None, Some(error.to_string())),
        };
        if body.len().saturating_add(chunk.len()) > 64 * 1024 {
            return (
                Some(status),
                None,
                Some("RBE health response exceeded the 64 KiB limit.".to_string()),
            );
        }
        body.extend_from_slice(&chunk);
    }
    let value = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<Value>(&body).ok()
    };
    (Some(status), value, None)
}

#[tauri::command]
async fn check_rbe_node_status(address: String) -> Result<RbeNodeStatusInfo, String> {
    let base = validate_rbe_node_base_url(&address)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(6))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("Could not create the RBE status client: {error}"))?;

    let mut backend_endpoint = base.clone();
    backend_endpoint.set_path("/health");
    let mut api_endpoint = base.clone();
    api_endpoint.set_path("/api/health");
    let mut api_status_endpoint = base.clone();
    api_status_endpoint.set_path("/api/status");

    let (backend_http_status, backend_health, backend_error) =
        fetch_rbe_health(&client, backend_endpoint).await;
    let (api_route_http_status, api_route_health, api_route_error) =
        fetch_rbe_health(&client, api_endpoint).await;
    let (api_status_http_status, api_status_health, api_status_error) =
        fetch_rbe_health(&client, api_status_endpoint).await;
    let reachable = backend_http_status.is_some_and(|status| (200..300).contains(&status));
    let message = if reachable {
        "RBE backend is reachable on the LAN. Custom health and status route responses are reported separately.".to_string()
    } else {
        "Could not confirm the RBE backend health endpoint. Check the IP, port, host firewall and whether backend is running.".to_string()
    };

    Ok(RbeNodeStatusInfo {
        address: base.to_string(),
        reachable,
        backend_http_status,
        api_route_http_status,
        api_status_http_status,
        backend_health,
        api_route_health,
        api_status_health,
        backend_error,
        api_route_error,
        api_status_error,
        message,
    })
}

#[tauri::command]
async fn capture_repository_archive(
    repo_path: String,
    kind: String,
    archive_root: Option<String>,
) -> Result<Value, String> {
    request_core_service(
        move || crash_helper::service_capture_repository_archive(repo_path, kind, archive_root),
        "repository archive capture",
    ).await
}

#[tauri::command]
async fn list_repository_archives(
    repo_path: String,
    kind: Option<String>,
    archive_root: Option<String>,
) -> Result<Value, String> {
    request_core_service(
        move || crash_helper::service_list_repository_archives(repo_path, kind, archive_root),
        "repository archive list",
    ).await
}

#[tauri::command]
async fn restore_repository_archive(
    repo_path: String,
    destination_path: String,
    snapshot_id: String,
    kind: String,
    archive_root: Option<String>,
    confirm_replace: bool,
) -> Result<Value, String> {
    request_core_service(
        move || crash_helper::service_restore_repository_archive(
            repo_path, destination_path, snapshot_id, kind, archive_root, confirm_replace,
        ),
        "repository archive restore",
    ).await
}

#[tauri::command]
fn get_default_scan_directories() -> Vec<String> {
    default_scan_roots()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

#[tauri::command]
async fn get_repository_index_generation() -> Result<String, String> {
    request_core_service(
        crash_helper::service_get_repository_index_generation,
        "repository index generation",
    ).await
}

#[tauri::command]
async fn load_indexed_repositories(
    filter_mode: Option<String>,
    state: State<'_, AppState>,
) -> Result<IndexedRepoSummary, String> {
    let requested = filter_mode.unwrap_or_else(|| "grouped".to_string());
    let summary: IndexedRepoSummary = request_core_service(
        move || crash_helper::service_load_indexed_repositories(requested),
        "repository index snapshot",
    ).await?;
    if let Ok(mut cache) = state.cached_repos.lock() {
        *cache = summary.repositories.clone();
    }
    Ok(summary)
}

#[tauri::command]
async fn scan_machine_repositories(
    root_paths: Vec<String>,
    max_depth: Option<usize>,
    include_hidden_dirs: Option<bool>,
    filter_mode: Option<String>,
    index_dependency_repositories: Option<bool>,
    state: State<'_, AppState>,
) -> Result<IndexedRepoSummary, String> {
    // Retain the public command contract; the service reads persisted settings
    // and performs the filesystem walk. The UI only waits for a newer service
    // generation and fetches the published snapshot through authenticated IPC.
    let _requested_settings = (root_paths, max_depth, include_hidden_dirs, index_dependency_repositories);
    let previous_generation = request_core_service::<String, _>(
        crash_helper::service_get_repository_index_generation,
        "repository index generation",
    ).await?
    .parse::<u64>()
    .map_err(|error| format!("Core service returned an invalid repository index generation: {error}"))?;

    crash_helper::request_service_scan()
        .map_err(|error| format!("Could not request repository indexing from gitmanager.service: {error}"))?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        let current_generation = request_core_service::<String, _>(
            crash_helper::service_get_repository_index_generation,
            "repository index generation",
        ).await?
        .parse::<u64>()
        .map_err(|error| format!("Core service returned an invalid repository index generation: {error}"))?;

        if current_generation > previous_generation {
            let requested = filter_mode.clone().unwrap_or_else(|| "grouped".to_string());
            let summary: IndexedRepoSummary = request_core_service(
                move || crash_helper::service_load_indexed_repositories(requested),
                "repository index snapshot",
            ).await?;
            if let Ok(mut cache) = state.cached_repos.lock() {
                *cache = summary.repositories.clone();
            }
            return Ok(summary);
        }

        if std::time::Instant::now() >= deadline {
            return Err(
                "gitmanager.service has not published a refreshed repository index yet. The existing index is preserved; check Task Center → Service for diagnostics."
                    .to_string(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

#[tauri::command]
async fn ensure_required_service(app_handle: AppHandle) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        // Preserve the one-time migration from Tauri's former app-config location.
        // All normal preference reads and writes happen through gitmanager.service.
        let _ = preferences_file(&app_handle)?;
        watcher_service::ensure_required_service(app_handle)
    })
    .await
    .map_err(|error| format!("Core service startup task failed: {error}"))?
}

#[tauri::command]
async fn get_core_service_status() -> Result<crash_helper::ServiceStatus, String> {
    tokio::task::spawn_blocking(crash_helper::request_service_status)
        .await
        .map_err(|error| format!("Core service status task failed: {error}"))?
}

#[tauri::command]
async fn inspect_repository(repo_path: String) -> Result<GitRepositoryInfo, String> {
    let data = tokio::task::spawn_blocking(move || {
        crash_helper::load_service_repository_overview(repo_path)
    })
    .await
    .map_err(|error| format!("Repository overview service request task failed: {error}"))??;
    serde_json::from_value(data)
        .map_err(|error| format!("Core service returned invalid repository overview: {error}"))
}

#[tauri::command]
async fn load_repository_commit_history(
    repo_path: String,
    offset: usize,
    limit: usize,
    branch_filter: Option<String>,
    search_query: Option<String>,
) -> Result<gitmanager_core::models::CommitHistoryPage, String> {
    let data = tokio::task::spawn_blocking(move || {
        crash_helper::load_service_commit_history(
            repo_path,
            offset,
            limit,
            branch_filter,
            search_query,
        )
    })
    .await
    .map_err(|error| format!("Commit history service request task failed: {error}"))??;
    serde_json::from_value(data)
        .map_err(|error| format!("Core service returned invalid commit history: {error}"))
}


#[tauri::command]
async fn list_repository_files(
    repo_path: String,
    relative_path: Option<String>,
    source: Option<String>,
    branch: Option<String>,
) -> Result<Vec<gitmanager_core::models::RepositoryFileEntry>, String> {
    let data = tokio::task::spawn_blocking(move || {
        crash_helper::load_service_repository_files(
            repo_path,
            relative_path,
            source.unwrap_or_else(|| "local".to_string()),
            branch,
        )
    })
    .await
    .map_err(|error| format!("Repository file listing service request task failed: {error}"))??;
    serde_json::from_value(data)
        .map_err(|error| format!("Core service returned an invalid repository file listing: {error}"))
}

#[tauri::command]
async fn read_repository_file(
    repo_path: String,
    relative_path: String,
    source: Option<String>,
    branch: Option<String>,
) -> Result<String, String> {
    let data = tokio::task::spawn_blocking(move || {
        crash_helper::read_service_repository_file(
            repo_path,
            relative_path,
            source.unwrap_or_else(|| "local".to_string()),
            branch,
        )
    })
    .await
    .map_err(|error| format!("Repository file read service request task failed: {error}"))??;
    serde_json::from_value(data)
        .map_err(|error| format!("Core service returned an invalid repository file preview: {error}"))
}

async fn request_core_service<T, F>(
    operation: F,
    description: &'static str,
) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + Send + 'static,
    F: FnOnce() -> Result<serde_json::Value, String> + Send + 'static,
{
    let data = tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| format!("{description} service request task failed: {error}"))??;
    serde_json::from_value(data)
        .map_err(|error| format!("Core service returned invalid {description}: {error}"))
}

#[tauri::command]
async fn save_conflict_resolution(
    repo_path: String, relative_path: String, content: String,
) -> Result<String, String> {
    request_core_service(
        move || crash_helper::service_save_conflict_resolution(repo_path, relative_path, content),
        "conflict resolution",
    ).await
}

#[tauri::command]
async fn preview_repository_pull(
    repo_path: String,
) -> Result<gitmanager_core::models::PullPreview, String> {
    request_core_service(
        move || crash_helper::service_preview_repository_pull(repo_path),
        "pull preview",
    ).await
}

#[tauri::command]
async fn get_repository_changes(
    repo_path: String,
) -> Result<Vec<gitmanager_core::models::WorktreeChange>, String> {
    request_core_service(
        move || crash_helper::service_get_repository_changes(repo_path),
        "repository changes",
    ).await
}

#[tauri::command]
async fn read_repository_file_diff(
    repo_path: String, relative_path: String, staged: bool,
) -> Result<String, String> {
    request_core_service(
        move || crash_helper::service_read_repository_file_diff(repo_path, relative_path, staged),
        "repository diff",
    ).await
}

#[tauri::command]
async fn fetch_repository_remote(
    repo_path: String, remote_name: String,
) -> Result<String, String> {
    request_core_service(
        move || crash_helper::service_fetch_repository_remote(repo_path, remote_name),
        "remote fetch",
    ).await
}

#[tauri::command]
async fn stage_repository_paths(
    repo_path: String, paths: Vec<String>,
) -> Result<Vec<gitmanager_core::models::WorktreeChange>, String> {
    request_core_service(
        move || crash_helper::service_stage_repository_paths(repo_path, paths),
        "stage",
    ).await
}

#[tauri::command]
async fn unstage_repository_paths(
    repo_path: String, paths: Vec<String>,
) -> Result<Vec<gitmanager_core::models::WorktreeChange>, String> {
    request_core_service(
        move || crash_helper::service_unstage_repository_paths(repo_path, paths),
        "unstage",
    ).await
}

#[tauri::command]
async fn preview_repository_commit(
    repo_path: String,
) -> Result<gitmanager_core::models::RepositoryCommitPreview, String> {
    request_core_service(
        move || crash_helper::service_preview_repository_commit(repo_path),
        "commit preview",
    ).await
}

#[tauri::command]
async fn create_repository_commit(
    repo_path: String, message: String, expected_index_tree: String,
) -> Result<(String, Vec<gitmanager_core::models::WorktreeChange>), String> {
    request_core_service(
        move || crash_helper::service_create_repository_commit(repo_path, message, expected_index_tree),
        "commit",
    ).await
}

#[tauri::command]
async fn delete_repository_path(
    repo_path: String, relative_path: String,
) -> Result<String, String> {
    request_core_service(
        move || crash_helper::service_delete_repository_path(repo_path, relative_path),
        "repository deletion",
    ).await
}

#[tauri::command]
async fn prepare_and_pull_repository_update(
    repo_path: String,
    expected_branch: String,
    expected_local_oid: String,
    expected_remote_oid: String,
    expected_status_token: String,
    mode: String,
    stage_paths: Vec<String>,
    commit_message: Option<String>,
) -> Result<gitmanager_core::models::PullExecutionResult, String> {
    request_core_service(
        move || crash_helper::service_prepare_and_pull_repository_update(
            repo_path,
            expected_branch,
            expected_local_oid,
            expected_remote_oid,
            expected_status_token,
            mode,
            stage_paths,
            commit_message.unwrap_or_default(),
        ),
        "pull",
    ).await
}

#[tauri::command]
async fn list_local_repository_branches(
    repo_path: String,
) -> Result<Vec<gitmanager_core::models::BranchInfo>, String> {
    request_core_service(
        move || crash_helper::service_list_local_repository_branches(repo_path),
        "branch list",
    ).await
}

#[tauri::command]
async fn switch_repository_branch(
    repo_path: String, branch_name: String,
) -> Result<GitRepositoryInfo, String> {
    request_core_service(
        move || crash_helper::service_switch_repository_branch(repo_path, branch_name),
        "branch switch",
    ).await
}

#[tauri::command]
async fn detect_github_identity() -> Result<remote_providers::GitHubIdentity, String> {
    request_core_service(
        crash_helper::service_detect_github_identity,
        "GitHub CLI identity",
    ).await
}

/// Return the application version from the compiled Cargo package metadata.
#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
/// Validate a repository web URL before opening it in the system browser.
/// The WebView never receives the shell-open permission.
fn normalize_external_url(raw_url: &str) -> Result<String, String> {
    let mut url = url::Url::parse(raw_url.trim())
        .map_err(|_| "The repository web URL is invalid.".to_string())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("Only HTTPS repository web URLs can be opened.".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Repository URLs containing embedded credentials cannot be opened.".to_string());
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let url = normalize_external_url(&url)?;

    #[cfg(target_os = "windows")]
    {
        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| "Windows SystemRoot is unavailable.".to_string())?;
        let executable = PathBuf::from(system_root).join("System32").join("rundll32.exe");
        hidden_command(executable)
            .arg("url.dll,FileProtocolHandler")
            .arg(&url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not open repository website: {error}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        hidden_command("/usr/bin/open")
            .arg(&url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not open repository website: {error}"))?;
    }

    #[cfg(target_os = "linux")]
    {
        hidden_command("xdg-open")
            .arg(&url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not open repository website: {error}"))?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = url;
        return Err("Opening external URLs is not supported on this platform.".to_string());
    }

    Ok(())
}

/// Report whether this process is the first-run setup companion.
#[tauri::command]
fn is_setup_mode() -> bool {
    std::env::var_os("GITMANAGER_SETUP_MODE").is_some()
        || std::env::args_os().any(|argument| argument == "--setup")
}

/// Start the packaged setup companion and close the bootstrap window.
/// If the sidecar is unavailable, the frontend falls back to its in-process
/// first-run wizard rather than leaving the user without onboarding.
#[tauri::command]
fn launch_first_run_setup(app_handle: AppHandle) -> Result<(), String> {
    // Tauri places sidecars beside the main executable. The helper resolves
    // that sibling itself, so the WebView cannot supply an arbitrary program path.
    let command = app_handle
        .shell()
        .sidecar("binaries/gitmanager-setup")
        .map_err(|error| format!("GitManager Setup helper is unavailable: {error}"))?;

    let (_events, child) = command.spawn()
        .map_err(|error| format!("Could not start GitManager Setup: {error}"))?;

    // Keep the child handle alive until this bootstrap process exits; dropping
    // it here can terminate the just-launched setup process on some platforms.
    std::mem::forget(child);

    app_handle.exit(0);
    Ok(())
}

#[tauri::command]
async fn load_connected_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectedAccountSummary>, String> {
    let payload = tokio::task::spawn_blocking(crash_helper::load_service_account_cache)
        .await
        .map_err(|error| format!("Account metadata service request failed: {error}"))??;
    let records: Vec<gitmanager_core::account_store::StoredAccount> = serde_json::from_value(payload)
        .map_err(|error| format!("Service returned invalid account metadata: {error}"))?;
    let accounts = records
        .into_iter()
        .map(|record| ConnectedAccountSummary {
            provider: record.metadata.provider,
            account_login: record.metadata.account_login,
            display_name: record.metadata.display_name,
            email: record.metadata.email,
            avatar_url: record.metadata.avatar_url,
            profile_url: record.metadata.profile_url,
            session_connected: false,
            repositories: record.repositories,
        })
        .collect::<Vec<_>>();
    if let Ok(mut cached) = state.connected_accounts.lock() {
        *cached = accounts.clone();
    }
    Ok(accounts)
}

#[tauri::command]
async fn connect_git_account(
    provider: String,
    username_or_host: String,
    access_token: Option<String>,
    state: State<'_, AppState>,
) -> Result<ConnectedAccountSummary, String> {
    let payload = serde_json::to_value(ProviderAccountRequest {
        provider,
        username_or_host,
        access_token,
    }).map_err(|error| format!("Could not encode provider account request: {error}"))?;
    let summary: ConnectedAccountSummary = request_core_service(
        move || crash_helper::service_connect_git_account(payload),
        "provider account synchronization",
    ).await?;

    if let Ok(mut accounts) = state.connected_accounts.lock() {
        accounts.retain(|account| {
            !(account.provider == summary.provider
                && account.account_login.eq_ignore_ascii_case(&summary.account_login))
        });
        accounts.push(summary.clone());
    }

    if let Ok(mut cache) = state.cached_repos.lock() {
        for repository in &summary.repositories {
            if !cache.iter().any(|existing| existing.id == repository.id) {
                cache.push(repository.clone());
            }
        }
    }

    Ok(summary)
}

#[tauri::command]
async fn check_repo_updates_now(
    _root_paths: Vec<String>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<RepoUpdateNotification>, String> {
    state.watcher.checking_now.store(true, Ordering::Relaxed);
    let _ = app_handle.emit(
        "gitmanager://update-check-state",
        serde_json::json!({
            "checking": true,
            "status": "Queuing remote check in GitManager Core Service..."
        }),
    );

    let result = tokio::task::spawn_blocking(crash_helper::request_service_remote_check)
        .await
        .map_err(|error| format!("Remote-check request task failed: {error}"))?;

    state.watcher.checking_now.store(false, Ordering::Relaxed);
    let _ = app_handle.emit(
        "gitmanager://update-check-state",
        serde_json::json!({
            "checking": false,
            "queued": result.is_ok()
        }),
    );
    result.map_err(|error| format!("Could not queue remote check in gitmanager.service: {error}"))?;
    Ok(Vec::new())
}

#[tauri::command]
async fn read_service_log_tail() -> Result<String, String> {
    tokio::task::spawn_blocking(|| -> Result<String, String> {
        let service_log = watcher_service::read_service_log_tail()?;
        let incidents = crash_helper::read_service_incident_tail()?;
        Ok(format!(
            "GITMANAGER CORE SERVICE LOG\n{}\n\nCRASH_HELPER INCIDENT HISTORY\n{}",
            service_log.trim_end(),
            incidents.trim_end(),
        ))
    })
    .await
    .map_err(|error| format!("Service diagnostics read task failed: {error}"))?
}

#[tauri::command]
async fn list_app_notifications() -> Result<Vec<notification_center::AppNotification>, String> {
    request_core_service(
        crash_helper::service_list_notifications,
        "notification history",
    ).await
}

#[tauri::command]
async fn mark_app_notification_read(id: String) -> Result<(), String> {
    request_core_service(
        move || crash_helper::service_mark_notification_read(id),
        "notification read update",
    ).await
}

#[tauri::command]
async fn dismiss_app_notification(id: String) -> Result<(), String> {
    request_core_service(
        move || crash_helper::service_dismiss_notification(id),
        "notification dismissal",
    ).await
}

#[tauri::command]
async fn mark_all_app_notifications_read() -> Result<usize, String> {
    request_core_service(
        crash_helper::service_mark_all_notifications_read,
        "bulk notification read update",
    ).await
}

#[tauri::command]
fn report_ui_heartbeat() -> Result<(), String> {
    crash_helper::report_ui_heartbeat_from_env()
}

#[tauri::command]
fn resume_background_service() -> Result<crash_helper::ServiceStatus, String> {
    crash_helper::request_service_resume()
}

#[tauri::command]
async fn control_background_service(
    enable_in_app: Option<bool>,
    install_os_service: Option<bool>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<ServiceStatusInfo, String> {
    if let Some(en) = enable_in_app {
        state.watcher.enabled.store(en, Ordering::Relaxed);
    }

    if install_os_service == Some(true) {
        tokio::task::spawn_blocking(move || register_os_background_service(app_handle))
            .await
            .map_err(|error| format!("Background service registration task failed: {error}"))??;
        state
            .watcher
            .os_service_registered
            .store(true, Ordering::Relaxed);
    }

    let os_kind = if cfg!(target_os = "windows") {
        "Windows Task Scheduler (user logon)"
    } else if cfg!(target_os = "macos") {
        "macOS LaunchAgent (launchd)"
    } else {
        "Linux systemd User Service"
    };

    // Remote-update results now live in the service-owned notification store.
    // Keep this legacy response field empty for older settings clients.
    let pending: Vec<RepoUpdateNotification> = Vec::new();

    Ok(ServiceStatusInfo {
        running_in_background: state.watcher.enabled.load(Ordering::Relaxed),
        checking_now: state.watcher.checking_now.load(Ordering::Relaxed),
        os_service_registered: state.watcher.os_service_registered.load(Ordering::Relaxed),
        os_service_kind: os_kind.to_string(),
        poll_interval_seconds: 120,
        pending_updates: pending,
    })
}



fn resolve_cleaner_script(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let bundled = app_handle
        .path()
        .resolve("tools/gitmanager_cleaner.py", BaseDirectory::Resource)
        .ok();
    if let Some(path) = bundled.as_ref().filter(|path| path.is_file()) {
        return Ok(path.clone());
    }

    // Keep source checkouts usable when running with cargo/tauri dev.
    let current = std::env::current_dir().map_err(|e| e.to_string())?;
    let candidates = [
        current.join("tools/gitmanager_cleaner.py"),
        current.join("../tools/gitmanager_cleaner.py"),
        current.join("../../tools/gitmanager_cleaner.py"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            let checked = bundled.map(|path| path.display().to_string())
                .unwrap_or_else(|| "the Tauri resource directory".to_string());
            format!("Python cleaner resource was not found (checked {}).", checked)
        })
}


#[cfg(target_os = "windows")]
fn resolve_bundled_python(app_handle: &AppHandle) -> Option<PathBuf> {
    // Release installers already extract the pinned runtime into the app's resource
    // directory. Prefer that copy; the per-user data location is a safe migration/
    // development fallback and never a machine-wide Python installation.
    app_handle.path().resolve("python/python.exe", BaseDirectory::Resource)
        .ok().filter(|path| path.is_file())
        .or_else(|| {
            application_directories().ok().map(|directories| {
                PathBuf::from(directories.data_dir).join("runtime").join("python").join("python.exe")
            }).filter(|path| path.is_file())
        })
}

fn run_cleaner_script(app_handle: AppHandle, args: Vec<String>) -> Result<Value, String> {
    let script = resolve_cleaner_script(&app_handle)?;
    #[cfg(target_os = "windows")]
    let mut interpreters: Vec<(String, Vec<String>)> = Vec::new();
    #[cfg(target_os = "windows")]
    if let Some(executable) = resolve_bundled_python(&app_handle) {
        interpreters.push((executable.to_string_lossy().to_string(), vec!["-I".to_string()]));
    }
    #[cfg(target_os = "windows")]
    interpreters.extend([
        ("py".to_string(), vec!["-3.10".to_string(), "-I".to_string()]),
        ("py".to_string(), vec!["-3".to_string(), "-I".to_string()]),
        ("python".to_string(), vec!["-I".to_string()]),
    ]);
    #[cfg(not(target_os = "windows"))]
    let interpreters: Vec<(String, Vec<String>)> = vec![
        ("python3".to_string(), vec![]),
        ("python".to_string(), vec![]),
    ];

    let mut last_error = String::new();
    for (executable, prefix) in interpreters {
        let mut version_command = hidden_command(&executable);
        version_command.args(&prefix)
            .args(["-c", "import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 3)"]);
        let version_check = version_command.output();
        match version_check {
            Ok(result) if result.status.success() => {}
            Ok(_) => {
                last_error = format!("{executable} does not provide Python 3.10 or newer.");
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                last_error = format!("Could not check {executable}: {error}");
                continue;
            }
        }

        let mut run_command = hidden_command(&executable);
        run_command.args(&prefix).arg(&script).args(&args)
            .env("PYTHONUTF8", "1").env("PYTHONDONTWRITEBYTECODE", "1");
        let output = run_command.output();

        let output = match output {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                last_error = format!("Could not start {executable}: {error}");
                continue;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            last_error = if stderr.is_empty() {
                format!("{executable} exited with status {}", output.status)
            } else {
                stderr
            };
            // A Python version too old to parse the helper can fall through to
            // another installed interpreter. Other errors should be reported.
            if last_error.contains("SyntaxError") || last_error.contains("invalid syntax") {
                continue;
            }
            return Err(last_error);
        }

        return serde_json::from_slice::<Value>(&output.stdout)
            .map_err(|e| format!("Cleaner returned invalid JSON: {e}"));
    }

    if last_error.is_empty() {
        last_error = "The bundled Python runtime and compatible system Python interpreters were not found.".to_string();
    }
    Err(last_error)
}

fn cleaner_arguments(
    command: &str,
    scope: &str,
    repo_path: Option<String>,
    remote_url: Option<String>,
    branch: Option<String>,
    include_dependencies: bool,
    include_virtualenvs: bool,
    include_windows_junk: bool,
    confirmation: Option<String>,
) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    match (command, scope) {
        ("plan", "repository") | ("apply", "repository") => {
            args.extend([command.to_string(), "--scope".to_string(), "repository".to_string()]);
            let path = repo_path.filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "Select a local Git repository first.".to_string())?;
            args.extend(["--path".to_string(), path]);
        }
        ("plan", "system") | ("apply", "system") => {
            args.extend([command.to_string(), "--scope".to_string(), "system".to_string()]);
            if include_windows_junk {
                args.push("--include-windows-junk".to_string());
            }
        }
        ("remote-plan", "remote") | ("remote-clean", "remote") => {
            args.push(command.to_string());
            let url = remote_url.filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "The selected repository does not have a usable remote URL.".to_string())?;
            args.extend(["--url".to_string(), url]);
            if let Some(value) = branch.filter(|value| !value.trim().is_empty()) {
                args.extend(["--branch".to_string(), value]);
            }
            if command == "remote-clean" {
                args.push("--push".to_string());
            }
        }
        _ => return Err("Unsupported cleanup scope.".to_string()),
    }

    if scope == "remote" {
        // Remote subcommands include dependencies by default; the remote parser
        // uses the inverse opt-out flag rather than the local --include flag.
        if !include_dependencies {
            args.push("--no-dependencies".to_string());
        }
    } else if include_dependencies {
        args.push("--include-dependencies".to_string());
    }
    if include_virtualenvs {
        args.push("--include-virtualenvs".to_string());
    }
    if let Some(value) = confirmation {
        args.extend(["--confirm".to_string(), value]);
    }
    Ok(args)
}

#[tauri::command]
async fn set_repository_commit_clean(
    app_handle: AppHandle,
    repo_path: String,
    enabled: bool,
) -> Result<Value, String> {
    if repo_path.trim().is_empty() {
        return Err("Select a local Git repository first.".to_string());
    }
    let script = resolve_cleaner_script(&app_handle)?;
    let mut args = vec![
        "commit-hook".to_string(),
        "--path".to_string(),
        repo_path,
    ];
    if enabled {
        args.extend([
            "--script".to_string(),
            script.to_string_lossy().to_string(),
        ]);
    } else {
        args.push("--disable".to_string());
    }

    tokio::task::spawn_blocking(move || run_cleaner_script(app_handle, args))
        .await
        .map_err(|error| format!("Commit Clean hook task failed: {error}"))?
}

#[tauri::command]
async fn cleaner_preview(
    app_handle: AppHandle,
    scope: String,
    repo_path: Option<String>,
    remote_url: Option<String>,
    branch: Option<String>,
    include_dependencies: Option<bool>,
    include_virtualenvs: Option<bool>,
    include_windows_junk: Option<bool>,
) -> Result<Value, String> {
    let args = cleaner_arguments(
        if scope == "remote" { "remote-plan" } else { "plan" },
        &scope,
        repo_path,
        remote_url,
        branch,
        include_dependencies.unwrap_or(false),
        include_virtualenvs.unwrap_or(false),
        include_windows_junk.unwrap_or(false),
        None,
    )?;
    tokio::task::spawn_blocking(move || run_cleaner_script(app_handle, args))
        .await
        .map_err(|e| format!("Cleaner task failed: {e}"))?
}

#[tauri::command]
async fn cleaner_apply(
    app_handle: AppHandle,
    scope: String,
    repo_path: Option<String>,
    remote_url: Option<String>,
    branch: Option<String>,
    include_dependencies: Option<bool>,
    include_virtualenvs: Option<bool>,
    include_windows_junk: Option<bool>,
    confirm_text: String,
) -> Result<Value, String> {
    if confirm_text != "YES" {
        return Err("Type YES exactly to confirm cleanup.".to_string());
    }
    let args = cleaner_arguments(
        if scope == "remote" { "remote-clean" } else { "apply" },
        &scope,
        repo_path,
        remote_url,
        branch,
        include_dependencies.unwrap_or(false),
        include_virtualenvs.unwrap_or(false),
        include_windows_junk.unwrap_or(false),
        Some(confirm_text),
    )?;
    tokio::task::spawn_blocking(move || run_cleaner_script(app_handle, args))
        .await
        .map_err(|e| format!("Cleaner task failed: {e}"))?
}



#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowsSigningCertificate {
    thumbprint: String,
    subject: String,
    not_after: String,
}

#[cfg(target_os = "windows")]
fn run_signing_powershell(script: &str) -> Result<Value, String> {
    let mut command = hidden_command("powershell.exe");
    command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("POWERSHELL_TELEMETRY_OPTOUT", "1");
    let output = command.output().map_err(|error| format!("Could not start PowerShell: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() { format!("PowerShell exited with {}.", output.status) } else { stderr });
    }
    if output.stdout.is_empty() { return Ok(Value::Array(Vec::new())); }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("Certificate query returned invalid JSON: {error}"))
}

fn read_windows_signing_certificates() -> Result<Vec<WindowsSigningCertificate>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
$ErrorActionPreference = 'Stop'
$now = Get-Date
$items = @(Get-ChildItem -Path Cert:\CurrentUser\My | Where-Object {
    $_.HasPrivateKey -and $_.NotBefore -le $now -and $_.NotAfter -gt $now -and
    (@($_.EnhancedKeyUsageList | ForEach-Object { $_.ObjectId.Value }) -contains '1.3.6.1.5.5.7.3.3')
} | ForEach-Object {
    [PSCustomObject]@{ thumbprint = $_.Thumbprint; subject = $_.Subject; notAfter = $_.NotAfter.ToString('yyyy-MM-dd') }
})
ConvertTo-Json -InputObject $items -Compress -Depth 2
"#;
        let value = run_signing_powershell(script)?;
        if value.is_null() { return Ok(Vec::new()); }
        if let Some(items) = value.as_array() {
            return items.iter().cloned()
                .map(|item| serde_json::from_value(item).map_err(|error| format!("Invalid certificate result: {error}")))
                .collect();
        }
        return Ok(vec![serde_json::from_value(value)
            .map_err(|error| format!("Invalid certificate result: {error}"))?]);
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Windows Authenticode signing is available only on Windows.".to_string())
    }
}

#[tauri::command]
async fn list_windows_signing_certificates() -> Result<Vec<WindowsSigningCertificate>, String> {
    tokio::task::spawn_blocking(read_windows_signing_certificates)
        .await
        .map_err(|error| format!("Certificate discovery task failed: {error}"))?
}

#[cfg(target_os = "windows")]
fn find_windows_signtool() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for env_name in ["ProgramFiles(x86)", "ProgramFiles"] {
        let root = match std::env::var_os(env_name) { Some(value) => PathBuf::from(value), None => continue };
        let bin_root = root.join("Windows Kits").join("10").join("bin");
        let entries = match fs::read_dir(bin_root) { Ok(value) => value, Err(_) => continue };
        for entry in entries.flatten() {
            let candidate = entry.path().join("x64").join("signtool.exe");
            if candidate.is_file() { candidates.push(candidate); }
        }
    }
    candidates.sort_by(|left, right| right.to_string_lossy().cmp(&left.to_string_lossy()));
    candidates.into_iter().next()
}

fn sign_windows_installer_blocking(installer_path: String, thumbprint: String) -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    {
        let clean_thumbprint = thumbprint.chars().filter(|character| character.is_ascii_hexdigit()).collect::<String>();
        if clean_thumbprint.len() != 40 {
            return Err("Select a valid SHA-1 thumbprint from the current-user Code Signing certificate store.".to_string());
        }
        let path = fs::canonicalize(&installer_path).map_err(|error| format!("Could not resolve installer file: {error}"))?;
        if !path.is_file() || !matches!(path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase().as_str(), "exe" | "msi") {
            return Err("Choose a Windows .exe or .msi installer file.".to_string());
        }
        let certificates = read_windows_signing_certificates()?;
        if !certificates.iter().any(|certificate| certificate.thumbprint.eq_ignore_ascii_case(&clean_thumbprint)) {
            return Err("The selected certificate is missing, expired, has no private key, or lacks Code Signing usage.".to_string());
        }
        let signtool = find_windows_signtool()
            .ok_or_else(|| "Windows SDK SignTool was not found. Install the Windows 10/11 SDK signing tools.".to_string())?;

        let mut sign = hidden_command(&signtool);
        sign.args(["sign", "/sha1", &clean_thumbprint, "/fd", "SHA256",
            "/tr", "https://timestamp.digicert.com", "/td", "SHA256",
            "/d", "GitManager", "/du", "https://github.com/Kate-alt-69/GitManager"])
            .arg(&path);
        let signed = sign.output().map_err(|error| format!("Could not start SignTool: {error}"))?;
        if !signed.status.success() {
            let stderr = String::from_utf8_lossy(&signed.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&signed.stdout).trim().to_string();
            return Err(format!("SignTool could not sign the installer: {}", if stderr.is_empty() { stdout } else { stderr }));
        }

        let mut verify = hidden_command(&signtool);
        verify.args(["verify", "/pa", "/v"]).arg(&path);
        let verified = verify.output().map_err(|error| format!("Could not verify signed installer: {error}"))?;
        let stdout = String::from_utf8_lossy(&verified.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&verified.stderr).trim().to_string();
        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
            "thumbprint": clean_thumbprint,
            "signatureCreated": true,
            "chainTrusted": verified.status.success(),
            "verification": if stdout.is_empty() { stderr } else { stdout }
        }))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (installer_path, thumbprint);
        Err("Windows Authenticode signing is available only on Windows.".to_string())
    }
}


#[tauri::command]
async fn sign_windows_installer(installer_path: String, thumbprint: String) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || sign_windows_installer_blocking(installer_path, thumbprint))
        .await
        .map_err(|error| format!("Installer signing task failed: {error}"))?
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessUsageSummary {
    average_cpu_percent: f32,
    peak_cpu_percent: f32,
    average_memory_bytes: u64,
    peak_memory_bytes: u64,
    process_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IdlePerformanceCapture {
    sample_duration_seconds: u64,
    sample_count: usize,
    logical_processors: usize,
    captured_at_unix_ms: u128,
    ui_process_tree: ProcessUsageSummary,
    service_process_tree: Option<ProcessUsageSummary>,
    service_generation: u32,
    service_protocol_version: u32,
    notes: Vec<String>,
}

fn collect_process_tree(
    system: &System,
    root: Pid,
    excluded: &HashSet<Pid>,
) -> HashSet<Pid> {
    let mut collected = HashSet::new();
    if system.process(root.clone()).is_none() || excluded.contains(&root) {
        return collected;
    }
    collected.insert(root);

    loop {
        let additions = system.processes()
            .iter()
            .filter_map(|(pid, process)| {
                if collected.contains(pid) || excluded.contains(pid) {
                    return None;
                }
                process.parent()
                    .filter(|parent| collected.contains(parent))
                    .map(|_| pid.clone())
            })
            .collect::<Vec<_>>();
        if additions.is_empty() {
            break;
        }
        collected.extend(additions);
    }

    collected
}

fn read_process_group(
    system: &System,
    pids: &HashSet<Pid>,
    logical_processors: f32,
) -> (f32, u64) {
    let mut cpu_percent = 0.0f32;
    let mut memory_bytes = 0u64;
    for pid in pids {
        if let Some(process) = system.process(pid.clone()) {
            cpu_percent += process.cpu_usage() / logical_processors.max(1.0);
            memory_bytes = memory_bytes.saturating_add(process.memory());
        }
    }
    (cpu_percent, memory_bytes)
}

fn capture_idle_performance_sync() -> Result<IdlePerformanceCapture, String> {
    const SAMPLE_INTERVAL: Duration = Duration::from_secs(3);
    const SAMPLE_COUNT: usize = 20;

    let service_status = crash_helper::request_service_status()
        .map_err(|error| format!("The required core service is unavailable: {error}"))?;
    if service_status.service_generation < crash_helper::SERVICE_GENERATION
        || service_status.protocol_version < crash_helper::SERVICE_PROTOCOL_VERSION
    {
        return Err("The required GitManager core service is outdated. Restart the service before measuring idle performance.".to_string());
    }
    let service_pid = (service_status.process_id > 0)
        .then(|| Pid::from_u32(service_status.process_id));

    let logical_processors = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let logical_processors_f32 = logical_processors as f32;

    // Process enumeration is deliberately done only after an explicit user
    // action. Normal application startup and idle operation do not use sysinfo.
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let service_processes = service_pid
        .map(|pid| collect_process_tree(&system, pid, &HashSet::new()))
        .unwrap_or_default();
    if service_processes.is_empty() {
        return Err("The core service process was not found in the operating-system process table.".to_string());
    }
    let ui_root = Pid::from_u32(std::process::id());
    let ui_processes = collect_process_tree(&system, ui_root, &service_processes);
    if ui_processes.is_empty() {
        return Err("Could not find the GitManager UI process in the operating-system process table.".to_string());
    }

    let mut tracked = ui_processes.iter().cloned().collect::<HashSet<_>>();
    tracked.extend(service_processes.iter().cloned());
    let tracked = tracked.into_iter().collect::<Vec<_>>();

    // Prime the CPU counters before collecting the 60-second window.
    system.refresh_processes(ProcessesToUpdate::Some(&tracked), true);
    std::thread::sleep(Duration::from_millis(500));
    system.refresh_processes(ProcessesToUpdate::Some(&tracked), true);

    let mut ui_cpu_total = 0.0f32;
    let mut ui_cpu_peak = 0.0f32;
    let mut ui_memory_total = 0u128;
    let mut ui_memory_peak = 0u64;
    let mut service_cpu_total = 0.0f32;
    let mut service_cpu_peak = 0.0f32;
    let mut service_memory_total = 0u128;
    let mut service_memory_peak = 0u64;

    for _ in 0..SAMPLE_COUNT {
        std::thread::sleep(SAMPLE_INTERVAL);
        system.refresh_processes(ProcessesToUpdate::Some(&tracked), true);

        let (ui_cpu, ui_memory) =
            read_process_group(&system, &ui_processes, logical_processors_f32);
        ui_cpu_total += ui_cpu;
        ui_cpu_peak = ui_cpu_peak.max(ui_cpu);
        ui_memory_total += u128::from(ui_memory);
        ui_memory_peak = ui_memory_peak.max(ui_memory);

        if !service_processes.is_empty() {
            let (service_cpu, service_memory) =
                read_process_group(&system, &service_processes, logical_processors_f32);
            service_cpu_total += service_cpu;
            service_cpu_peak = service_cpu_peak.max(service_cpu);
            service_memory_total += u128::from(service_memory);
            service_memory_peak = service_memory_peak.max(service_memory);
        }
    }

    let ui_summary = ProcessUsageSummary {
        average_cpu_percent: ui_cpu_total / SAMPLE_COUNT as f32,
        peak_cpu_percent: ui_cpu_peak,
        average_memory_bytes: (ui_memory_total / SAMPLE_COUNT as u128) as u64,
        peak_memory_bytes: ui_memory_peak,
        process_count: ui_processes.len(),
    };
    let service_summary = if service_processes.is_empty() {
        None
    } else {
        Some(ProcessUsageSummary {
            average_cpu_percent: service_cpu_total / SAMPLE_COUNT as f32,
            peak_cpu_percent: service_cpu_peak,
            average_memory_bytes: (service_memory_total / SAMPLE_COUNT as u128) as u64,
            peak_memory_bytes: service_memory_peak,
            process_count: service_processes.len(),
        })
    };

    Ok(IdlePerformanceCapture {
        sample_duration_seconds: SAMPLE_INTERVAL.as_secs() * SAMPLE_COUNT as u64,
        sample_count: SAMPLE_COUNT,
        logical_processors,
        captured_at_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        ui_process_tree: ui_summary,
        service_process_tree: service_summary,
        service_generation: service_status.service_generation,
        service_protocol_version: service_status.protocol_version,
        notes: vec![
            "CPU percentages are normalized across all logical processors.".to_string(),
            "The UI group includes the Tauri process and descendant processes visible in the process tree.".to_string(),
            "Close active scans, account sync and large diffs before starting an idle capture.".to_string(),
        ],
    })
}

#[tauri::command]
async fn capture_idle_performance() -> Result<IdlePerformanceCapture, String> {
    tokio::task::spawn_blocking(capture_idle_performance_sync)
        .await
        .map_err(|error| format!("Performance capture task failed: {error}"))?
}

/// Keeps low-frequency health and file-change monitoring outside the WebView.
/// The frontend sleeps until one of these native events reports a real change.
fn start_native_runtime_monitor(app_handle: AppHandle) {
    let heartbeat_spawn = std::thread::Builder::new()
        .name("gitmanager-native-supervisor-heartbeat".to_string())
        .spawn(|| {
            loop {
                std::thread::sleep(Duration::from_secs(3));
                // This updates the native supervisor heartbeat directly;
                // it no longer requires waking the React/WebView runtime.
                let _ = crash_helper::report_ui_heartbeat_from_env();
            }
        });
    if let Err(error) = heartbeat_spawn {
        eprintln!("GitManager native supervisor heartbeat could not start: {error}");
    }

    let monitor_spawn = std::thread::Builder::new()
        .name("gitmanager-native-runtime-monitor".to_string())
        .spawn(move || {
            let mut index_generation =
                repository_index::repository_index_generation().unwrap_or_default();
            let mut notification_generation =
                notification_center::notification_store_modified_unix_nanos().unwrap_or_default();
            let mut last_service_check = Instant::now();
            let mut last_service_state: Option<(bool, u32, u32, u32, bool, bool, bool)> = None;

            loop {
                std::thread::sleep(Duration::from_secs(1));

                if let Ok(generation) = repository_index::repository_index_generation() {
                    if generation != index_generation {
                        index_generation = generation;
                        let _ = app_handle.emit(
                            "gitmanager://repository-index-updated",
                            generation.to_string(),
                        );
                    }
                }

                if let Ok(generation) =
                    notification_center::notification_store_modified_unix_nanos()
                {
                    if generation != notification_generation {
                        notification_generation = generation;
                        let _ = app_handle.emit("gitmanager://notifications-updated", ());
                    }
                }

                if last_service_check.elapsed() >= Duration::from_secs(5) {
                    let (ready, generation, process_id, protocol_version, remote_check_pending, remote_check_running, indexing, message) =
                        match crash_helper::request_service_status() {
                            Ok(status) => (
                                status.service_generation >= crash_helper::SERVICE_GENERATION
                                    && status.protocol_version >= crash_helper::SERVICE_PROTOCOL_VERSION,
                                status.service_generation,
                                status.process_id,
                                status.protocol_version,
                                status.remote_check_pending,
                                status.remote_check_running,
                                status.indexing,
                                status.message,
                            ),
                            Err(error) => (false, 0, 0, 0, false, false, false, error),
                        };

                    let changed = last_service_state
                        .map(|(previous_ready, previous_generation, previous_pid, previous_protocol, previous_remote_pending, previous_remote_running, previous_indexing)| {
                            previous_ready != ready
                                || previous_generation != generation
                                || previous_pid != process_id
                                || previous_protocol != protocol_version
                                || previous_remote_pending != remote_check_pending
                                || previous_remote_running != remote_check_running
                                || previous_indexing != indexing
                        })
                        .unwrap_or(true);
                    if changed {
                        let status_message = if remote_check_running {
                            "Checking remote repository branches…".to_string()
                        } else if remote_check_pending {
                            "Remote repository check queued".to_string()
                        } else {
                            message
                        };
                        let _ = app_handle.emit(
                            "gitmanager://service-health",
                            serde_json::json!({
                                "ready": ready,
                                "generation": generation,
                                "processId": process_id,
                                "protocolVersion": protocol_version,
                                "remoteCheckPending": remote_check_pending,
                                "remoteCheckRunning": remote_check_running,
                                "indexing": indexing,
                                "message": status_message,
                            }),
                        );
                        last_service_state = Some((
                            ready, generation, process_id, protocol_version,
                            remote_check_pending, remote_check_running, indexing,
                        ));
                    }
                    last_service_check = Instant::now();
                }
            }
        });

    if let Err(error) = monitor_spawn {
        eprintln!("GitManager native runtime monitor could not start: {error}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            // Discovery, remote polling and indexing stay in the service.
            // This native monitor only relays lightweight state changes to
            // the WebView and maintains the crash-supervisor heartbeat.
            start_native_runtime_monitor(app.handle().clone());
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            check_cli_tools,
            exit_application,
            get_flatpak_cli_integration_status,
            install_flatpak_gitm_cli,
            remove_flatpak_gitm_cli,
            get_default_scan_directories,
            get_default_archive_directory,
            get_archive_cloud_storage_info,
            configure_archive_cloud_storage,
            disable_archive_cloud_storage,
            sync_archive_cloud_storage,
            check_rbe_node_status,
            save_cloud_node_address,
            capture_repository_archive,
            list_repository_archives,
            restore_repository_archive,
            get_application_directories,
            load_app_preferences,
            save_app_preferences,
            load_repository_settings,
            save_repository_settings,
            get_commit_details,
            list_windows_signing_certificates,
            sign_windows_installer,
            get_repository_index_generation,
            load_indexed_repositories,
            scan_machine_repositories,
            ensure_required_service,
            get_core_service_status,
            inspect_repository,
            load_repository_commit_history,
            capture_idle_performance,
            list_repository_files,
            read_repository_file,
            save_conflict_resolution,
            preview_repository_pull,
            prepare_and_pull_repository_update,
            get_repository_changes,
            read_repository_file_diff,
            fetch_repository_remote,
            stage_repository_paths,
            unstage_repository_paths,
            preview_repository_commit,
            create_repository_commit,
            delete_repository_path,
            list_local_repository_branches,
            switch_repository_branch,
            detect_github_identity,
            get_app_version,
            open_external_url,
            is_setup_mode,
            launch_first_run_setup,
            load_connected_accounts,
            connect_git_account,
            check_repo_updates_now,
            read_service_log_tail,
            list_app_notifications,
            mark_app_notification_read,
            dismiss_app_notification,
            mark_all_app_notifications_read,
            control_background_service,
            report_ui_heartbeat,
            resume_background_service,
            cleaner_preview,
            cleaner_apply,
            set_repository_commit_clean
        ])
        .run(tauri::generate_context!())
        .expect("error while running GitManager Tauri v2 application");
}

#[cfg(test)]
mod rbe_node_address_tests {
    use super::{validate_rbe_node_base_url, AppPreferences};

    #[test]
    fn older_preferences_without_a_saved_node_address_remain_compatible() {
        let preferences: AppPreferences = serde_json::from_str("{}").unwrap();
        assert!(preferences.cloud_node_address.is_empty());
    }

    #[test]
    fn a_saved_node_address_round_trips_with_preferences() {
        let mut preferences = AppPreferences::default();
        preferences.cloud_node_address = "http://192.168.1.50:8080/".to_string();
        let encoded = serde_json::to_string(&preferences).unwrap();
        let decoded: AppPreferences = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.cloud_node_address, preferences.cloud_node_address);
    }

    #[test]
    fn accepts_private_lan_ipv4_and_defaults_to_backend_port() {
        let address = validate_rbe_node_base_url("192.168.1.50").unwrap();
        assert_eq!(address.as_str(), "http://192.168.1.50:8080/");
    }

    #[test]
    fn accepts_explicit_lan_port_and_ipv6_unique_local_addresses() {
        let address = validate_rbe_node_base_url("http://10.0.0.12:8090").unwrap();
        assert_eq!(address.port(), Some(8090));
        assert!(validate_rbe_node_base_url("http://[fd12::20]:8080").is_ok());
    }

    #[test]
    fn rejects_public_hosts_non_ip_names_and_port_zero() {
        assert!(validate_rbe_node_base_url("https://8.8.8.8").is_err());
        assert!(validate_rbe_node_base_url("nas.example.com:8080").is_err());
        assert!(validate_rbe_node_base_url("192.168.1.50:0").is_err());
    }

    #[test]
    fn rejects_credentials_paths_queries_and_fragments() {
        assert!(validate_rbe_node_base_url("http://user:pass@192.168.1.50").is_err());
        assert!(validate_rbe_node_base_url("http://192.168.1.50/api").is_err());
        assert!(validate_rbe_node_base_url("http://192.168.1.50?token=secret").is_err());
        assert!(validate_rbe_node_base_url("http://192.168.1.50/#health").is_err());
    }
}

#[cfg(test)]
mod external_url_tests {
    use super::normalize_external_url;

    #[test]
    fn external_urls_require_https_without_credentials() {
        assert!(normalize_external_url("http://github.com/org/repo").is_err());
        assert!(normalize_external_url("javascript:alert(1)").is_err());
        assert!(normalize_external_url("https://user:token@github.com/org/repo").is_err());
    }

    #[test]
    fn external_urls_drop_query_and_fragment_data() {
        assert_eq!(
            normalize_external_url("https://github.com/org/repo?token=secret#settings").unwrap(),
            "https://github.com/org/repo"
        );
    }
}


#[cfg(test)]
mod application_path_tests {
    use super::application_directory_layout;
    use std::path::{Path, PathBuf};

    #[test]
    fn install_config_and_data_share_the_kastrick_product_root() {
        let local_data = Path::new("local-app-data");
        let paths = application_directory_layout(local_data);
        let product_root = local_data.join("kastrick's_software");

        assert_eq!(PathBuf::from(paths.software_root), product_root);
        assert_eq!(PathBuf::from(paths.install_dir), product_root.join("GitManager"));
        assert_eq!(PathBuf::from(paths.config_dir), product_root.join("config").join("GitManager"));
        let data = product_root.join("data").join("GitManager");
        assert_eq!(PathBuf::from(paths.data_dir), data);
        assert_eq!(PathBuf::from(paths.repo_metadata_dir), data.join("repo").join("metadata"));
        assert_eq!(PathBuf::from(paths.git_data_dir), data.join("git_data"));
        assert_eq!(PathBuf::from(paths.account_metadata_dir), data.join("account").join("metadata"));
        assert_eq!(PathBuf::from(paths.account_git_data_dir), data.join("account").join("git_data"));
        assert_eq!(PathBuf::from(paths.remote_repo_metadata_dir), data.join("repo").join("remote").join("metadata"));
        assert_eq!(PathBuf::from(paths.remote_repo_git_data_dir), data.join("repo").join("remote").join("git_data"));
        assert_eq!(PathBuf::from(paths.cache_dir), data.join(".cache"));
    }
}
