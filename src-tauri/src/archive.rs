use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cloud_node::{CloudNodeSettings, CloudNodeStore, is_default_secret_path};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use git2::Repository;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::{Archive as TarArchive, Builder as TarBuilder, Header};
use walkdir::WalkDir;

const ARCHIVE_FORMAT_VERSION: u16 = 1;
const MAX_RESTORE_POINTS: usize = 5;
const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EXCLUDED_PATHS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveKind {
    Local,
    Remote,
}

impl ArchiveKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "remote" => Ok(Self::Remote),
            _ => Err("Archive kind must be 'local' or 'remote'.".to_string()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSnapshot {
    pub id: String,
    pub repository_id: String,
    pub repository_name: String,
    pub repository_path: String,
    pub kind: ArchiveKind,
    pub captured_at: String,
    pub captured_at_unix_ms: u64,
    pub branch: String,
    pub head: Option<String>,
    pub file_count: u64,
    pub source_bytes: u64,
    pub archive_bytes: u64,
    pub sha256: String,
    pub archive_path: String,
    pub excluded_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRestoreResult {
    pub snapshot: ArchiveSnapshot,
    pub destination: String,
    pub preserved_previous_path: Option<String>,
    pub initialized_git_repository: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ArchiveIndex {
    format_version: u16,
    snapshots: Vec<ArchiveSnapshot>,
}

pub fn default_archive_root() -> Result<PathBuf, String> {
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the current user's local application-data directory.".to_string())?;
    Ok(local_data
        .join("kastrick's_software")
        .join("data")
        .join("GitManager")
        .join("archives"))
}

pub fn capture_repository(
    repository_path: &str,
    archive_root: Option<&str>,
    kind_text: &str,
) -> Result<ArchiveSnapshot, String> {
    let kind = ArchiveKind::parse(kind_text)?;
    let source = PathBuf::from(repository_path);
    let source = fs::canonicalize(&source)
        .map_err(|error| format!("Could not resolve repository path: {error}"))?;
    let repository = Repository::open(&source)
        .map_err(|error| format!("The selected path is not a readable Git repository: {error}"))?;
    if repository.is_bare() {
        return Err("Bare repositories cannot be archived as working repositories.".to_string());
    }
    let git_marker = source.join(".git");
    if kind == ArchiveKind::Remote && git_marker.is_file() {
        return Err("Remote Archive currently requires a self-contained .git directory; linked worktrees store Git metadata outside the worktree.".to_string());
    }

    let root = match archive_root {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => default_archive_root()?,
    };
    fs::create_dir_all(&root)
        .map_err(|error| format!("Could not create archive storage directory: {error}"))?;
    let root = fs::canonicalize(&root)
        .map_err(|error| format!("Could not resolve archive storage directory: {error}"))?;
    if root.starts_with(&source) {
        return Err("The archive storage directory cannot be inside the repository being archived.".to_string());
    }

    let repository_id = repository_id(&source);
    let repository_name = source.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repository")
        .to_string();
    let head = repository.head().ok();
    let branch = head.as_ref()
        .and_then(|value| value.shorthand())
        .unwrap_or("HEAD")
        .to_string();
    let head_oid = head.and_then(|value| value.target()).map(|oid| oid.to_string());
    let timestamp = now_ms()?;
    let snapshot_id = format!("{}-{}", timestamp, unique_suffix());
    let kind_dir = root.join("repositories").join(&repository_id).join(kind.as_str());
    let snapshots_dir = kind_dir.join("snapshots");
    fs::create_dir_all(&snapshots_dir)
        .map_err(|error| format!("Could not create repository snapshot directory: {error}"))?;
    let final_archive = snapshots_dir.join(format!("{snapshot_id}.tar.gz"));
    let temporary_archive = snapshots_dir.join(format!(".{snapshot_id}.tar.gz.part"));

    let (file_count, source_bytes, excluded_paths) =
        build_archive(&source, &temporary_archive, kind, &repository_name, &branch, head_oid.as_deref())?;
    let archive_bytes = fs::metadata(&temporary_archive)
        .map_err(|error| format!("Could not inspect staged archive: {error}"))?
        .len();
    let digest = sha256_file(&temporary_archive)
        .map_err(|error| format!("Could not verify staged archive hash: {error}"))?;
    verify_archive(&temporary_archive)
        .map_err(|error| format!("Staged archive verification failed: {error}"))?;
    fs::rename(&temporary_archive, &final_archive)
        .map_err(|error| format!("Could not publish verified archive: {error}"))?;

    let snapshot = ArchiveSnapshot {
        id: snapshot_id.clone(),
        repository_id: repository_id.clone(),
        repository_name,
        repository_path: source.to_string_lossy().to_string(),
        kind,
        captured_at: chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp)
            .map(|date| date.to_rfc3339())
            .unwrap_or_else(|| timestamp.to_string()),
        captured_at_unix_ms: timestamp,
        branch,
        head: head_oid,
        file_count,
        source_bytes,
        archive_bytes,
        sha256: digest,
        archive_path: final_archive.to_string_lossy().to_string(),
        excluded_paths,
    };

    let cloud_store = open_local_cloud_store(&root)?;
    let logical_path = cloud_archive_path(&snapshot);
    cloud_store.store_file(&final_archive, &logical_path)
        .map_err(|error| format!("Could not add archive to the content-addressed store: {error}"))?;

    let index_path = kind_dir.join("index.json");
    let mut index = read_index(&index_path)?;
    index.format_version = ARCHIVE_FORMAT_VERSION;
    let mut previous = std::mem::take(&mut index.snapshots);
    previous.retain(|item| item.kind == kind && item.repository_id == repository_id);
    previous.sort_by(|a, b| b.captured_at_unix_ms.cmp(&a.captured_at_unix_ms));
    let mut retained = vec![snapshot.clone()];
    retained.extend(previous.into_iter().filter(|item| item.id != snapshot_id).take(MAX_RESTORE_POINTS - 1));
    let retained_ids = retained.iter().map(|item| item.id.clone()).collect::<std::collections::HashSet<_>>();
    let removed = index.snapshots.iter()
        .filter(|item| !retained_ids.contains(&item.id))
        .cloned()
        .collect::<Vec<_>>();
    index.snapshots = retained;
    if let Err(error) = write_index_atomic(&index_path, &index) {
        let _ = fs::remove_file(&final_archive);
        let _ = cloud_store.remove_logical_path(&logical_path);
        return Err(error);
    }

    // Retention happens only after the verified new archive and its index are durable.
    for old in removed {
        let path = PathBuf::from(&old.archive_path);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
        let _ = cloud_store.remove_logical_path(&cloud_archive_path(&old));
    }
    Ok(snapshot)
}

pub fn list_repository_archives(
    repository_path: &str,
    archive_root: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<ArchiveSnapshot>, String> {
    let repository = absolute_path(repository_path)?;
    let id = repository_id(&repository);
    let root = match archive_root {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => default_archive_root()?,
    };
    let kinds = match kind {
        Some(value) => vec![ArchiveKind::parse(value)?],
        None => vec![ArchiveKind::Local, ArchiveKind::Remote],
    };
    let mut snapshots = Vec::new();
    for archive_kind in kinds {
        let index_path = root.join("repositories").join(&id).join(archive_kind.as_str()).join("index.json");
        let index = read_index(&index_path)?;
        for snapshot in index.snapshots {
            if snapshot.kind == archive_kind && snapshot.repository_id == id {
                snapshots.push(snapshot);
            }
        }
    }
    snapshots.sort_by(|a, b| b.captured_at_unix_ms.cmp(&a.captured_at_unix_ms));
    Ok(snapshots)
}

pub fn restore_repository_archive(
    destination_path: &str,
    archive_root: Option<&str>,
    kind_text: &str,
    snapshot_id: &str,
    confirm_replace: bool,
) -> Result<ArchiveRestoreResult, String> {
    let kind = ArchiveKind::parse(kind_text)?;
    let destination = absolute_path(destination_path)?;
    let root = match archive_root {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => default_archive_root()?,
    };
    let repository_id = repository_id(&destination);
    let index_path = root.join("repositories").join(&repository_id).join(kind.as_str()).join("index.json");
    let index = read_index(&index_path)?;
    let snapshot = index.snapshots.into_iter()
        .find(|item| item.id == snapshot_id && item.kind == kind)
        .ok_or_else(|| format!("Restore point {snapshot_id} was not found for this repository/archive type."))?;
    let archive_path = PathBuf::from(&snapshot.archive_path);
    if !archive_path.is_file() {
        return Err("The selected archive payload is missing from local storage.".to_string());
    }
    let actual_hash = sha256_file(&archive_path)
        .map_err(|error| format!("Could not verify archive integrity: {error}"))?;
    if actual_hash != snapshot.sha256 {
        return Err("Archive integrity check failed: SHA-256 does not match its index.".to_string());
    }
    verify_archive(&archive_path)
        .map_err(|error| format!("Archive validation failed: {error}"))?;

    let parent = destination.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "Restore destination must have a parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create restore parent directory: {error}"))?;
    let stage = parent.join(format!(".gitmanager-restore-{}-{}", snapshot.id, unique_suffix()));
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|error| format!("Could not clear stale restore staging directory: {error}"))?;
    }
    fs::create_dir_all(&stage)
        .map_err(|error| format!("Could not create restore staging directory: {error}"))?;
    if let Err(error) = extract_archive(&archive_path, &stage) {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }

    let existing = destination.exists();
    let non_empty = existing && fs::read_dir(&destination)
        .map_err(|error| format!("Could not inspect restore destination: {error}"))?
        .next().is_some();
    if non_empty && !confirm_replace {
        let _ = fs::remove_dir_all(&stage);
        return Err("Restore destination is not empty. Preview the restore and confirm replacement explicitly.".to_string());
    }

    let backup_path = if non_empty {
        Some(parent.join(format!(
            ".{}.gitmanager-before-restore-{}",
            destination.file_name().and_then(|v| v.to_str()).unwrap_or("repository"),
            snapshot.id
        )))
    } else {
        None
    };
    if let Some(backup) = &backup_path {
        if backup.exists() {
            let _ = fs::remove_dir_all(backup);
        }
        fs::rename(&destination, backup)
            .map_err(|error| format!("Could not preserve the current destination before restore: {error}"))?;
    } else if existing {
        fs::remove_dir(&destination)
            .map_err(|error| format!("Could not remove the empty restore destination: {error}"))?;
    }

    if let Err(error) = fs::rename(&stage, &destination) {
        if let Some(backup) = &backup_path {
            let _ = fs::rename(backup, &destination);
        }
        let _ = fs::remove_dir_all(&stage);
        return Err(format!("Could not activate restored repository: {error}"));
    }

    let mut initialized_git = false;
    if kind == ArchiveKind::Local {
        // Local Archive intentionally omits .git; keep the destination's original
        // Git metadata when available, or initialize a new repository on recovery.
        if let Some(backup) = &backup_path {
            let old_git = backup.join(".git");
            if old_git.exists() {
                if let Err(error) = fs::rename(&old_git, destination.join(".git")) {
                    let failed = parent.join(format!(".gitmanager-restore-failed-{}", snapshot.id));
                    let _ = fs::rename(&destination, &failed);
                    let _ = fs::rename(backup, &destination);
                    let _ = fs::remove_dir_all(failed);
                    return Err(format!("Could not preserve existing Git metadata: {error}"));
                }
            }
        }
        if !destination.join(".git").exists() {
            Repository::init(&destination)
                .map_err(|error| format!("Files were restored, but Git initialization failed: {error}"))?;
            initialized_git = true;
        }
    } else {
        let git_dir = destination.join(".git");
        if !git_dir.is_dir() {
            return Err("Remote Archive restored without a self-contained .git directory.".to_string());
        }
        write_minimal_git_config(&git_dir.join("config"))
            .map_err(|error| format!("Files restored, but Git configuration could not be initialized: {error}"))?;
        Repository::open(&destination)
            .map_err(|error| format!("Restored Git metadata could not be opened: {error}"))?;
    }

    Ok(ArchiveRestoreResult {
        snapshot,
        destination: destination.to_string_lossy().to_string(),
        preserved_previous_path: backup_path.map(|path| path.to_string_lossy().to_string()),
        initialized_git_repository: initialized_git,
    })
}

