use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use atomic_io::AtomicIo;
use cloud_node::{CloudNodeSettings, CloudNodeStore, is_default_secret_path};
use git2::Repository;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

const ARCHIVE_FORMAT_VERSION: u16 = 1;
const MAX_RESTORE_POINTS: usize = 5;
const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXCLUDED_PATHS: usize = 4096;
const COPY_BUFFER_BYTES: usize = 1024 * 1024;

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

    pub fn as_str(self) -> &'static str {
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
    pub directory_count: u64,
    pub source_bytes: u64,
    /// Newly added unique blob bytes; unchanged file content is shared between snapshots.
    pub stored_bytes: u64,
    pub manifest_sha256: String,
    /// Cloud Node logical object key for the immutable restore-point manifest.
    pub manifest_logical_path: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotManifest {
    format_version: u16,
    repository_id: String,
    snapshot_id: String,
    kind: ArchiveKind,
    directories: Vec<String>,
    files: Vec<SnapshotFile>,
    excluded_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotFile {
    relative_path: String,
    blob_logical_path: String,
    content_sha256: String,
    size: u64,
    mode: u32,
}

#[derive(Debug)]
struct ScannedFile {
    source: PathBuf,
    relative_path: String,
    size: u64,
    mode: u32,
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
    if kind == ArchiveKind::Remote && source.join(".git").is_file() {
        return Err("Remote Archive currently requires a self-contained .git directory; linked worktrees keep Git metadata outside the worktree.".to_string());
    }

    let root = resolve_archive_root(archive_root)?;
    if root.starts_with(&source) || root == source {
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
    let store = open_archive_store(&root)?;
    let staging = root.join(".staging").join(&repository_id).join(&snapshot_id);
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Could not create archive staging directory: {error}"))?;

    let (scanned_files, directories, excluded_paths) =
        scan_repository(&source, kind)?;
    let mut files = Vec::with_capacity(scanned_files.len());
    let mut source_bytes = 0u64;
    let mut stored_bytes = 0u64;

    for (index, item) in scanned_files.iter().enumerate() {
        source_bytes = source_bytes.saturating_add(item.size);
        let source_hash = sha256_file(&item.source)
            .map_err(|error| format!("Could not hash {}: {error}", item.relative_path))?;
        let blob_path = blob_logical_path(&repository_id, kind, &source_hash);
        if !store.verify_logical_file(&blob_path, &source_hash)
            .map_err(|error| format!("Could not verify existing archive blob: {error}"))?
        {
            let staged_file = staging.join(format!("file-{index}.blob"));
            fs::copy(&item.source, &staged_file)
                .map_err(|error| format!("Could not stage {}: {error}", item.relative_path))?;
            File::open(&staged_file)
                .and_then(|file| file.sync_all())
                .map_err(|error| format!("Could not flush staged file {}: {error}", item.relative_path))?;
            let staged_hash = sha256_file(&staged_file)
                .map_err(|error| format!("Could not verify staged file {}: {error}", item.relative_path))?;
            if staged_hash != source_hash {
                let _ = fs::remove_file(&staged_file);
                let _ = fs::remove_dir_all(&staging);
                return Err(format!("{} changed while the archive was being captured; retry the snapshot.", item.relative_path));
            }
            let stored = store.store_file(&staged_file, &blob_path)
                .map_err(|error| format!("Could not store {} in the content-addressed archive: {error}", item.relative_path))?;
            let _ = fs::remove_file(&staged_file);
            if stored.content_sha256 != source_hash {
                let _ = store.remove_logical_path(&blob_path);
                let _ = fs::remove_dir_all(&staging);
                return Err(format!("Stored content hash mismatch for {}.", item.relative_path));
            }
            stored_bytes = stored_bytes.saturating_add(item.size);
        }

        files.push(SnapshotFile {
            relative_path: item.relative_path.clone(),
            blob_logical_path: blob_path,
            content_sha256: source_hash,
            size: item.size,
            mode: item.mode,
        });
    }

    let mut manifest = SnapshotManifest {
        format_version: ARCHIVE_FORMAT_VERSION,
        repository_id: repository_id.clone(),
        snapshot_id: snapshot_id.clone(),
        kind,
        directories,
        files,
        excluded_paths: excluded_paths.clone(),
    };
    manifest.directories.sort();
    manifest.files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|error| format!("Could not serialize restore-point manifest: {error}"))?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        let _ = fs::remove_dir_all(&staging);
        return Err("Repository snapshot manifest exceeds the 64 MiB safety limit.".to_string());
    }
    let manifest_sha256 = sha256_bytes(&manifest_bytes);
    let manifest_logical_path = manifest_logical_path(&repository_id, kind, &snapshot_id);
    let staged_manifest = staging.join("manifest.json");
    fs::write(&staged_manifest, &manifest_bytes)
        .map_err(|error| format!("Could not stage restore-point manifest: {error}"))?;
    store.store_file(&staged_manifest, &manifest_logical_path)
        .map_err(|error| format!("Could not store restore-point manifest: {error}"))?;

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
        file_count: manifest.files.len() as u64,
        directory_count: manifest.directories.len() as u64,
        source_bytes,
        stored_bytes,
        manifest_sha256,
        manifest_logical_path: manifest_logical_path.clone(),
        excluded_paths,
    };

    let index_path = local_index_path(&root, &repository_id, kind);
    let index_key = index_logical_path(&repository_id, kind);
    let mut index = read_index(&index_path, &store, &index_key)?;
    index.format_version = ARCHIVE_FORMAT_VERSION;
    let mut previous = std::mem::take(&mut index.snapshots);
    previous.retain(|item| item.repository_id == repository_id && item.kind == kind && item.id != snapshot_id);
    previous.sort_by(|a, b| b.captured_at_unix_ms.cmp(&a.captured_at_unix_ms));

    let mut retained = vec![snapshot.clone()];
    let mut removed = Vec::new();
    for old in previous {
        if retained.len() < MAX_RESTORE_POINTS {
            retained.push(old);
        } else {
            removed.push(old);
        }
    }
    let next_index = ArchiveIndex {
        format_version: ARCHIVE_FORMAT_VERSION,
        snapshots: retained.clone(),
    };
    let index_bytes = serde_json::to_vec_pretty(&next_index)
        .map_err(|error| format!("Could not serialize archive index: {error}"))?;
    let staged_index = staging.join("index.json");
    fs::write(&staged_index, &index_bytes)
        .map_err(|error| format!("Could not stage archive index: {error}"))?;
    store.store_file(&staged_index, &index_key)
        .map_err(|error| format!("Could not publish archive index into Cloud Node storage: {error}"))?;
    write_index_atomic(&index_path, &next_index)?;

    // Keep all blobs referenced by the retained complete snapshots. Immutable
    // manifests and blobs are pruned only after the next index is durable.
    let mut retained_blobs = HashSet::<String>::new();
    for kept in &retained {
        let kept_manifest = if kept.id == snapshot_id {
            manifest.clone()
        } else {
            read_manifest(&store, kept)?
        };
        for file in kept_manifest.files {
            retained_blobs.insert(file.blob_logical_path);
        }
    }
    for old in removed {
        let _ = store.remove_logical_path(&old.manifest_logical_path);
    }
    // The active Cloud Node set is the union of the retained restore points.
    // List the old manifest before pruning to discover no-longer-referenced blobs.
    // Snapshot blobs are namespaced by repository/archive kind and content hash.
    let prefix = blob_prefix(&repository_id, kind);
    let plan = store.sync_plan()
        .map_err(|error| format!("Could not inspect retained archive blobs: {error}"))?;
    let stale_blobs = plan.ordered()
        .filter(|object| object.logical_path.starts_with(&prefix))
        .filter(|object| !retained_blobs.contains(&object.logical_path))
        .map(|object| object.logical_path.clone())
        .collect::<Vec<_>>();
    for stale in stale_blobs {
        let _ = store.remove_logical_path(&stale);
    }

    let _ = fs::remove_dir_all(&staging);
    Ok(snapshot)
}

