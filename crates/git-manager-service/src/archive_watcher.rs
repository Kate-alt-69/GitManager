//! Bounded archive capture watcher.
//!
//! A single OS filesystem watcher observes discovered repositories. At most two
//! archive captures execute concurrently; this keeps idle and burst-time thread
//! use predictable on lower-end machines.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::archive;
use gitmanager_core::models::GitRepositoryInfo;
use crate::scanner::{scan_repositories, ScanOptions};
use crate::AppPreferences;

static WATCH_GENERATION: AtomicU64 = AtomicU64::new(0);
static WATCHER_RUNNING: AtomicBool = AtomicBool::new(false);
static WATCHER_LEASE: OnceLock<Mutex<Option<Arc<File>>>> = OnceLock::new();
static WATCHER_CONFIG: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// Returns whether a non-blocking fs2 lock failed because another owner holds it.
///
/// fs2 exposes the platform-specific contention error kind; on Windows this
/// is not necessarily std::io::ErrorKind::WouldBlock.
fn is_watcher_lease_contended(error: &std::io::Error) -> bool {
    error.kind() == fs2::lock_contended_error().kind()
}
const MAX_WATCHED_REPOSITORIES: usize = 256;
const MAX_CONCURRENT_ARCHIVES: usize = 2;
const COMMIT_SETTLE: Duration = Duration::from_millis(180);
const EVENT_POLL: Duration = Duration::from_millis(200);

struct WatchedRepository {
    path: String,
    root: PathBuf,
    git_dir: PathBuf,
    archive_root: String,
    local_enabled: bool,
    remote_enabled: bool,
    last_change: Option<Instant>,
    pending_commit: Option<Instant>,
    last_head: Option<String>,
    capture_in_progress: bool,
}

struct WatcherRunGuard {
    generation: u64,
}

impl Drop for WatcherRunGuard {
    fn drop(&mut self) {
        if WATCH_GENERATION.load(Ordering::SeqCst) == self.generation {
            WATCHER_RUNNING.store(false, Ordering::SeqCst);
            if let Some(slot) = WATCHER_CONFIG.get() {
                *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
            }
            if let Some(slot) = WATCHER_LEASE.get() {
                *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
            }
        }
    }
}

fn watcher_lease_path() -> Result<PathBuf, String> {
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the user's local data directory for archive watcher ownership.".to_string())?;
    let data_dir = local_data
        .join("kastrick's_software")
        .join("data")
        .join("GitManager");
    std::fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Could not create archive watcher state directory: {error}"))?;
    Ok(data_dir.join("archive-watcher.lock"))
}

fn watcher_config_signature(
    preferences: &AppPreferences,
    repositories: Option<&[GitRepositoryInfo]>,
) -> String {
    let repository_paths = repositories.map(|items: &[GitRepositoryInfo]| {
        items.iter().map(|repository| repository.path.as_str()).collect::<Vec<_>>()
    });
    serde_json::to_string(&(
        &preferences.scan_roots,
        preferences.max_scan_depth,
        preferences.include_hidden_dirs,
        preferences.index_dependency_repositories,
        preferences.first_run_complete,
        &preferences.archive_root,
        preferences.archive_local_enabled,
        preferences.archive_remote_enabled,
        &preferences.archive_trigger,
        preferences.archive_quiet_ms,
        repository_paths,
    ))
    .unwrap_or_default()
}

pub fn archive_watchers_running() -> bool {
    WATCHER_RUNNING.load(Ordering::SeqCst)
}