fn build_archive(
    source: &Path,
    target: &Path,
    kind: ArchiveKind,
    _repository_name: &str,
    _branch: &str,
    _head: Option<&str>,
) -> Result<(u64, u64, Vec<String>), String> {
    let output = File::create(target)
        .map_err(|error| format!("Could not create staged archive: {error}"))?;
    let encoder = GzEncoder::new(output, Compression::fast());
    let mut builder = TarBuilder::new(encoder);
    let mut file_count = 0u64;
    let mut source_bytes = 0u64;
    let mut excluded = Vec::new();

    for entry in WalkDir::new(source).min_depth(1).follow_links(false) {
        let entry = entry.map_err(|error| format!("Could not read repository entry: {error}"))?;
        let path = entry.path();
        let relative = path.strip_prefix(source)
            .map_err(|error| format!("Could not compute repository-relative path: {error}"))?;
        let relative_text = relative.to_string_lossy().replace('\\', "/");
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            record_excluded(&mut excluded, relative_text);
            continue;
        }
        if kind == ArchiveKind::Local && relative.components().next()
            .is_some_and(|component| component.as_os_str() == ".git") {
            record_excluded(&mut excluded, relative_text);
            continue;
        }
        if is_default_secret_path(relative) || is_git_sensitive_path(relative) {
            record_excluded(&mut excluded, relative_text);
            continue;
        }
        if file_type.is_dir() {
            builder.append_dir(relative, path)
                .map_err(|error| format!("Could not add directory to archive: {error}"))?;
        } else if file_type.is_file() {
            let size = entry.metadata()
                .map_err(|error| format!("Could not inspect repository file: {error}"))?
                .len();
            builder.append_path_with_name(path, relative)
                .map_err(|error| format!("Could not add file to archive: {error}"))?;
            file_count = file_count.saturating_add(1);
            source_bytes = source_bytes.saturating_add(size);
        }
    }

    let encoder = builder.into_inner()
        .map_err(|error| format!("Could not finalize tar archive: {error}"))?;
    encoder.finish()
        .map_err(|error| format!("Could not finish gzip archive: {error}"))?;
    Ok((file_count, source_bytes, excluded))
}