pub fn list_repository_archives(
    repository_path: &str,
    archive_root: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<ArchiveSnapshot>, String> {
    let identity_path = repository_identity_path(repository_path)?;
    let id = repository_id(&identity_path);
    let root = resolve_archive_root(archive_root)?;
    let store = open_archive_store(&root)?;
    let kinds = match kind {
        Some(value) => vec![ArchiveKind::parse(value)?],
        None => vec![ArchiveKind::Local, ArchiveKind::Remote],
    };
    let mut snapshots = Vec::new();
    for archive_kind in kinds {
        let index = read_index(
            &local_index_path(&root, &id, archive_kind),
            &store,
            &index_logical_path(&id, archive_kind),
        )?;
        snapshots.extend(index.snapshots.into_iter().filter(|item| {
            item.kind == archive_kind && item.repository_id == id
        }));
    }
    snapshots.sort_by(|a, b| b.captured_at_unix_ms.cmp(&a.captured_at_unix_ms));
    Ok(snapshots)
}

pub fn restore_repository_archive(
    repository_path: &str,
    destination_path: &str,
    archive_root: Option<&str>,
    kind_text: &str,
    snapshot_id: &str,
    confirm_replace: bool,
) -> Result<ArchiveRestoreResult, String> {
    let kind = ArchiveKind::parse(kind_text)?;
    let identity_path = repository_identity_path(repository_path)?;
    let repository_id = repository_id(&identity_path);
    let destination = absolute_path(destination_path)?;
    let root = resolve_archive_root(archive_root)?;
    let store = open_archive_store(&root)?;
    let index_path = local_index_path(&root, &repository_id, kind);
    let index = read_index(&index_path, &store, &index_logical_path(&repository_id, kind))?;
    let snapshot = index.snapshots.into_iter()
        .find(|item| item.id == snapshot_id && item.kind == kind && item.repository_id == repository_id)
        .ok_or_else(|| format!("Restore point {snapshot_id} was not found for this repository/archive type."))?;
    let manifest = read_manifest(&store, &snapshot)?;
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|error| format!("Could not serialize selected restore point: {error}"))?;
    if sha256_bytes(&manifest_bytes) != snapshot.manifest_sha256 {
        return Err("Restore-point manifest integrity check failed.".to_string());
    }
    validate_manifest(&manifest, &snapshot)?;

    let parent = destination.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "Restore destination must have a parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create restore parent directory: {error}"))?;
    let stage = parent.join(format!(".gitmanager-restore-{}-{}", snapshot.id, unique_suffix()));
    if stage.exists() {
        fs::remove_dir_all(&stage)
            .map_err(|error| format!("Could not clear stale restore staging directory: {error}"))?;
    }
    fs::create_dir_all(&stage)
        .map_err(|error| format!("Could not create restore staging directory: {error}"))?;

    for directory in &manifest.directories {
        let relative = validate_relative_path(directory)?;
        let target = stage.join(relative);
        fs::create_dir_all(&target)
            .map_err(|error| format!("Could not create restored directory {directory}: {error}"))?;
    }
    for file in &manifest.files {
        let relative = validate_relative_path(&file.relative_path)?;
        if is_default_secret_path(relative) || (kind == ArchiveKind::Remote && is_git_sensitive_path(relative)) {
            let _ = fs::remove_dir_all(&stage);
            return Err(format!("Restore-point manifest contains a path excluded by the security policy: {}", file.relative_path));
        }
        let expected_blob = blob_logical_path(&repository_id, kind, &file.content_sha256);
        if expected_blob != file.blob_logical_path {
            let _ = fs::remove_dir_all(&stage);
            return Err(format!("Restore-point manifest contains an invalid content address for {}", file.relative_path));
        }
        let target = stage.join(relative);
        let target_parent = target.parent()
            .ok_or_else(|| "Restored file has no parent directory.".to_string())?;
        fs::create_dir_all(target_parent)
            .map_err(|error| format!("Could not create restored file parent: {error}"))?;
        if !store.restore_logical_file(&file.blob_logical_path, &target, &file.content_sha256)
            .map_err(|error| format!("Could not restore {}: {error}", file.relative_path))?
        {
            let _ = fs::remove_dir_all(&stage);
            return Err(format!("The archived payload for {} is missing.", file.relative_path));
        }
        set_file_mode(&target, file.mode)
            .map_err(|error| format!("Could not restore file permissions for {}: {error}", file.relative_path))?;
    }

    if kind == ArchiveKind::Remote && !stage.join(".git").is_dir() {
        let _ = fs::remove_dir_all(&stage);
        return Err("Remote Archive restore point does not contain a self-contained .git directory.".to_string());
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
            destination.file_name().and_then(|value| value.to_str()).unwrap_or("repository"),
            snapshot.id
        )))
    } else {
        None
    };
    if let Some(backup) = &backup_path {
        if backup.exists() {
            return Err(format!("Refusing to overwrite an existing safety copy: {}", backup.display()));
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
        if let Some(backup) = &backup_path {
            let old_git = backup.join(".git");
            if old_git.is_dir() {
                copy_directory_tree(&old_git, &destination.join(".git"))
                    .map_err(|error| format!("Files restored, but existing Git metadata could not be copied: {error}"))?;
            }
        }
        if !destination.join(".git").exists() {
            Repository::init(&destination)
                .map_err(|error| format!("Files restored, but Git initialization failed: {error}"))?;
            initialized_git = true;
        }
    } else {
        let git_dir = destination.join(".git");
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

fn scan_repository(
    source: &Path,
    kind: ArchiveKind,
) -> Result<(Vec<ScannedFile>, Vec<String>, Vec<String>), String> {
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut excluded = Vec::new();
    let walker = WalkDir::new(source)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let Ok(relative) = entry.path().strip_prefix(source) else { return false; };
            let relative_text = relative.to_string_lossy().replace('\\', "/");
            let file_type = entry.file_type();
            let skip = file_type.is_symlink()
                || (kind == ArchiveKind::Local && starts_with_git_directory(relative))
                || is_default_secret_path(relative)
                || (kind == ArchiveKind::Remote && is_git_sensitive_path(relative));
            if skip {
                record_excluded(&mut excluded, relative_text);
            }
            !skip
        });

    for entry in walker {
        let entry = entry.map_err(|error| format!("Could not inspect repository entry: {error}"))?;
        let path = entry.path();
        let relative = path.strip_prefix(source)
            .map_err(|error| format!("Could not compute repository-relative path: {error}"))?;
        let relative_path = relative.to_str()
            .ok_or_else(|| format!("Repository path is not valid UTF-8: {}", relative.display()))?
            .replace('\\', "/");
        let file_type = entry.file_type();
        if file_type.is_dir() {
            directories.push(relative_path);
        } else if file_type.is_file() {
            let metadata = entry.metadata()
                .map_err(|error| format!("Could not inspect {}: {error}", relative.display()))?;
            files.push(ScannedFile {
                source: path.to_path_buf(),
                relative_path,
                size: metadata.len(),
                mode: file_mode(&metadata),
            });
        }
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    directories.sort();
    Ok((files, directories, excluded))
}

fn starts_with_git_directory(relative: &Path) -> bool {
    relative.components().next().is_some_and(|part| part.as_os_str() == ".git")
}

fn is_git_sensitive_path(relative: &Path) -> bool {
    let mut components = relative.components();
    let Some(first) = components.next() else { return false; };
    if first.as_os_str() != ".git" {
        return false;
    }
    let remaining = components
        .map(|part| part.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if remaining.is_empty() {
        return false;
    }
    matches!(remaining[0].as_str(), "config" | "config.lock" | "index.lock" | "hooks")
}

fn blob_prefix(repository_id: &str, kind: ArchiveKind) -> String {
    format!("gitmanager/repositories/{repository_id}/{}/blobs/", kind.as_str())
}

fn blob_logical_path(repository_id: &str, kind: ArchiveKind, sha256: &str) -> String {
    format!("{}{}", blob_prefix(repository_id, kind), sha256)
}

fn manifest_logical_path(repository_id: &str, kind: ArchiveKind, snapshot_id: &str) -> String {
    format!(
        "gitmanager/repositories/{repository_id}/{}/snapshots/{snapshot_id}.json",
        kind.as_str()
    )
}

fn index_logical_path(repository_id: &str, kind: ArchiveKind) -> String {
    format!("gitmanager/repositories/{repository_id}/{}/index.json", kind.as_str())
}

fn local_index_path(root: &Path, repository_id: &str, kind: ArchiveKind) -> PathBuf {
    root.join("repositories").join(repository_id).join(kind.as_str()).join("index.json")
}

fn resolve_archive_root(value: Option<&str>) -> Result<PathBuf, String> {
    let candidate = match value {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => default_archive_root()?,
    };
    fs::create_dir_all(&candidate)
        .map_err(|error| format!("Could not create archive storage directory: {error}"))?;
    candidate.canonicalize()
        .map_err(|error| format!("Could not resolve archive storage directory: {error}"))
}

fn open_archive_store(root: &Path) -> Result<CloudNodeStore, String> {
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
        .map_err(|error| format!("Could not open the Cloud Node archive store: {error}"))
}

fn read_index(path: &Path, store: &CloudNodeStore, logical_path: &str) -> Result<ArchiveIndex, String> {
    let bytes = if path.is_file() {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("Could not inspect archive index: {error}"))?;
        if metadata.len() > MAX_INDEX_BYTES {
            return Err("Archive index exceeds the 2 MiB safety limit.".to_string());
        }
        fs::read(path).map_err(|error| format!("Could not read archive index: {error}"))?
    } else {
        match store.read_logical_file(logical_path, MAX_INDEX_BYTES)
            .map_err(|error| format!("Could not recover archive index from Cloud Node: {error}"))? {
            Some(bytes) => bytes,
            None => return Ok(ArchiveIndex { format_version: ARCHIVE_FORMAT_VERSION, snapshots: Vec::new() }),
        }
    };
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

fn read_manifest(store: &CloudNodeStore, snapshot: &ArchiveSnapshot) -> Result<SnapshotManifest, String> {
    let bytes = store.read_logical_file(&snapshot.manifest_logical_path, MAX_MANIFEST_BYTES)
        .map_err(|error| format!("Could not read restore-point manifest: {error}"))?
        .ok_or_else(|| format!("Restore-point manifest {} is missing from Cloud Node storage.", snapshot.id))?;
    if sha256_bytes(&bytes) != snapshot.manifest_sha256 {
        return Err(format!("Restore-point manifest {} failed its SHA-256 check.", snapshot.id));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("Restore-point manifest is invalid JSON: {error}"))
}

fn validate_manifest(manifest: &SnapshotManifest, snapshot: &ArchiveSnapshot) -> Result<(), String> {
    if manifest.format_version != ARCHIVE_FORMAT_VERSION
        || manifest.repository_id != snapshot.repository_id
        || manifest.snapshot_id != snapshot.id
        || manifest.kind != snapshot.kind
    {
        return Err("Restore-point manifest identity does not match its index entry.".to_string());
    }
    if manifest.files.len() as u64 != snapshot.file_count
        || manifest.directories.len() as u64 != snapshot.directory_count
        || manifest.files.len() > 2_000_000
        || manifest.directories.len() > 2_000_000
    {
        return Err("Restore-point manifest entry counts are invalid.".to_string());
    }
    let mut seen = HashSet::new();
    for file in &manifest.files {
        let path = validate_relative_path(&file.relative_path)?;
        if !seen.insert(file.relative_path.as_str()) {
            return Err(format!("Restore-point manifest contains duplicate path: {}", file.relative_path));
        }
        if is_default_secret_path(path)
            || (snapshot.kind == ArchiveKind::Remote && is_git_sensitive_path(path))
        {
            return Err(format!("Restore-point manifest contains a secret or restricted path: {}", file.relative_path));
        }
        let expected = blob_logical_path(&snapshot.repository_id, snapshot.kind, &file.content_sha256);
        if file.blob_logical_path != expected {
            return Err(format!("Restore-point manifest has an invalid content address: {}", file.relative_path));
        }
    }
    for directory in &manifest.directories {
        let path = validate_relative_path(directory)?;
        if snapshot.kind == ArchiveKind::Local && starts_with_git_directory(path) {
            return Err("Local Archive manifest unexpectedly contains .git.".to_string());
        }
        if is_default_secret_path(path)
            || (snapshot.kind == ArchiveKind::Remote && is_git_sensitive_path(path))
        {
            return Err(format!("Restore-point manifest contains a restricted directory: {directory}"));
        }
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<&Path, String> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() || value.contains('\\') {
        return Err(format!("Archive path is not a canonical relative path: {value}"));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(format!("Archive path contains a traversal component: {value}"));
        }
    }
    if value.contains(':') {
        return Err(format!("Archive path contains a drive or stream separator: {value}"));
    }
    Ok(path)
}

fn write_index_atomic(path: &Path, index: &ArchiveIndex) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "Archive index has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create archive index directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(index)
        .map_err(|error| format!("Could not serialize archive index: {error}"))?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err("Archive index exceeds the 2 MiB safety limit.".to_string());
    }
    AtomicIo::new().write_atomic(path, &bytes)
        .map_err(|error| format!("Could not publish archive index atomically: {error}"))?;
    Ok(())
}

