pub mod git_info;
pub mod remote_providers;
pub mod scanner;
pub mod watcher_service;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::path::BaseDirectory;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

use git_info::{extract_git_repository_info, GitRepositoryInfo};
use remote_providers::{
    fetch_remote_account_repositories, ConnectedAccountSummary, ProviderAccountRequest,
};
use scanner::{
    build_indexed_summary, default_scan_roots, scan_repositories, IndexedRepoSummary, ScanOptions,
};
use watcher_service::{
    apply_repository_update, check_all_repos_for_updates, register_os_background_service,
    send_os_desktop_notification, spawn_background_update_watcher, BackgroundWatcherControl,
    RepoUpdateNotification, ServiceStatusInfo,
};

#[derive(Default)]
pub struct AppState {
    pub cached_repos: Mutex<Vec<GitRepositoryInfo>>,
    pub connected_accounts: Mutex<Vec<ConnectedAccountSummary>>,
    pub pending_updates: Mutex<Vec<RepoUpdateNotification>>,
    pub watcher: BackgroundWatcherControl,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryPreferences {
    pub auto_update: bool,
    pub keep_remote_clean: bool,
    pub commit_clean: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppPreferences {
    pub scan_roots: Vec<String>,
    pub max_scan_depth: usize,
    pub include_hidden_dirs: bool,
    pub index_dependency_repositories: bool,
    pub show_dependency_repositories: bool,
    pub repository_settings: HashMap<String, RepositoryPreferences>,
    pub first_run_complete: bool,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            scan_roots: default_scan_roots()
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            max_scan_depth: 8,
            include_hidden_dirs: false,
            index_dependency_repositories: true,
            show_dependency_repositories: false,
            repository_settings: HashMap::new(),
            first_run_complete: false,
        }
    }
}

fn preferences_file(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let directory = app_handle
        .path()
        .app_config_dir()
        .map_err(|error| format!("Could not resolve GitManager config directory: {error}"))?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create GitManager config directory: {error}"))?;
    Ok(directory.join("preferences.json"))
}

#[tauri::command]
fn load_app_preferences(app_handle: AppHandle) -> Result<AppPreferences, String> {
    let path = preferences_file(&app_handle)?;
    if !path.is_file() {
        return Ok(AppPreferences::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read GitManager preferences: {error}"))?;
    if content.len() > 256 * 1024 {
        return Err("GitManager preferences file is unexpectedly large.".to_string());
    }

    let mut preferences: AppPreferences = serde_json::from_str(&content)
        .map_err(|error| format!("GitManager preferences are invalid JSON: {error}"))?;
    // Keep disconnected drive roots in the saved setup so removable/external
    // drives can be scanned again when they are mounted.
    let mut retained_roots = Vec::new();
    for raw_root in preferences.scan_roots.drain(..).take(32) {
        let root = raw_root.trim();
        if root.is_empty() {
            continue;
        }
        let value = fs::canonicalize(root)
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|_| root.to_string());
        if !retained_roots.iter().any(|existing: &String| existing.eq_ignore_ascii_case(&value)) {
            retained_roots.push(value);
        }
    }
    preferences.scan_roots = retained_roots;
    preferences.max_scan_depth = preferences.max_scan_depth.clamp(2, 16);
    Ok(preferences)
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
fn save_app_preferences(
    app_handle: AppHandle,
    mut preferences: AppPreferences,
) -> Result<AppPreferences, String> {
    preferences.scan_roots = normalize_scan_roots(preferences.scan_roots)?;
    if !(2..=16).contains(&preferences.max_scan_depth) {
        return Err("Scan depth must be between 2 and 16.".to_string());
    }

    let path = preferences_file(&app_handle)?;
    let temporary = path.with_extension("json.tmp");
    let content = serde_json::to_vec_pretty(&preferences)
        .map_err(|error| format!("Could not serialize GitManager preferences: {error}"))?;
    fs::write(&temporary, content)
        .map_err(|error| format!("Could not write GitManager preferences: {error}"))?;
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("Could not replace GitManager preferences: {error}"))?;
    }
    fs::rename(&temporary, &path)
        .map_err(|error| format!("Could not save GitManager preferences: {error}"))?;
    Ok(preferences)
}