fn is_git_sensitive_path(relative: &Path) -> bool {
    let mut components = relative.components();
    let Some(first) = components.next() else { return false; };
    if first.as_os_str() != ".git" { return false; }
    let remaining = components.map(|part| part.as_os_str().to_string_lossy().to_ascii_lowercase()).collect::<Vec<_>>();
    if remaining.is_empty() { return false; }
    if remaining[0] == "config" || remaining[0] == "config.lock" || remaining[0] == "index.lock" {
        return true;
    }
    remaining.first().is_some_and(|part| part == "hooks")
}

fn record_excluded(paths: &mut Vec<String>, path: String) {
    if paths.len() < MAX_EXCLUDED_PATHS {
        paths.push(path);
    }
}

fn verify_archive(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("Could not open archive: {error}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = TarArchive::new(decoder);
    for entry in archive.entries().map_err(|error| format!("Could not read tar index: {error}"))? {
        let entry = entry.map_err(|error| format!("Could not verify tar entry: {error}"))?;
        let path = entry.path().map_err(|error| format!("Archive contains an invalid path: {error}"))?;
        validate_relative_archive_path(&path)?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err("Archive contains a link or special filesystem entry; refusing to restore it.".to_string());
        }
    }
    Ok(())
}

fn extract_archive(path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("Could not open archive: {error}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = TarArchive::new(decoder);
    for item in archive.entries().map_err(|error| format!("Could not enumerate archive entries: {error}"))? {
        let mut entry = item.map_err(|error| format!("Could not read archive entry: {error}"))?;
        let relative = entry.path()
            .map_err(|error| format!("Archive contains an invalid path: {error}"))?
            .into_owned();
        validate_relative_archive_path(&relative)?;
        let target = destination.join(&relative);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| format!("Could not create restored directory: {error}"))?;
        } else if kind.is_file() {
            let parent = target.parent().ok_or_else(|| "Archive entry has no parent directory.".to_string())?;
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create restored parent directory: {error}"))?;
            let mut output = OpenOptions::new().write(true).create_new(true).open(&target)
                .map_err(|error| format!("Could not create restored file: {error}"))?;
            io::copy(&mut entry, &mut output)
                .map_err(|error| format!("Could not write restored file: {error}"))?;
            output.flush().map_err(|error| format!("Could not flush restored file: {error}"))?;
        } else {
            return Err("Archive contains a link or special filesystem entry; refusing to restore it.".to_string());
        }
    }
    Ok(())
}

