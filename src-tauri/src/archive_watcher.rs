//! Per-repository archive capture triggers.
//!
//! Only discovered Git working directories are watched. The module deliberately
//! avoids watching entire disks or dependency trees directly.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::archive;
use crate::scanner::{scan_repositories, ScanOptions};
use crate::AppPreferences;

static WATCH_GENERATION: AtomicU64 = AtomicU64::new(0);
const MAX_WATCHED_REPOSITORIES: usize = 256;
const COMMIT_SETTLE: Duration = Duration::from_millis(180);

pub fn restart_archive_watchers(preferences: &AppPreferences) {
    let generation = WATCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if !preferences.first_run_complete
        || preferences.archive_trigger == "manual"
        || (!preferences.archive_local_enabled && !preferences.archive_remote_enabled)
    {
        return;
    }

    let roots = preferences.scan_roots.clone();
    let max_depth = preferences.max_scan_depth;
    let include_hidden = preferences.include_hidden_dirs;
    let trigger = preferences.archive_trigger.clone();
    let quiet_ms = preferences.archive_quiet_ms.clamp(250, 60_000);
    let archive_root = preferences.archive_root.clone();
    let local_enabled = preferences.archive_local_enabled;
    let remote_enabled = preferences.archive_remote_enabled;

    thread::Builder::new()
        .name("gitmanager-archive-discovery".to_string())
        .spawn(move || {
            if generation != WATCH_GENERATION.load(Ordering::SeqCst) {
                return;
            }
            let repositories = match scan_repositories(ScanOptions {
                root_paths: roots,
                max_depth: Some(max_depth.clamp(2, 16)),
                include_hidden_dirs: Some(include_hidden),
                index_dependency_repositories: Some(true),
            }) {
                Ok(repositories) => repositories,
                Err(error) => {
                    eprintln!("GitManager archive watcher: repository discovery failed: {error}");
                    return;
                }
            };

            for repository in repositories.into_iter().take(MAX_WATCHED_REPOSITORIES) {
                if generation != WATCH_GENERATION.load(Ordering::SeqCst) {
                    break;
                }
                let path = repository.path.clone();
                let git_dir = repository.git_dir.clone();
                let trigger = trigger.clone();
                let archive_root = archive_root.clone();
                thread::Builder::new()
                    .name(format!("gitmanager-archive-{}", repository.name))
                    .spawn(move || {
                        watch_repository(
                            generation,
                            path,
                            git_dir,
                            trigger,
                            quiet_ms,
                            archive_root,
                            local_enabled,
                            remote_enabled,
                        );
                    })
                    .ok();
            }
        })
        .ok();
}

fn watch_repository(
    generation: u64,
    repository_path: String,
    git_dir: String,
    trigger: String,
    quiet_ms: u64,
    archive_root: String,
    local_enabled: bool,
    remote_enabled: bool,
) {
    let watch_path = if trigger == "commit" {
        std::path::PathBuf::from(git_dir)
    } else {
        std::path::PathBuf::from(&repository_path)
    };
    if !watch_path.is_dir() {
        return;
    }

    let (sender, receiver) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = match notify::recommended_watcher(move |event: notify::Result<Event>| {
        let _ = sender.send(event);
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("GitManager archive watcher: could not watch {}: {error}", watch_path.display());
            return;
        }
    };
    if let Err(error) = watcher.watch(&watch_path, RecursiveMode::Recursive) {
        eprintln!("GitManager archive watcher: could not register {}: {error}", watch_path.display());
        return;
    }

    let mut last_change = None::<Instant>;
    let mut pending_commit = None::<Instant>;
    let mut last_head = current_head(&repository_path);

    while generation == WATCH_GENERATION.load(Ordering::SeqCst) {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(event)) => {
                if matches!(event.kind, EventKind::Access(_)) {
                    continue;
                }
                if trigger == "change" {
                    last_change = Some(Instant::now());
                } else {
                    pending_commit = Some(Instant::now() + COMMIT_SETTLE);
                }
            }
            Ok(Err(error)) => {
                eprintln!("GitManager archive watcher: event error for {repository_path}: {error}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if trigger == "change"
            && last_change.is_some_and(|last| last.elapsed() >= Duration::from_millis(quiet_ms))
        {
            last_change = None;
            capture_enabled_archives(
                &repository_path,
                &archive_root,
                local_enabled,
                remote_enabled,
            );
        }

        if trigger == "commit"
            && pending_commit.is_some_and(|deadline| Instant::now() >= deadline)
        {
            pending_commit = None;
            let current = current_head(&repository_path);
            if current.is_some() && current != last_head {
                last_head = current;
                capture_enabled_archives(
                    &repository_path,
                    &archive_root,
                    local_enabled,
                    remote_enabled,
                );
            }
        }
    }
}

fn current_head(repository_path: &str) -> Option<String> {
    git2::Repository::open(repository_path)
        .ok()?
        .head()
        .ok()?
        .target()
        .map(|oid| oid.to_string())
}

fn capture_enabled_archives(
    repository_path: &str,
    archive_root: &str,
    local_enabled: bool,
    remote_enabled: bool,
) {
    if local_enabled {
        if let Err(error) = archive::capture_repository(
            repository_path,
            Some(archive_root),
            "local",
        ) {
            eprintln!("GitManager Local Archive for {repository_path}: {error}");
        }
    }
    if remote_enabled {
        if let Err(error) = archive::capture_repository(
            repository_path,
            Some(archive_root),
            "remote",
        ) {
            eprintln!("GitManager Remote Archive for {repository_path}: {error}");
        }
    }
}
