use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use fs2::FileExt;

use crate::git_info::{extract_git_repository_summary, GitRepositoryInfo};

const SCAN_WORKERS: usize = 2;
const MAX_SCAN_DIRECTORIES: usize = 40_000;
const MAX_SCAN_ENTRIES: usize = 250_000;
const MAX_PENDING_DIRECTORIES: usize = 8_192;
const MAX_REPOSITORIES: usize = 1_000;
const MAX_DEPENDENCY_DESCENT: usize = 2;

// Prevent UI, archive watcher, and update service scans from multiplying I/O.
static ACTIVE_SCAN: Mutex<()> = Mutex::new(());

const IGNORED_DIRECTORY_NAMES: &[&str] = &[
    "appdata", "library", "system volume information", "$recycle.bin", "windows",
    "program files", "program files (x86)", "programdata", "recovery", "proc",
    "sys", "dev", "volumes",
];
const DEPENDENCY_DIRECTORY_NAMES: &[&str] = &[
    "node_modules", "target", "vendor", ".cargo", ".rustup", ".npm", ".pnpm-store",
    ".yarn", "venv", ".venv", "env", "site-packages", "dist-packages", "__pycache__",
    ".gradle", ".nuget", "bower_components", "pods", "dist", "build", "bin", "obj",
    "third_party", "third-party",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub root_paths: Vec<String>,
    pub max_depth: Option<usize>,
    pub include_hidden_dirs: Option<bool>,
    pub index_dependency_repositories: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ScanReport {
    pub repositories: Vec<GitRepositoryInfo>,
    pub truncated: bool,
}

#[derive(Clone)]
struct ScanTask { root: PathBuf, path: PathBuf, depth: usize }

struct QueueState { pending: VecDeque<ScanTask>, active: usize }

#[derive(Clone)]
struct ScanConfig { max_depth: usize, include_hidden: bool, index_dependencies: bool }

#[derive(Clone)]
struct ScanShared {
    queue: Arc<(Mutex<QueueState>, Condvar)>,
    config: Arc<ScanConfig>,
    dependency_fallback: bool,
    directories_seen: Arc<AtomicUsize>,
    entries_seen: Arc<AtomicUsize>,
    seen_repositories: Arc<Mutex<HashSet<String>>>,
    repositories: Arc<Mutex<Vec<GitRepositoryInfo>>>,
    truncated: Arc<AtomicBool>,
}

pub fn default_scan_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        let candidates = [
            "Projects", "Repos", "Repositories", "Development", "dev", "workspace",
            "src", "code", "GitHub", "GitLab", "source/repos", "Documents/GitHub",
            "Documents/Projects", "Desktop",
        ];
        for relative in candidates {
            let path = home.join(relative);
            if path.exists() && path.is_dir() { roots.push(path); }
        }
        if roots.is_empty() { roots.push(home); }
    }
    roots
}

/// Compatibility wrapper for archive and background-update callers.
pub fn scan_repositories(options: ScanOptions) -> Result<Vec<GitRepositoryInfo>, String> {
    scan_repositories_with_report(options).map(|report| report.repositories)
}

/// Discover ordinary projects first. Dependency folders are inspected only as
/// a fallback, and only when the primary scan finds no regular repositories.
pub fn scan_repositories_with_report(options: ScanOptions) -> Result<ScanReport, String> {
    let _scan_guard = ACTIVE_SCAN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // A second process (the standalone background service) shares this lease.
    // It keeps only one two-thread crawl active across the whole user session.
    let _process_scan_lease = acquire_process_scan_lease()?;
    let config = Arc::new(ScanConfig {
        max_depth: options.max_depth.unwrap_or(8).clamp(2, 16),
        include_hidden: options.include_hidden_dirs.unwrap_or(false),
        index_dependencies: options.index_dependency_repositories.unwrap_or(true),
    });
    let raw_roots = if options.root_paths.is_empty() {
        default_scan_roots()
    } else {
        options.root_paths.into_iter().map(PathBuf::from).collect()
    };

    let mut root_candidates = Vec::new();
    for raw in raw_roots {
        if !raw.is_dir() { continue; }
        let root = fs::canonicalize(&raw).unwrap_or(raw);
        root_candidates.push(root);
    }
    // A configured parent root already covers its descendants. Removing nested
    // roots avoids repeating the same directory walk for overlapping settings.
    let roots = normalize_scan_roots(root_candidates);

    let primary = run_scan_pass(&roots, Arc::clone(&config), false);
    if !primary.repositories.is_empty() || !config.index_dependencies || primary.truncated {
        return Ok(primary);
    }
    Ok(run_scan_pass(&roots, config, true))
}