fn validate_relative_archive_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("Archive contains an absolute or empty path.".to_string());
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("Archive contains a path traversal component.".to_string());
        }
    }
    Ok(())
}

fn write_minimal_git_config(path: &Path) -> io::Result<()> {
    if path.exists() { return Ok(()); }
    fs::write(path, "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n")
}

fn open_local_cloud_store(root: &Path) -> Result<CloudNodeStore, String> {
    let store_root = root.join(".cloud-node");
    let settings: CloudNodeSettings = serde_json::from_value(serde_json::json!({
        "formatVersion": 1,
        "node": {
            "id": "gitmanager-archives",
            "mode": "archive",
            "storageRoot": store_root,
            "backupVersions": MAX_RESTORE_POINTS,
            "preserveOriginal": true,
            "videoChunkBytes": 4194304
        }
    })).map_err(|error| format!("Could not initialize archive storage settings: {error}"))?;
    CloudNodeStore::open(&settings)
        .map_err(|error| format!("Could not open the local Cloud Node archive store: {error}"))
}

fn cloud_archive_path(snapshot: &ArchiveSnapshot) -> String {
    format!(
        "gitmanager/{}/{}/{}.tar.gz",
        snapshot.repository_id,
        snapshot.kind.as_str(),
        snapshot.id
    )
}