fn repository_identity_path(path: &str) -> Result<PathBuf, String> {
    let candidate = absolute_path(path)?;
    Ok(candidate.canonicalize().unwrap_or(candidate))
}

fn repository_id(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    hex::encode(&digest[..16])
}

fn absolute_path(path: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        Ok(candidate)
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(candidate))
            .map_err(|error| format!("Could not resolve current directory: {error}"))
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn file_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}

fn set_file_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn copy_directory_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in WalkDir::new(source).min_depth(1).follow_links(false) {
        let entry = entry.map_err(std::io::Error::other)?;
        let relative = entry.path().strip_prefix(source).map_err(std::io::Error::other)?;
        let target = destination.join(relative);
        if entry.file_type().is_symlink() {
            continue;
        }
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn write_minimal_git_config(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n")
}

fn record_excluded(paths: &mut Vec<String>, path: String) {
    if paths.len() < MAX_EXCLUDED_PATHS {
        paths.push(path);
    }
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

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("gitmanager-archive-{label}-{}", unique_suffix()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn commit_file(repo: &Repository, path: &str, contents: &str) {
        let root = repo.workdir().unwrap();
        fs::write(root.join(path), contents).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(path)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("GitManager Test", "gitmanager-test@example.invalid").unwrap();
        let parent = repo.head().ok().and_then(|head| head.target()).and_then(|oid| repo.find_commit(oid).ok());
        let parents = parent.iter().collect::<Vec<_>>();
        let _ = repo.commit(Some("HEAD"), &signature, &signature, contents, &tree, &parents).unwrap();
    }

    #[test]
    fn captures_five_complete_local_restore_points_and_recovers_untracked_files() {
        let root = temp_root("retention");
        let repository_path = root.join("repository");
        fs::create_dir_all(&repository_path).unwrap();
        let repo = Repository::init(&repository_path).unwrap();
        fs::write(repository_path.join("untracked.txt"), "untracked-0").unwrap();
        fs::write(repository_path.join(".env"), "SECRET=do-not-archive").unwrap();

        for version in 0..6 {
            commit_file(&repo, "tracked.txt", &format!("version-{version}"));
            fs::write(repository_path.join("untracked.txt"), format!("untracked-{version}")).unwrap();
            capture_repository(
                repository_path.to_str().unwrap(),
                Some(root.join("archives").to_str().unwrap()),
                "local",
            ).unwrap();
        }

        let snapshots = list_repository_archives(
            repository_path.to_str().unwrap(),
            Some(root.join("archives").to_str().unwrap()),
            Some("local"),
        ).unwrap();
        assert_eq!(snapshots.len(), 5);
        assert_eq!(snapshots[0].file_count, 2);
        assert!(snapshots[0].excluded_paths.iter().any(|path| path == ".env"));

        let selected = snapshots.last().unwrap();
        let destination = root.join("recovered");
        restore_repository_archive(
            repository_path.to_str().unwrap(),
            destination.to_str().unwrap(),
            Some(root.join("archives").to_str().unwrap()),
            "local",
            &selected.id,
            false,
        ).unwrap();

        assert_eq!(fs::read_to_string(destination.join("untracked.txt")).unwrap(), "untracked-1");
        assert_eq!(fs::read_to_string(destination.join("tracked.txt")).unwrap(), "version-1");
        assert!(Repository::open(&destination).is_ok());
        assert!(!destination.join(".env").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_archive_restores_git_history_without_git_credentials_config() {
        let root = temp_root("remote");
        let repository_path = root.join("repository");
        fs::create_dir_all(&repository_path).unwrap();
        let repo = Repository::init(&repository_path).unwrap();
        commit_file(&repo, "README.md", "first commit");
        let snapshot = capture_repository(
            repository_path.to_str().unwrap(),
            Some(root.join("archives").to_str().unwrap()),
            "remote",
        ).unwrap();

        let destination = root.join("remote-restored");
        restore_repository_archive(
            repository_path.to_str().unwrap(),
            destination.to_str().unwrap(),
            Some(root.join("archives").to_str().unwrap()),
            "remote",
            &snapshot.id,
            false,
        ).unwrap();
        let restored = Repository::open(&destination).unwrap();
        assert!(restored.head().is_ok());
        let config = fs::read_to_string(destination.join(".git").join("config")).unwrap();
        assert!(!config.contains("credential"));
        assert_eq!(fs::read_to_string(destination.join("README.md")).unwrap(), "first commit");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn path_validation_rejects_traversal_and_drive_paths() {
        assert!(validate_relative_path("../escape").is_err());
        assert!(validate_relative_path("/absolute").is_err());
        assert!(validate_relative_path("C:/outside").is_err());
        assert!(validate_relative_path("src/main.rs").is_ok());
    }

    #[test]
    fn local_archive_excludes_git_and_credentials_by_default() {
        assert!(starts_with_git_directory(Path::new(".git/objects/pack/a.pack")));
        assert!(is_default_secret_path(Path::new(".env")));
        assert!(is_default_secret_path(Path::new("nested/.env.production")));
        assert!(is_default_secret_path(Path::new(".ssh/id_ed25519")));
        assert!(!is_default_secret_path(Path::new(".env.example")));
    }
}
