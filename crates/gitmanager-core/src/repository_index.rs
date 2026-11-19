//! Persisted repository index and lightweight UI-facing summaries.
//!
//! The desktop UI can read this module without linking the directory traversal
//! engine, which is enabled only for the standalone gitmanager.service crate.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::git_info::GitRepositoryInfo;
use crate::storage::data_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub repositories: Vec<GitRepositoryInfo>,
    pub truncated: bool,
}

const REPOSITORY_INDEX_VERSION: u32 = 1;
const MAX_REPOSITORY_INDEX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedRepositoryIndex {
    pub schema_version: u32,
    pub indexed_at_unix_ms: u64,
    #[serde(default)]
    pub scan_settings_hash: String,
    pub truncated: bool,
    pub repositories: Vec<GitRepositoryInfo>,
}

pub fn repository_index_path() -> Result<PathBuf, String> {
    Ok(crate::storage::data_dir()?
        .join("repo")
        .join("metadata")
        .join("index.json"))
}

fn repository_index_generation_path() -> Result<PathBuf, String> {
    Ok(crate::storage::data_dir()?
        .join("repo")
        .join("metadata")
        .join("index.generation"))
}

fn read_index_generation_file(path: &Path) -> Result<Option<u64>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not inspect repository index generation: {error}"))?;
    if metadata.len() > 32 {
        return Err("Repository index generation file is invalid.".to_string());
    }
    let content = fs::read_to_string(path)
        .map_err(|error| format!("Could not read repository index generation: {error}"))?;
    let generation = content.trim().parse::<u64>()
        .map_err(|_| "Repository index generation file is invalid.".to_string())?;
    Ok(Some(generation))
}

fn next_index_generation(previous: u64, current_unix_ns: u128) -> u64 {
    previous
        .saturating_add(1)
        .max(current_unix_ns.min(u64::MAX as u128) as u64)
}

fn write_index_generation_file(path: &Path, generation: u64) -> Result<(), String> {
    let parent = path.parent()
        .ok_or_else(|| "Repository index generation path has no parent directory.".to_string())?;
    let temporary = path.with_extension("generation.tmp");
    let backup = path.with_extension("generation.previous");
    {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Could not create repository index generation: {error}"))?;
        use std::io::Write;
        write!(file, "{generation}\n")
            .map_err(|error| format!("Could not write repository index generation: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Could not flush repository index generation: {error}"))?;
    }

    let had_previous = path.is_file();
    if had_previous {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Could not clear previous index generation backup: {error}"))?;
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("Could not stage previous index generation: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_previous {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("Could not publish repository index generation: {error}"));
    }
    if backup.exists() {
        let _ = fs::remove_file(backup);
    }
    // Ensure the directory entry is durable before notifying the UI.
    let _ = OpenOptions::new().read(true).open(parent).and_then(|directory| directory.sync_all());
    Ok(())
}

/// Returns a small monotonic generation without deserializing the repository index.
pub fn repository_index_generation() -> Result<u64, String> {
    let generation_path = repository_index_generation_path()?;
    let sidecar_generation = read_index_generation_file(&generation_path)
        .ok()
        .flatten()
        .unwrap_or(0);

    // The index timestamp remains a cheap recovery signal. Taking the maximum
    // means a missing/stale sidecar cannot hide an index publish, while the
    // sidecar advances generations even on filesystems with coarse timestamps.
    let index_path = repository_index_path()?;
    let file_generation = if index_path.is_file() {
        let modified = fs::metadata(&index_path)
            .map_err(|error| format!("Could not inspect repository index {}: {error}", index_path.display()))?
            .modified()
            .map_err(|error| format!("Could not read repository index timestamp {}: {error}", index_path.display()))?;
        modified.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos().min(u64::MAX as u128) as u64
    } else {
        0
    };
    Ok(sidecar_generation.max(file_generation))
}

/// Compatibility alias retained for callers that used the former timestamp API.
pub fn repository_index_modified_unix_nanos() -> Result<u128, String> {
    Ok(u128::from(repository_index_generation()?))
}

pub fn load_persisted_repository_index() -> Result<Option<PersistedRepositoryIndex>, String> {
    let path = repository_index_path()?;
    // The writer stages the previous valid file as a backup before replacing
    // the primary path. Readers may observe that brief rename window; read the
    // backup in place rather than returning an empty inventory or mutating it.
    let backup = path.with_extension("json.previous");
    let read_path = if path.is_file() {
        &path
    } else if backup.is_file() {
        &backup
    } else {
        return Ok(None);
    };
    let metadata = fs::metadata(read_path)
        .map_err(|error| format!("Could not inspect repository index {}: {error}", read_path.display()))?;
    if metadata.len() > MAX_REPOSITORY_INDEX_BYTES {
        return Err(format!(
            "Repository index {} exceeds the {} MiB safety limit.",
            read_path.display(),
            MAX_REPOSITORY_INDEX_BYTES / (1024 * 1024)
        ));
    }
    let bytes = fs::read(read_path)
        .map_err(|error| format!("Could not read repository index {}: {error}", read_path.display()))?;
    let index: PersistedRepositoryIndex = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Repository index {} is invalid: {error}", read_path.display()))?;
    if index.schema_version != REPOSITORY_INDEX_VERSION {
        return Err(format!(
            "Repository index schema {} is unsupported; expected {}.",
            index.schema_version,
            REPOSITORY_INDEX_VERSION
        ));
    }
    Ok(Some(index))
}

pub fn save_persisted_repository_index(
    report: &ScanReport,
    scan_settings_hash: &str,
) -> Result<PersistedRepositoryIndex, String> {
    let path = repository_index_path()?;
    let parent = path.parent()
        .ok_or_else(|| "Repository index path has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create repository metadata directory: {error}"))?;
    let backup = path.with_extension("json.previous");
    // Recovery belongs to the single-writer service. Readers must never move
    // the backup while a publish is in progress.
    if !path.is_file() && backup.is_file() {
        fs::rename(&backup, &path)
            .map_err(|error| format!("Could not recover the previous repository index: {error}"))?;
    }

    let previous_generation = repository_index_generation().unwrap_or(0);
    let current_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let next_generation = next_index_generation(previous_generation, current_unix.as_nanos());
    let current_time = current_unix.as_millis().min(u64::MAX as u128) as u64;
    // Keep the generation strictly increasing even when two refreshes happen
    // within the same system-clock millisecond.
    // A corrupt prior index must not prevent the service from rebuilding it.
    let previous_time = load_persisted_repository_index()
        .ok()
        .flatten()
        .map(|previous| previous.indexed_at_unix_ms.saturating_add(1))
        .unwrap_or(0);
    let indexed_at_unix_ms = current_time.max(previous_time);
    let index = PersistedRepositoryIndex {
        schema_version: REPOSITORY_INDEX_VERSION,
        indexed_at_unix_ms,
        scan_settings_hash: scan_settings_hash.to_string(),
        truncated: report.truncated,
        repositories: report.repositories.clone(),
    };
    let encoded = serde_json::to_vec(&index)
        .map_err(|error| format!("Could not encode repository index: {error}"))?;
    if encoded.len() as u64 > MAX_REPOSITORY_INDEX_BYTES {
        return Err("Repository index exceeds the 64 MiB safety limit.".to_string());
    }

    let temporary = path.with_extension("json.tmp");
    {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Could not create temporary repository index: {error}"))?;
        use std::io::Write;
        file.write_all(&encoded)
            .map_err(|error| format!("Could not write temporary repository index: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Could not flush temporary repository index: {error}"))?;
    }
    let had_previous = path.is_file();
    if had_previous {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Could not clear previous repository index backup: {error}"))?;
        }
        fs::rename(&path, &backup)
            .map_err(|error| format!("Could not stage the previous repository index: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if had_previous {
            let _ = fs::rename(&backup, &path);
        }
        return Err(format!("Could not publish repository index {}: {error}", path.display()));
    }
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }

    let generation_path = repository_index_generation_path()?;
    write_index_generation_file(&generation_path, next_generation)?;
    Ok(index)
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