fn read_index(path: &Path) -> Result<ArchiveIndex, String> {
    if !path.is_file() {
        return Ok(ArchiveIndex { format_version: ARCHIVE_FORMAT_VERSION, snapshots: Vec::new() });
    }
    let metadata = fs::metadata(path).map_err(|error| format!("Could not inspect archive index: {error}"))?;
    if metadata.len() > MAX_INDEX_BYTES {
        return Err("Archive index exceeds the 2 MiB safety limit.".to_string());
    }
    let bytes = fs::read(path).map_err(|error| format!("Could not read archive index: {error}"))?;
    let index: ArchiveIndex = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Archive index is invalid JSON: {error}"))?;
    if index.format_version > ARCHIVE_FORMAT_VERSION {
        return Err("Archive index was created by a newer GitManager version.".to_string());
    }
    if index.snapshots.len() > MAX_RESTORE_POINTS {
        return Err("Archive index exceeds the five-snapshot retention limit.".to_string());
    }
    Ok(index)
}

fn write_index_atomic(path: &Path, index: &ArchiveIndex) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "Archive index has no parent directory.".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("Could not create archive index directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(index).map_err(|error| format!("Could not serialize archive index: {error}"))?;
    if bytes.len() > MAX_INDEX_BYTES as usize {
        return Err("Archive index exceeds the 2 MiB safety limit.".to_string());
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(|error| format!("Could not write staged archive index: {error}"))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not publish archive index atomically: {error}"));
    }
    Ok(())
}

fn repository_id(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    hex::encode(&digest[..16])
}

fn absolute_path(path: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() { Ok(candidate) }
    else { std::env::current_dir().map(|cwd| cwd.join(candidate)).map_err(|error| format!("Could not resolve current directory: {error}")) }
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 { break; }
        digest.update(&buffer[..size]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn now_ms() -> Result<u64, String> {
    SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis().try_into().unwrap_or(u64::MAX))
        .map_err(|error| format!("System clock is before Unix epoch: {error}"))
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos()).unwrap_or_default();
    format!("{}-{}", std::process::id(), nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_path_filter_is_applied_to_both_archive_kinds() {
        assert!(is_default_secret_path(Path::new(".env")));
        assert!(is_default_secret_path(Path::new("nested/.env.prod")));
        assert!(is_default_secret_path(Path::new("secrets/token.json")));
        assert!(!is_default_secret_path(Path::new(".env.example")));
    }

    #[test]
    fn local_archive_excludes_git_and_remote_archive_keeps_it() {
        let git_path = Path::new(".git/objects/pack/pack-abc.pack");
        assert!(ArchiveKind::Local == ArchiveKind::Local);
        assert_eq!(git_path.components().next().unwrap().as_os_str(), ".git");
        assert!(!is_default_secret_path(git_path));
    }

    #[test]
    fn archive_path_validation_rejects_parent_components() {
        assert!(validate_relative_archive_path(Path::new("../escape")).is_err());
        assert!(validate_relative_archive_path(Path::new("/absolute")).is_err());
        assert!(validate_relative_archive_path(Path::new("src/main.rs")).is_ok());
    }
}