/// Stop accepting new archive events/captures while retaining archive data.
/// Already-running transactional capture workers are allowed to finish safely.
pub fn stop_archive_watchers() {
    WATCH_GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(slot) = WATCHER_CONFIG.get() {
        *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
    if let Some(slot) = WATCHER_LEASE.get() {
        *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
    WATCHER_RUNNING.store(false, Ordering::SeqCst);
}

pub fn restart_archive_watchers(preferences: &AppPreferences) -> bool {
    restart_archive_watchers_with_scan(preferences, None)
}

/// Starts archive watchers from the repository list published by gitmanager.service.
/// This prevents a second full directory scan over the configured roots.
pub fn restart_archive_watchers_with_repositories(
    preferences: &AppPreferences,
    repositories: Vec<GitRepositoryInfo>,
) -> bool {
    restart_archive_watchers_with_scan(preferences, Some(repositories))
}

fn restart_archive_watchers_with_scan(
    preferences: &AppPreferences,
    discovered_repositories: Option<Vec<GitRepositoryInfo>>,
) -> bool {
    let enabled = preferences.first_run_complete
        && preferences.archive_trigger != "manual"
        && (preferences.archive_local_enabled || preferences.archive_remote_enabled);
    let lease_slot = WATCHER_LEASE.get_or_init(|| Mutex::new(None));
    let config_slot = WATCHER_CONFIG.get_or_init(|| Mutex::new(None));
    let mut lease_guard = lease_slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut config_guard = config_slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    if !enabled {
        WATCH_GENERATION.fetch_add(1, Ordering::SeqCst);
        *config_guard = None;
        *lease_guard = None;
        WATCHER_RUNNING.store(false, Ordering::SeqCst);
        return false;
    }

    let signature = watcher_config_signature(preferences, discovered_repositories.as_deref());
    if config_guard.as_deref() == Some(signature.as_str())
        && lease_guard.is_some()
        && WATCHER_RUNNING.load(Ordering::SeqCst)
    {
        return true;
    }

    if lease_guard.is_none() {
        let path = match watcher_lease_path() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("GitManager archive watcher: {error}");
                return false;
            }
        };
        let file = match OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) => {
                eprintln!("GitManager archive watcher: could not open {}: {error}", path.display());
                return false;
            }
        };
        if let Err(error) = file.try_lock_exclusive() {
            if !is_watcher_lease_contended(&error) {
                eprintln!("GitManager archive watcher: could not acquire {}: {error}", path.display());
            }
            return false;
        }
        *lease_guard = Some(Arc::new(file));
    }

    let generation = WATCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    *config_guard = Some(signature);
    let lease = match lease_guard.as_ref() {
        Some(file) => Arc::clone(file),
        None => return false,
    };
    drop(config_guard);
    drop(lease_guard);

    let roots = preferences.scan_roots.clone();
    let max_depth = preferences.max_scan_depth;
    let include_hidden = preferences.include_hidden_dirs;
    let trigger = preferences.archive_trigger.clone();
    let quiet_ms = preferences.archive_quiet_ms.clamp(250, 60_000);
    let archive_root = preferences.archive_root.clone();
    let local_enabled = preferences.archive_local_enabled;
    let remote_enabled = preferences.archive_remote_enabled;
    let index_dependencies = preferences.index_dependency_repositories;

    WATCHER_RUNNING.store(true, Ordering::SeqCst);
    let spawned = thread::Builder::new()
        .name("gitmanager-archive-watcher".to_string())
        .spawn(move || {
            let _lease = lease;
            let _run_guard = WatcherRunGuard { generation };
            let repositories = match discovered_repositories {
                Some(repositories) => repositories,
                None => match scan_repositories(ScanOptions {
                    root_paths: roots,
                    max_depth: Some(max_depth.clamp(2, 16)),
                    include_hidden_dirs: Some(include_hidden),
                    index_dependency_repositories: Some(index_dependencies),
                }) {
                    Ok(repositories) => repositories,
                    Err(error) => {
                        eprintln!("GitManager archive watcher: repository discovery failed: {error}");
                        return;
                    }
                },
            };
            if generation != WATCH_GENERATION.load(Ordering::SeqCst) {
                return;
            }

            let archive_root_path = canonicalize_or_original(Path::new(&archive_root));
            let mut watched = Vec::<WatchedRepository>::new();
            let mut watch_targets = Vec::<(PathBuf, RecursiveMode)>::new();
            let mut registered = HashSet::<PathBuf>::new();

            for repository in repositories.into_iter().take(MAX_WATCHED_REPOSITORIES) {
                let root = match std::fs::canonicalize(&repository.path) {
                    Ok(path) => path,
                    Err(error) => {
                        eprintln!(
                            "GitManager archive watcher: could not resolve repository {}: {error}",
                            repository.path
                        );
                        continue;
                    }
                };
                if archive_root_path == root || archive_root_path.starts_with(&root) {
                    eprintln!(
                        "GitManager archive watcher: refusing to watch {} because archive storage {} is inside it",
                        root.display(),
                        archive_root_path.display()
                    );
                    continue;
                }

                let git_dir = canonicalize_or_original(Path::new(&repository.git_dir));
                if trigger == "commit" {
                    if !git_dir.is_dir() {
                        continue;
                    }
                    // HEAD and packed-refs are direct entries. Refs and reflog refs
                    // are recursive. Never watch the potentially huge object DB.
                    add_watch_target(
                        &mut watch_targets,
                        &mut registered,
                        git_dir.clone(),
                        RecursiveMode::NonRecursive,
                    );
                    let refs = git_dir.join("refs");
                    if refs.is_dir() {
                        add_watch_target(
                            &mut watch_targets,
                            &mut registered,
                            refs,
                            RecursiveMode::Recursive,
                        );
                    }
                    let ref_logs = git_dir.join("logs").join("refs");
                    if ref_logs.is_dir() {
                        add_watch_target(
                            &mut watch_targets,
                            &mut registered,
                            ref_logs,
                            RecursiveMode::Recursive,
                        );
                    }
                } else {
                    add_watch_target(
                        &mut watch_targets,
                        &mut registered,
                        root.clone(),
                        RecursiveMode::Recursive,
                    );
                }

                watched.push(WatchedRepository {
                    path: repository.path,
                    root,
                    git_dir,
                    archive_root: archive_root.clone(),
                    local_enabled,
                    remote_enabled,
                    last_change: None,
                    pending_commit: None,
                    last_head: None,
                    capture_in_progress: false,
                });
                let index = watched.len() - 1;
                watched[index].last_head = current_head(&watched[index].path);
            }

            if watched.is_empty() || watch_targets.is_empty() {
                return;
            }

            let (event_sender, event_receiver) = mpsc::channel::<notify::Result<Event>>();
            let mut watcher = match notify::recommended_watcher(
                move |event: notify::Result<Event>| {
                    let _ = event_sender.send(event);
                },
            ) {
                Ok(watcher) => watcher,
                Err(error) => {
                    eprintln!("GitManager archive watcher: could not initialize filesystem watcher: {error}");
                    return;
                }
            };

            let mut registered_targets = 0usize;
            for (path, mode) in watch_targets {
                match watcher.watch(&path, mode) {
                    Ok(()) => registered_targets += 1,
                    Err(error) => {
                        eprintln!(
                            "GitManager archive watcher: could not register {}: {error}",
                            path.display()
                        );
                    }
                }
            }
            if registered_targets == 0 {
                eprintln!("GitManager archive watcher: no filesystem watch targets could be registered.");
                return;
            }

            let (capture_done_sender, capture_done_receiver) = mpsc::channel::<usize>();
            let mut active_captures = 0usize;

            while generation == WATCH_GENERATION.load(Ordering::SeqCst) {
                match event_receiver.recv_timeout(EVENT_POLL) {
                    Ok(Ok(event)) => {
                        if !matches!(event.kind, EventKind::Access(_)) {
                            apply_event(&mut watched, &event, &trigger);
                        }
                    }
                    Ok(Err(error)) => {
                        eprintln!("GitManager archive watcher: filesystem event error: {error}");
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }

                while let Ok(index) = capture_done_receiver.try_recv() {
                    if let Some(repository) = watched.get_mut(index) {
                        if repository.capture_in_progress {
                            repository.capture_in_progress = false;
                            active_captures = active_captures.saturating_sub(1);
                        }
                    }
                }

                if active_captures >= MAX_CONCURRENT_ARCHIVES {
                    continue;
                }

                for index in 0..watched.len() {
                    if active_captures >= MAX_CONCURRENT_ARCHIVES {
                        break;
                    }
                    if watched[index].capture_in_progress {
                        continue;
                    }

                    let should_capture = if trigger == "change" {
                        watched[index].last_change.is_some_and(|last| {
                            last.elapsed() >= Duration::from_millis(quiet_ms)
                        })
                    } else {
                        watched[index].pending_commit.is_some_and(|deadline| Instant::now() >= deadline)
                    };
                    if !should_capture {
                        continue;
                    }

                    if trigger == "change" {
                        watched[index].last_change = None;
                    } else {
                        watched[index].pending_commit = None;
                        let head = current_head(&watched[index].path);
                        if head.is_none() || head == watched[index].last_head {
                            continue;
                        }
                        watched[index].last_head = head;
                    }

                    watched[index].capture_in_progress = true;
                    active_captures += 1;
                    let path = watched[index].path.clone();
                    let root = watched[index].archive_root.clone();
                    let local = watched[index].local_enabled;
                    let remote = watched[index].remote_enabled;
                    let done = capture_done_sender.clone();
                    if let Err(error) = thread::Builder::new()
                        .name("gitmanager-archive-capture".to_string())
                        .spawn(move || {
                            capture_enabled_archives(&path, &root, local, remote);
                            let _ = done.send(index);
                        })
                    {
                        watched[index].capture_in_progress = false;
                        active_captures = active_captures.saturating_sub(1);
                        eprintln!(
                            "GitManager archive watcher: could not start capture worker for {}: {error}",
                            watched[index].path
                        );
                    }
                }
            }
        });
    if let Err(error) = spawned {
        WATCHER_RUNNING.store(false, Ordering::SeqCst);
        if let Some(slot) = WATCHER_CONFIG.get() {
            *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        }
        if let Some(slot) = WATCHER_LEASE.get() {
            *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        }
        eprintln!("GitManager archive watcher: could not start watcher thread: {error}");
        return false;
    }
    true
}