fn run_scan_pass(roots: &[PathBuf], config: Arc<ScanConfig>, dependency_fallback: bool) -> ScanReport {
    let mut pending = VecDeque::new();
    for root in roots {
        let root_is_dependency = path_has_dependency_component(root);
        if root_is_dependency && !dependency_fallback { continue; }
        pending.push_back(ScanTask { root: root.clone(), path: root.clone(), depth: 0 });
    }

    let shared = ScanShared {
        queue: Arc::new((Mutex::new(QueueState { pending, active: 0 }), Condvar::new())),
        config,
        dependency_fallback,
        directories_seen: Arc::new(AtomicUsize::new(0)),
        entries_seen: Arc::new(AtomicUsize::new(0)),
        seen_repositories: Arc::new(Mutex::new(HashSet::new())),
        repositories: Arc::new(Mutex::new(Vec::new())),
        truncated: Arc::new(AtomicBool::new(false)),
    };

    // A selected scan root may itself be the repository, not merely its parent.
    // Register it without descending into its .git database.
    for root in roots {
        if root.join(".git").exists() && path_has_dependency_component(root) == dependency_fallback {
            register_repository(&root.join(".git"), &shared);
        }
    }

    thread::scope(|scope| {
        for _ in 0..scan_worker_count() {
            let worker = shared.clone();
            scope.spawn(move || scan_worker(worker));
        }
    });

    let mut repositories = shared.repositories.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    repositories.sort_by(|left, right| {
        left.parent_folder.to_lowercase().cmp(&right.parent_folder.to_lowercase())
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    ScanReport { repositories, truncated: shared.truncated.load(Ordering::Relaxed) }
}

fn acquire_process_scan_lease() -> Result<File, String> {
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the current user's local data directory for scan ownership.".to_string())?;
    let data_dir = local_data
        .join("kastrick's_software")
        .join("data")
        .join("GitManager");
    fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Could not create repository scan state directory: {error}"))?;

    let lock_path = data_dir.join("repository-scan.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| format!("Could not open repository scan lease {}: {error}", lock_path.display()))?;
    FileExt::lock_exclusive(&file)
        .map_err(|error| format!("Could not acquire repository scan lease {}: {error}", lock_path.display()))?;
    Ok(file)
}

fn scan_worker_count_for(available: usize) -> usize {
    if available >= 2 { SCAN_WORKERS } else { 1 }
}

fn scan_worker_count() -> usize {
    // Use two discovery workers on a multi-processor machine, but stay
    // single-threaded on a single-core environment.
    std::thread::available_parallelism()
        .map(|count| scan_worker_count_for(count.get()))
        .unwrap_or(1)
}

fn scan_worker(shared: ScanShared) {
    loop {
        let task = {
            let (lock, ready) = &*shared.queue;
            let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            loop {
                if let Some(task) = state.pending.pop_front() {
                    state.active += 1;
                    break Some(task);
                }
                if state.active == 0 { break None; }
                state = ready.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let Some(task) = task else { break };
        let children = scan_one_directory(&task, &shared);
        let (lock, ready) = &*shared.queue;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.active = state.active.saturating_sub(1);
        for child in children {
            if state.pending.len() >= MAX_PENDING_DIRECTORIES {
                shared.truncated.store(true, Ordering::Relaxed);
                state.pending.clear();
                break;
            }
            state.pending.push_back(child);
        }
        ready.notify_all();
    }
}

fn scan_one_directory(task: &ScanTask, shared: &ScanShared) -> Vec<ScanTask> {
    if shared.truncated.load(Ordering::Relaxed) || task.depth >= shared.config.max_depth {
        return Vec::new();
    }
    if shared.directories_seen.fetch_add(1, Ordering::Relaxed) + 1 > MAX_SCAN_DIRECTORIES {
        truncate_scan(shared);
        return Vec::new();
    }
    let entries = match fs::read_dir(&task.path) { Ok(entries) => entries, Err(_) => return Vec::new() };
    let mut children = Vec::new();

    for entry_result in entries {
        if shared.truncated.load(Ordering::Relaxed) { break; }
        if shared.entries_seen.fetch_add(1, Ordering::Relaxed) + 1 > MAX_SCAN_ENTRIES {
            truncate_scan(shared);
            break;
        }
        // Yield occasionally so large directory enumerations share CPU time
        // with the foreground UI and the operating system.
        if shared.entries_seen.load(Ordering::Relaxed) % 128 == 0 {
            thread::yield_now();
        }
        let Ok(entry) = entry_result else { continue };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let depth = task.depth + 1;

        // .git can be a directory or a worktree pointer file. Never descend into it.
        if name == ".git" {
            let relative = path.strip_prefix(&task.root).unwrap_or(path.as_path());
            let inside_dependency = path_has_dependency_component(relative)
                || path_has_dependency_component(&task.root);
            if inside_dependency == shared.dependency_fallback {
                register_repository(&path, shared);
            }
            continue;
        }

        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() || kind.is_symlink() || depth >= shared.config.max_depth { continue; }
        if !should_visit_directory(
            &path, &task.root, depth, shared.config.include_hidden, shared.dependency_fallback,
        ) { continue; }

        if children.len() >= MAX_PENDING_DIRECTORIES {
            shared.truncated.store(true, Ordering::Relaxed);
            break;
        }
        children.push(ScanTask { root: task.root.clone(), path, depth });
    }
    children
}

fn register_repository(git_marker: &Path, shared: &ScanShared) {
    let Some(workdir) = git_marker.parent() else { return };
    let canonical = fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let key = path_key(&canonical);
    let should_read = {
        let mut seen = shared.seen_repositories.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if seen.contains(&key) { false }
        else if seen.len() >= MAX_REPOSITORIES {
            shared.truncated.store(true, Ordering::Relaxed);
            false
        } else { seen.insert(key) }
    };
    if !should_read { return; }

    // A card needs only remotes and HEAD. History, tags, branches, and worktree
    // status are deferred until the selected repository is opened.
    if let Ok(repository) = extract_git_repository_summary(&canonical, git_marker) {
        shared.repositories.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()).push(repository);
    }
}

fn truncate_scan(shared: &ScanShared) {
    shared.truncated.store(true, Ordering::Relaxed);
    let (lock, ready) = &*shared.queue;
    let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.pending.clear();
    ready.notify_all();
}

fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().to_string();
    #[cfg(windows)] { value.to_lowercase() }
    #[cfg(not(windows))] { value }
}

fn normalize_scan_roots(mut candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    candidates.sort_by(|left, right| {
        left.components().count().cmp(&right.components().count())
            .then_with(|| path_key(left).cmp(&path_key(right)))
    });

    let mut roots = Vec::new();
    for candidate in candidates {
        let candidate_key = path_key(&candidate);
        let already_covered = roots.iter().any(|parent: &PathBuf| {
            let parent_key = path_key(parent);
            if candidate_key == parent_key { return true; }
            let prefix = format!(
                "{}{}",
                parent_key.trim_end_matches(std::path::MAIN_SEPARATOR),
                std::path::MAIN_SEPARATOR
            );
            candidate_key.starts_with(&prefix)
        });
        if !already_covered { roots.push(candidate); }
    }
    roots
}


fn path_has_dependency_component(path: &Path) -> bool {
    path.components().any(|component| {
        let part = component.as_os_str().to_string_lossy().to_lowercase();
        DEPENDENCY_DIRECTORY_NAMES.contains(&part.as_str())
    })
}

fn should_visit_directory(
    path: &Path, root: &Path, depth: usize, include_hidden: bool, dependency_fallback: bool,
) -> bool {
    let Ok(relative) = path.strip_prefix(root) else { return false };
    let components: Vec<String> = relative.components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase()).collect();
    let Some(name) = components.last() else { return true };
    if name == ".git" || IGNORED_DIRECTORY_NAMES.contains(&name.as_str()) { return false; }

    let dependency_index = components.iter()
        .position(|part| DEPENDENCY_DIRECTORY_NAMES.contains(&part.as_str()));
    let root_is_dependency = path_has_dependency_component(root);
    if !dependency_fallback && (dependency_index.is_some() || root_is_dependency) { return false; }

    if dependency_fallback {
        let depth_inside_dependency = dependency_index
            .map(|index| components.len().saturating_sub(index + 1))
            .unwrap_or_else(|| if root_is_dependency { depth } else { 0 });
        if (dependency_index.is_some() || root_is_dependency)
            && depth_inside_dependency > MAX_DEPENDENCY_DESCENT { return false; }
    }

    let inside_dependency = dependency_index.is_some() || root_is_dependency;
    if !include_hidden && depth > 0 && name.starts_with('.')
        && !inside_dependency && !DEPENDENCY_DIRECTORY_NAMES.contains(&name.as_str()) {
        return false;
    }
    true
}

fn is_dependency_repository_path(path: &str) -> bool {
    path_has_dependency_component(Path::new(path))
}

#[cfg(test)]
mod tests {
    use super::{
        is_dependency_repository_path, normalize_scan_roots, scan_worker_count_for,
        should_visit_directory,
    };
    use std::path::{Path, PathBuf, MAIN_SEPARATOR};

    #[test]
    fn classifies_dependency_directory_paths() {
        let path = format!("projects{0}app{0}node_modules{0}some-package", MAIN_SEPARATOR);
        assert!(is_dependency_repository_path(&path));
    }
    #[test]
    fn does_not_classify_similarly_named_directories() {
        let path = format!("projects{0}targeting-tool{0}src", MAIN_SEPARATOR);
        assert!(!is_dependency_repository_path(&path));
    }
    #[test]
    fn primary_pass_skips_dependency_trees() {
        let root = Path::new("/workspace");
        assert!(!should_visit_directory(&root.join("node_modules"), root, 1, false, false));
        assert!(!should_visit_directory(&root.join("vendor"), root, 1, false, false));
        assert!(should_visit_directory(&root.join("src"), root, 1, false, false));
    }
    #[test]
    fn fallback_traversal_limits_depth_inside_dependencies() {
        let root = Path::new("/workspace");
        assert!(should_visit_directory(&root.join("packages"), root, 1, false, true));
        assert!(should_visit_directory(&root.join("packages/node_modules"), root, 2, false, true));
        assert!(should_visit_directory(&root.join("packages/node_modules/pkg"), root, 3, false, true));
        assert!(should_visit_directory(&root.join("packages/node_modules/pkg/src"), root, 4, false, true));
        assert!(!should_visit_directory(&root.join("packages/node_modules/pkg/src/deep"), root, 5, false, true));
    }
    #[test]
    fn hidden_directories_follow_user_preference() {
        let root = Path::new("/workspace");
        assert!(!should_visit_directory(&root.join(".cache"), root, 1, false, false));
        assert!(should_visit_directory(&root.join(".cache"), root, 1, true, false));
    }

    #[test]
    fn scan_roots_already_inside_dependency_trees_use_fallback_mode() {
        let root = Path::new("/workspace/node_modules/example-package");
        assert!(!should_visit_directory(&root.join("src"), root, 1, false, false));
        assert!(should_visit_directory(&root.join("src"), root, 1, false, true));
    }

    #[test]
    fn dependency_paths_are_detected_above_the_selected_root() {
        let root = Path::new("/workspace/node_modules/example-package");
        assert!(super::path_has_dependency_component(root));
    }

    #[test]
    fn drops_duplicate_and_nested_scan_roots() {
        let roots = normalize_scan_roots(vec![
            PathBuf::from("/workspace/projects/app"),
            PathBuf::from("/workspace"),
            PathBuf::from("/workspace/projects"),
            PathBuf::from("/workspace"),
        ]);
        assert_eq!(roots, vec![PathBuf::from("/workspace")]);
    }

    #[test]
    fn uses_two_workers_when_at_least_two_processors_exist() {
        assert_eq!(scan_worker_count_for(1), 1);
        assert_eq!(scan_worker_count_for(2), 2);
        assert_eq!(scan_worker_count_for(16), 2);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedRepoSummary {
    pub repositories: Vec<GitRepositoryInfo>,
    pub by_folder: HashMap<String, Vec<String>>,
    pub remote_count: usize,
    pub local_only_count: usize,
    pub dependency_count: usize,
    pub dependency_repo_ids: Vec<String>,
    pub effective_filter_mode: String,
    pub scan_truncated: bool,
}

pub fn build_indexed_summary(repos: Vec<GitRepositoryInfo>, requested_filter: &str) -> IndexedRepoSummary {
    build_indexed_summary_with_scan_status(repos, requested_filter, false)
}

pub fn build_indexed_summary_with_scan_status(
    repos: Vec<GitRepositoryInfo>, requested_filter: &str, scan_truncated: bool,
) -> IndexedRepoSummary {
    let remote_count = repos.iter().filter(|repository| repository.has_remote).count();
    let local_only_count = repos.len().saturating_sub(remote_count);
    let dependency_repo_ids = repos.iter()
        .filter(|repository| is_dependency_repository_path(&repository.path))
        .map(|repository| repository.id.clone()).collect::<Vec<_>>();
    let dependency_count = dependency_repo_ids.len();

    let mut by_folder: HashMap<String, Vec<String>> = HashMap::new();
    for repository in &repos {
        by_folder.entry(repository.parent_folder.clone()).or_default().push(repository.id.clone());
    }
    let effective_filter_mode = match requested_filter {
        "remote" if remote_count > 0 => "remote".to_string(),
        "local" if local_only_count > 0 => "local".to_string(),
        "remote" | "local" | "grouped" | _ => {
            if remote_count == 0 || local_only_count == 0 { "grouped_fallback".to_string() }
            else if requested_filter == "remote" || requested_filter == "local" { requested_filter.to_string() }
            else { "grouped".to_string() }
        }
    };
    IndexedRepoSummary {
        repositories: repos, by_folder, remote_count, local_only_count,
        dependency_count, dependency_repo_ids, effective_filter_mode, scan_truncated,
    }
}