#[tauri::command]
fn get_default_scan_directories() -> Vec<String> {
    default_scan_roots()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

#[tauri::command]
fn scan_machine_repositories(
    root_paths: Vec<String>,
    max_depth: Option<usize>,
    include_hidden_dirs: Option<bool>,
    filter_mode: Option<String>,
    index_dependency_repositories: Option<bool>,
    state: State<'_, AppState>,
) -> Result<IndexedRepoSummary, String> {
    let local_repos = scan_repositories(ScanOptions {
        root_paths,
        max_depth,
        include_hidden_dirs,
        index_dependency_repositories,
    })?;

    let mut combined = local_repos;
    if let Ok(accounts) = state.connected_accounts.lock() {
        for acc in accounts.iter() {
            for r in &acc.repositories {
                let already_cloned = combined.iter().any(|local| {
                    local.web_url.as_deref() == r.web_url.as_deref() && r.web_url.is_some()
                });
                if !already_cloned {
                    combined.push(r.clone());
                }
            }
        }
    }

    if let Ok(mut cache) = state.cached_repos.lock() {
        *cache = combined.clone();
    }

    let requested = filter_mode.unwrap_or_else(|| "grouped".to_string());
    Ok(build_indexed_summary(combined, &requested))
}

#[tauri::command]
fn inspect_repository(repo_path: String) -> Result<GitRepositoryInfo, String> {
    let workdir = Path::new(&repo_path);
    let git_dir = workdir.join(".git");
    extract_git_repository_info(workdir, &git_dir)
}

#[tauri::command]
async fn connect_git_account(
    provider: String,
    username_or_host: String,
    access_token: Option<String>,
    state: State<'_, AppState>,
) -> Result<ConnectedAccountSummary, String> {
    let summary = fetch_remote_account_repositories(ProviderAccountRequest {
        provider,
        username_or_host,
        access_token,
    })
    .await?;

    if let Ok(mut accounts) = state.connected_accounts.lock() {
        accounts.retain(|a| {
            !(a.provider == summary.provider
                && a.account_login.eq_ignore_ascii_case(&summary.account_login))
        });
        accounts.push(summary.clone());
    }

    if let Ok(mut cache) = state.cached_repos.lock() {
        for r in &summary.repositories {
            let exists = cache.iter().any(|existing| existing.id == r.id);
            if !exists {
                cache.push(r.clone());
            }
        }
    }

    Ok(summary)
}

#[tauri::command]
async fn check_repo_updates_now(
    root_paths: Vec<String>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<RepoUpdateNotification>, String> {
    state.watcher.checking_now.store(true, Ordering::Relaxed);
    let _ = app_handle.emit(
        "gitmanager://update-check-state",
        serde_json::json!({
            "checking": true,
            "status": "Checking remote repositories for updates..."
        }),
    );

    let updates = tokio::task::spawn_blocking(move || check_all_repos_for_updates(root_paths))
        .await
        .map_err(|e| e.to_string())?;

    state.watcher.checking_now.store(false, Ordering::Relaxed);
    let _ = app_handle.emit(
        "gitmanager://update-check-state",
        serde_json::json!({
            "checking": false,
            "updates_count": updates.len()
        }),
    );

    if let Ok(mut pending) = state.pending_updates.lock() {
        *pending = updates.clone();
    }

    for upd in &updates {
        send_os_desktop_notification(upd);
        let _ = app_handle.emit("gitmanager://repo-update-available", upd);
    }

    Ok(updates)
}

#[tauri::command]
async fn pull_repository_update(
    repo_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let path_clone = repo_path.clone();
    let result = tokio::task::spawn_blocking(move || apply_repository_update(Path::new(&path_clone)))
        .await
        .map_err(|e| e.to_string())??;

    if let Ok(mut pending) = state.pending_updates.lock() {
        pending.retain(|u| u.repo_path != repo_path);
    }

    Ok(result)
}

#[tauri::command]
fn control_background_service(
    enable_in_app: Option<bool>,
    install_os_service: Option<bool>,
    state: State<'_, AppState>,
) -> Result<ServiceStatusInfo, String> {
    if let Some(en) = enable_in_app {
        state.watcher.enabled.store(en, Ordering::Relaxed);
    }

    if install_os_service == Some(true) {
        let _ = register_os_background_service()?;
        state
            .watcher
            .os_service_registered
            .store(true, Ordering::Relaxed);
    }

    let os_kind = if cfg!(target_os = "windows") {
        "Windows Service (svchost / sc.exe)"
    } else if cfg!(target_os = "macos") {
        "macOS LaunchAgent (launchd)"
    } else {
        "Linux systemd User Service"
    };

    let pending = state
        .pending_updates
        .lock()
        .map(|p| p.clone())
        .unwrap_or_default();

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

fn run_cleaner_script(app_handle: AppHandle, args: Vec<String>) -> Result<Value, String> {
    let script = resolve_cleaner_script(&app_handle)?;
    #[cfg(target_os = "windows")]
    let interpreters: Vec<(String, Vec<String>)> = vec![
        ("py".to_string(), vec!["-3.10".to_string()]),
        ("py".to_string(), vec!["-3".to_string()]),
        ("python".to_string(), vec![]),
    ];
    #[cfg(not(target_os = "windows"))]
    let interpreters: Vec<(String, Vec<String>)> = vec![
        ("python3".to_string(), vec![]),
        ("python".to_string(), vec![]),
    ];

    let mut last_error = String::new();
    for (executable, prefix) in interpreters {
        let version_check = Command::new(&executable)
            .args(&prefix)
            .args(["-c", "import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 3)"])
            .output();
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

        let output = Command::new(&executable)
            .args(&prefix)
            .arg(&script)
            .args(&args)
            .env("PYTHONUTF8", "1")
            .output();

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
        last_error = "Python 3.10 or newer was not found in PATH (tried py -3.10, py -3, python3, and python).".to_string();
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


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            let preferences = load_app_preferences(app.handle().clone())
                .unwrap_or_else(|_| AppPreferences::default());
            spawn_background_update_watcher(
                app.handle().clone(),
                state.watcher.enabled.clone(),
                state.watcher.checking_now.clone(),
                preferences.scan_roots,
            );
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            get_default_scan_directories,
            load_app_preferences,
            save_app_preferences,
            scan_machine_repositories,
            inspect_repository,
            connect_git_account,
            check_repo_updates_now,
            pull_repository_update,
            control_background_service,
            cleaner_preview,
            cleaner_apply,
            set_repository_commit_clean
        ])
        .run(tauri::generate_context!())
        .expect("error while running GitManager Tauri v2 application");
}