fn canonicalize_or_original(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn add_watch_target(
    targets: &mut Vec<(PathBuf, RecursiveMode)>,
    registered: &mut HashSet<PathBuf>,
    path: PathBuf,
    mode: RecursiveMode,
) {
    if registered.insert(path.clone()) {
        targets.push((path, mode));
    }
}

fn apply_event(repositories: &mut [WatchedRepository], event: &Event, trigger: &str) {
    // An event without paths cannot be attributed safely. Ignoring it is
    // preferable to scheduling every repository and causing a snapshot storm.
    if event.paths.is_empty() {
        return;
    }

    for path in &event.paths {
        if trigger == "change" {
            // Attribute a nested-repository event to the deepest discovered repo
            // only, not to every parent repository whose root contains the path.
            let selected = repositories
                .iter()
                .enumerate()
                .filter(|(_, repository)| path.starts_with(&repository.root))
                .max_by_key(|(_, repository)| repository.root.components().count())
                .map(|(index, _)| index);
            if let Some(index) = selected {
                if is_project_file_event(&repositories[index].root, path) {
                    repositories[index].last_change = Some(Instant::now());
                }
            }
        } else {
            let selected = repositories
                .iter()
                .enumerate()
                .filter(|(_, repository)| path.starts_with(&repository.git_dir))
                .max_by_key(|(_, repository)| repository.git_dir.components().count())
                .map(|(index, _)| index);
            if let Some(index) = selected {
                repositories[index].pending_commit = Some(Instant::now() + COMMIT_SETTLE);
            }
        }
    }
}

fn is_project_file_event(repository_root: &Path, event_path: &Path) -> bool {
    let Ok(relative) = event_path.strip_prefix(repository_root) else {
        return false;
    };
    !relative.components().any(|component| {
        component.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")
    })
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
    let repository_path_owned = repository_path.to_string();
    crash_helper::with_repository_archive_lock(&repository_path_owned, || {
        if local_enabled {
            if let Err(error) = archive::capture_repository(
                &repository_path_owned,
                Some(archive_root),
                "local",
            ) {
                eprintln!("GitManager Local Archive for {repository_path_owned}: {error}");
            }
        }
        if remote_enabled {
            if let Err(error) = archive::capture_repository(
                &repository_path_owned,
                Some(archive_root),
                "remote",
            ) {
                eprintln!("GitManager Remote Archive for {repository_path_owned}: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{apply_event, is_project_file_event, is_watcher_lease_contended, WatchedRepository};
    use fs2::FileExt;
    use notify::{Event, EventKind};
    use std::path::{Path, PathBuf};

    #[test]
    fn pathless_filesystem_events_do_not_schedule_every_repository() {
        let repository = WatchedRepository {
            path: "/projects/sample".to_string(),
            root: PathBuf::from("/projects/sample"),
            git_dir: PathBuf::from("/projects/sample/.git"),
            archive_root: "/backups".to_string(),
            local_enabled: true,
            remote_enabled: true,
            last_change: None,
            pending_commit: None,
            last_head: None,
            capture_in_progress: false,
        };
        let mut repositories = vec![repository];
        let event = Event::new(EventKind::Any);
        apply_event(&mut repositories, &event, "change");
        apply_event(&mut repositories, &event, "commit");
        assert!(repositories[0].last_change.is_none());
        assert!(repositories[0].pending_commit.is_none());
    }

    #[test]
    fn file_changes_in_git_metadata_do_not_trigger_change_archives() {
        assert!(!is_project_file_event(
            Path::new("/projects/app"),
            Path::new("/projects/app/.git/index")
        ));
        assert!(!is_project_file_event(
            Path::new("/projects/app"),
            Path::new("/projects/app/.git/objects/pack/pack-abc.pack")
        ));
        assert!(is_project_file_event(
            Path::new("/projects/app"),
            Path::new("/projects/app/src/main.rs")
        ));
    }

    #[test]
    fn watcher_lease_rejects_a_second_process_owner() {
        use std::fs::{self, OpenOptions};
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "gitmanager-watcher-lease-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create watcher lease test directory");
        let path = root.join("archive-watcher.lock");

        let first = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .expect("open first watcher lease");
        first.try_lock_exclusive().expect("first watcher should acquire lease");

        let second = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .expect("open second watcher lease");
        let error = second
            .try_lock_exclusive()
            .expect_err("second watcher must not acquire the same lease");
        assert!(
            is_watcher_lease_contended(&error),
            "a held watcher lease should use fs2's platform-specific contention error kind; got {error:?}, expected {:?}",
            fs2::lock_contended_error().kind()
        );

        FileExt::unlock(&first).expect("release first watcher lease");
        second.try_lock_exclusive().expect("second watcher can acquire released lease");
        FileExt::unlock(&second).expect("release second watcher lease");
        drop(first);
        drop(second);
        let _ = fs::remove_dir_all(root);
    }
}