const DEPENDENCY_DIRECTORY_NAMES: &[&str] = &[
    "node_modules", "target", "vendor", ".cargo", ".rustup", ".npm", ".pnpm-store",
    ".yarn", "venv", ".venv", "env", "site-packages", "dist-packages", "__pycache__",
    ".gradle", ".nuget", "bower_components", "pods", "dist", "build", "bin", "obj",
    "third_party", "third-party",
];

pub fn is_dependency_repository_path(path: &str) -> bool {
    Path::new(path).components().any(|component| {
        let part = component.as_os_str().to_string_lossy().to_lowercase();
        DEPENDENCY_DIRECTORY_NAMES.contains(&part.as_str())
    })
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

#[cfg(test)]
mod tests {
    use super::{next_index_generation, read_index_generation_file, write_index_generation_file};
    use std::fs;

    #[test]
    fn index_generation_advances_even_when_the_clock_does_not() {
        assert_eq!(next_index_generation(100, 100), 101);
        assert_eq!(next_index_generation(100, 99), 101);
        assert_eq!(next_index_generation(100, 150), 150);
    }

    #[test]
    fn index_generation_sidecar_round_trips_and_replaces_atomically() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("gitmanager-index-generation-{nonce}"));
        fs::create_dir_all(&directory).expect("create generation test directory");
        let path = directory.join("index.generation");

        write_index_generation_file(&path, 100).expect("write initial generation");
        assert_eq!(read_index_generation_file(&path).unwrap(), Some(100));
        write_index_generation_file(&path, 101).expect("replace generation");
        assert_eq!(read_index_generation_file(&path).unwrap(), Some(101));

        fs::remove_dir_all(directory).expect("remove generation test directory");
    }
    use super::{is_dependency_repository_path, PersistedRepositoryIndex};

    #[test]
    fn persisted_repository_index_accepts_a_legacy_record_without_settings_hash() {
        let legacy = serde_json::json!({
            "schema_version": 1,
            "indexed_at_unix_ms": 123,
            "truncated": false,
            "repositories": []
        });
        let index: PersistedRepositoryIndex =
            serde_json::from_value(legacy).expect("legacy index remains readable");
        assert_eq!(index.scan_settings_hash, "");
        assert_eq!(index.indexed_at_unix_ms, 123);
    }

    #[test]
    fn dependency_repository_paths_use_exact_path_components() {
        assert!(is_dependency_repository_path("projects/app/node_modules/pkg"));
        assert!(!is_dependency_repository_path("projects/targeting-tool/src"));
    }
}
