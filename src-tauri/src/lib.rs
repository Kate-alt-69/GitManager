pub mod git_info;
pub mod remote_providers;
pub mod scanner;
pub mod watcher_service;

use std::path::{Path, PathBuf};
use std::process::Command;
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
    state: State<'_, AppState>,
) -> Result<IndexedRepoSummary, String> {
    let local_repos = scan_repositories(ScanOptions {
        root_paths,
        max_depth,
        include_hidden_dirs,
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
        .map_err(|e| format!("Could not resolve the bundled cleaner: {e}"))?;
    if bundled.is_file() {
        return Ok(bundled);
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
        .ok_or_else(|| format!("Python cleaner resource was not found (checked {}).", bundled.display()))
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
        let output = Command::new(&executable)
            .args(prefix)
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

    if include_dependencies {
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
            spawn_background_update_watcher(
                app.handle().clone(),
                state.watcher.enabled.clone(),
                state.watcher.checking_now.clone(),
            );
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            get_default_scan_directories,
            scan_machine_repositories,
            inspect_repository,
            connect_git_account,
            check_repo_updates_now,
            pull_repository_update,
            control_background_service,
            cleaner_preview,
            cleaner_apply
        ])
        .run(tauri::generate_context!())
        .expect("error while running GitManager Tauri v2 application");
}
