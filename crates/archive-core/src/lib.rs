use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use atomic_io::AtomicIo;
use cloud_node::{
    synchronize_provider, CloudNodeSettings, CloudNodeStore, ProviderAuthMode, ProviderClient,
    ProviderKind, ProviderSyncResult, is_default_secret_path,
};
use fs2::FileExt;
use git2::{Oid, Repository, TreeWalkMode, TreeWalkResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

const ARCHIVE_FORMAT_VERSION: u16 = 1;
const MAX_RESTORE_POINTS: usize = 5;
const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXCLUDED_PATHS: usize = 4096;
const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const CLOUD_NODE_SETTINGS_FILE: &str = "cloud-node.settings.json";

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteHistorySafetyCache {
    format_version: u16,
    refs_fingerprint: String,
    #[serde(default)]
    known_commit_tips: Vec<String>,
    #[serde(default)]
    unsafe_path: Option<String>,
    safe: bool,
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
    #[serde(default)]
    remotes: Vec<SnapshotRemote>,
    excluded_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotRemote {
    name: String,
    fetch_url: Option<String>,
    push_url: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveCloudStorageInfo {
    pub configured: bool,
    pub provider: Option<String>,
    pub namespace: Option<String>,
    pub bucket: Option<String>,
    pub target: Option<String>,
    pub auth_environment_variables: Vec<String>,
    pub note: String,
}

struct ArchiveCaptureLock {
    file: File,
}

impl ArchiveCaptureLock {
    fn acquire(root: &Path, repository_id: &str, _kind: ArchiveKind) -> Result<Self, String> {
        let lock_dir = root.join(".locks");
        fs::create_dir_all(&lock_dir)
            .map_err(|error| format!("Could not create archive lock directory: {error}"))?;
        // Local and Remote snapshots share a Cloud Node store. Serialize all
        // operations for one repository to protect index and sync-plan updates.
        let path = lock_dir.join(format!("{repository_id}.lock"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| format!("Could not open repository archive lock: {error}"))?;
        file.try_lock_exclusive().map_err(|_| {
            "Another GitManager archive operation is already running for this repository.".to_string()
        })?;
        Ok(Self { file })
    }
}

impl Drop for ArchiveCaptureLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
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
    if kind == ArchiveKind::Remote {
        validate_remote_archive_source(&source)?;
    }
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

    let repository_id = repository_archive_id(&repository, &source)?;
    let _capture_lock = ArchiveCaptureLock::acquire(&root, &repository_id, kind)?;
    if kind == ArchiveKind::Remote {
        validate_remote_archive_history(&repository, &root, &repository_id)?;
    }
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
    let remotes = if kind == ArchiveKind::Remote {
        collect_safe_remotes(&repository)?
    } else {
        Vec::new()
    };
    let timestamp = now_ms()?;
    let snapshot_id = format!("{}-{}", timestamp, unique_suffix());
    let store = open_archive_store(&root)?;
    // Pull remote-ahead restore points before local retention/index updates.
    // An offline machine can still create a local restore point from its verified cache.
    let _ = sync_archive_provider(&root, &store);
    let staging = root.join(".staging").join(&repository_id).join(&snapshot_id);
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Could not create archive staging directory: {error}"))?;

    let (mut scanned_files, mut directories, excluded_paths) =
        scan_repository(&source, kind)?;
    if kind == ArchiveKind::Remote {
        // Never copy the source object database wholesale. It can contain staged,
        // dangling or otherwise unreachable secret blobs which are not covered by
        // the reachable-ref history scan. Build a fresh pack from refs only.
        let pack_dir = staging.join("reachable-git-pack");
        let pack_files = write_reachable_git_pack(&repository, &pack_dir)?;
        for source_file in pack_files {
            let file_name = source_file.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .ok_or_else(|| "Generated Git pack filename is not valid UTF-8.".to_string())?;
            let metadata = fs::metadata(&source_file)
                .map_err(|error| format!("Could not inspect generated Git pack {file_name}: {error}"))?;
            let relative_path = format!(".git/objects/pack/{file_name}");
            scanned_files.push(ScannedFile {
                source: source_file,
                relative_path,
                size: metadata.len(),
                mode: file_mode(&metadata),
            });
        }
        directories.push(".git/objects".to_string());
        directories.push(".git/objects/pack".to_string());
        scanned_files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        directories.sort();
        directories.dedup();
    }
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
            OpenOptions::new()
                .write(true)
                .open(&staged_file)
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
        remotes,
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
        captured_at: i64::try_from(timestamp)
            .ok()
            .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
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
    if let Err(error) = sync_archive_provider(&root, &store) {
        eprintln!("GitManager archive saved locally; cloud synchronization is pending: {error}");
    }
    Ok(snapshot)
}

pub fn list_repository_archives(
    repository_path: &str,
    archive_root: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<ArchiveSnapshot>, String> {
    let identity_path = repository_identity_path(repository_path)?;
    let repository = Repository::open(&identity_path)
        .map_err(|error| format!("The selected path is not a readable Git repository: {error}"))?;
    let id = repository_archive_id(&repository, &identity_path)?;
    let root = resolve_archive_root(archive_root)?;
    let _archive_lock = ArchiveCaptureLock::acquire(&root, &id, ArchiveKind::Local)?;
    let store = open_archive_store(&root)?;
    let _ = sync_archive_provider(&root, &store);
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
    let repository = Repository::open(&identity_path)
        .map_err(|error| format!("The selected path is not a readable Git repository: {error}"))?;
    let repository_id = repository_archive_id(&repository, &identity_path)?;
    let destination = absolute_path(destination_path)?;
    let root = resolve_archive_root(archive_root)?;
    let normalized_destination = canonicalize_future_path(&destination)
        .map_err(|error| format!("Could not resolve restore destination: {error}"))?;
    if normalized_destination == root
        || normalized_destination.starts_with(&root)
        || root.starts_with(&normalized_destination)
    {
        return Err("Restore destination cannot overlap the archive storage directory. Choose a separate folder.".to_string());
    }
    if fs::symlink_metadata(&destination)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err("Restore destination cannot be a symbolic link.".to_string());
    }
    let _restore_lock = ArchiveCaptureLock::acquire(&root, &repository_id, kind)?;
    let store = open_archive_store(&root)?;
    let _ = sync_archive_provider(&root, &store);
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

    // Finish repository setup in staging before touching the current destination.
    // This keeps any Git/config/remotes failure from leaving a partially activated
    // restore and makes the final directory swap the last state-changing step.
    let mut initialized_git = false;
    if kind == ArchiveKind::Local {
        let current_git = destination.join(".git");
        let current_git_is_real_directory = fs::symlink_metadata(&current_git)
            .map(|metadata| metadata.file_type().is_dir())
            .unwrap_or(false);
        if non_empty && current_git_is_real_directory {
            copy_directory_tree(&current_git, &stage.join(".git"))
                .map_err(|error| format!("Could not preserve existing Git metadata in restore staging: {error}"))?;
        }
        if !stage.join(".git").exists() {
            Repository::init(&stage)
                .map_err(|error| format!("Could not initialize Git metadata in restore staging: {error}"))?;
            initialized_git = true;
        }
    } else {
        let git_dir = stage.join(".git");
        write_minimal_git_config(&git_dir.join("config"))
            .map_err(|error| format!("Could not initialize staged Git configuration: {error}"))?;
        let restored_repository = Repository::open(&stage)
            .map_err(|error| format!("Staged Git metadata could not be opened: {error}"))?;
        for remote in &manifest.remotes {
            let fetch_url = remote.fetch_url.as_deref().ok_or_else(|| {
                format!("Remote {} has no safe fetch URL in the archive manifest.", remote.name)
            })?;
            if restored_repository.find_remote(&remote.name).is_ok() {
                restored_repository.remote_set_url(&remote.name, fetch_url)
                    .map_err(|error| format!("Could not restore Git remote {} in staging: {error}", remote.name))?;
            } else {
                restored_repository.remote(&remote.name, fetch_url)
                    .map_err(|error| format!("Could not restore Git remote {} in staging: {error}", remote.name))?;
            }
            if let Some(push_url) = remote.push_url.as_deref() {
                restored_repository.remote_set_pushurl(&remote.name, Some(push_url))
                    .map_err(|error| format!("Could not restore Git push URL for {} in staging: {error}", remote.name))?;
            }
        }

        // The source index is deliberately excluded from Remote Archive. Rebuild
        // a clean index from the restored HEAD without checking out files, so
        // captured uncommitted/staged worktree contents remain visible as changes.
        let mut restored_index = restored_repository.index()
            .map_err(|error| format!("Could not create the restored Git index: {error}"))?;
        if let Ok(head) = restored_repository.head() {
            let head_commit = head.peel_to_commit()
                .map_err(|error| format!("Could not resolve restored HEAD for index reconstruction: {error}"))?;
            let head_tree = head_commit.tree()
                .map_err(|error| format!("Could not read restored HEAD tree for index reconstruction: {error}"))?;
            restored_index.read_tree(&head_tree)
                .map_err(|error| format!("Could not rebuild the restored Git index from HEAD: {error}"))?;
        }
        restored_index.write()
            .map_err(|error| format!("Could not write the reconstructed Git index: {error}"))?;
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
            let _ = fs::remove_dir_all(&stage);
            return Err(format!("Refusing to overwrite an existing safety copy: {}", backup.display()));
        }
        if let Err(error) = fs::rename(&destination, backup) {
            let _ = fs::remove_dir_all(&stage);
            return Err(format!("Could not preserve the current destination before restore: {error}"));
        }
    } else if existing {
        if let Err(error) = fs::remove_dir(&destination) {
            let _ = fs::remove_dir_all(&stage);
            return Err(format!("Could not remove the empty restore destination: {error}"));
        }
    }

    if let Err(error) = fs::rename(&stage, &destination) {
        if let Some(backup) = &backup_path {
            let _ = fs::rename(backup, &destination);
        }
        let _ = fs::remove_dir_all(&stage);
        return Err(format!("Could not activate restored repository: {error}"));
    }
    Ok(ArchiveRestoreResult {
        snapshot,
        destination: destination.to_string_lossy().to_string(),
        preserved_previous_path: backup_path.map(|path| path.to_string_lossy().to_string()),
        initialized_git_repository: initialized_git,
    })
}

fn collect_safe_remotes(repository: &Repository) -> Result<Vec<SnapshotRemote>, String> {
    let names = repository.remotes()
        .map_err(|error| format!("Could not enumerate Git remotes: {error}"))?;
    let mut result = Vec::new();
    for name in names.iter().flatten() {
        let remote = repository.find_remote(name)
            .map_err(|error| format!("Could not read Git remote {name}: {error}"))?;
        let fetch_url = remote.url()
            .map(sanitize_remote_url)
            .transpose()?;
        let push_url = remote.pushurl()
            .map(sanitize_remote_url)
            .transpose()?;
        if fetch_url.is_some() || push_url.is_some() {
            result.push(SnapshotRemote {
                name: name.to_string(),
                fetch_url,
                push_url,
            });
        }
    }
    result.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(result)
}

fn sanitize_remote_url(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err("Git remote URL is empty or contains control characters.".to_string());
    }
    // Handle SCP-style SSH remotes before generic URL parsing. A value such as
    // "alice:token@host:path" can otherwise be misread as the URI scheme "alice".
    if !value.contains("://") {
        if let Some((user, destination)) = value.split_once('@') {
            if destination.contains(':') && !destination.starts_with('/') {
                let safe_user = if user.is_empty() || user.contains(':') { "git" } else { user };
                return Ok(format!("{safe_user}@{destination}"));
            }
        }
    }

    if let Ok(mut url) = url::Url::parse(value) {
        // SSH usernames identify the remote account (commonly "git") and are
        // not passwords. Keep them, but always discard a URL password. For all
        // other schemes, remove user-info entirely.
        if url.scheme() != "ssh" {
            let _ = url.set_username("");
        }
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        return Ok(url.to_string());
    }
    if value.contains("://") {
        return Err("Git remote URL could not be safely sanitized; remove embedded credentials and retry.".to_string());
    }
    Ok(value.to_string())
}

fn remote_commit_tips(repository: &Repository) -> Result<Vec<String>, String> {
    let references = repository.references()
        .map_err(|error| format!("Could not enumerate Git refs for Remote Archive security check: {error}"))?;
    let mut tips = Vec::<String>::new();
    for reference in references {
        let reference = reference
            .map_err(|error| format!("Could not read a Git ref for Remote Archive security check: {error}"))?;
        let name = reference.name().unwrap_or("<non-UTF-8 ref>");
        let commit = reference.peel_to_commit().map_err(|error| {
            format!("Remote Archive cannot preserve ref {name} because it does not resolve to a commit or annotated tag: {error}")
        })?;
        tips.push(commit.id().to_string());
    }
    if let Ok(head) = repository.head() {
        let commit = head.peel_to_commit()
            .map_err(|error| format!("Remote Archive HEAD does not resolve to a commit: {error}"))?;
        tips.push(commit.id().to_string());
    }
    tips.sort();
    tips.dedup();
    Ok(tips)
}

/// Build a compact Git pack from objects reachable through refs only.
/// Source .git/objects is deliberately not copied, because it may also hold
/// staged blobs, dangling objects or content left behind by history rewrites.
fn write_reachable_git_pack(repository: &Repository, output_dir: &Path) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(output_dir)
        .map_err(|error| format!("Could not create temporary Git pack directory: {error}"))?;
    let mut builder = repository.packbuilder()
        .map_err(|error| format!("Could not initialize reachable Git pack builder: {error}"))?;
    builder.set_threads(1);

    let references = repository.references()
        .map_err(|error| format!("Could not enumerate refs for Remote Archive pack: {error}"))?;
    for reference in references {
        let reference = reference
            .map_err(|error| format!("Could not read ref for Remote Archive pack: {error}"))?;
        let name = reference.name().unwrap_or("<non-UTF-8 ref>");
        let resolved = reference.resolve()
            .map_err(|error| format!("Could not resolve ref {name} for Remote Archive pack: {error}"))?;
        let target = resolved.target()
            .ok_or_else(|| format!("Remote Archive cannot pack ref {name} because it has no object target."))?;
        resolved.peel_to_commit().map_err(|error| {
            format!("Remote Archive cannot pack ref {name} because it does not resolve to a commit or annotated tag: {error}")
        })?;
        builder.insert_recursive(target, None)
            .map_err(|error| format!("Could not add reachable objects for ref {name} to Remote Archive pack: {error}"))?;
    }

    // A detached HEAD is not necessarily named by any ref. Include it explicitly.
    if let Ok(head) = repository.head() {
        let resolved = head.resolve()
            .map_err(|error| format!("Could not resolve HEAD for Remote Archive pack: {error}"))?;
        if let Some(target) = resolved.target() {
            resolved.peel_to_commit()
                .map_err(|error| format!("Remote Archive HEAD does not resolve to a commit: {error}"))?;
            builder.insert_recursive(target, None)
                .map_err(|error| format!("Could not add detached HEAD objects to Remote Archive pack: {error}"))?;
        }
    }

    if builder.object_count() == 0 {
        return Ok(Vec::new());
    }
    builder.write(output_dir, 0o644)
        .map_err(|error| format!("Could not write reachable Git pack: {error}"))?;
    let pack_name = builder.name()
        .ok_or_else(|| "Git pack builder did not return its generated pack name.".to_string())?;
    if !matches!(pack_name.len(), 40 | 64)
        || !pack_name.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Git pack builder returned an invalid pack name.".to_string());
    }

    let files = vec![
        output_dir.join(format!("pack-{pack_name}.pack")),
        output_dir.join(format!("pack-{pack_name}.idx")),
    ];
    for path in &files {
        if !path.is_file() {
            return Err(format!("Git pack builder did not create expected file {}.", path.display()));
        }
    }
    Ok(files)
}

fn remote_refs_fingerprint(repository: &Repository) -> Result<String, String> {
    let mut entries = Vec::<String>::new();
    let references = repository.references()
        .map_err(|error| format!("Could not enumerate Git refs for Remote Archive security check: {error}"))?;
    for reference in references {
        let reference = reference
            .map_err(|error| format!("Could not read a Git ref for Remote Archive security check: {error}"))?;
        let name = reference.name().unwrap_or("<non-UTF-8 ref>");
        let resolved = reference.resolve()
            .map_err(|error| format!("Could not resolve Git ref {name} for Remote Archive security check: {error}"))?;
        let target = resolved.target()
            .map(|oid| oid.to_string())
            .unwrap_or_else(|| "<no direct target>".to_string());
        entries.push(format!("{name}={target}"));
    }
    if let Ok(head) = repository.head() {
        entries.push(format!(
            "HEAD:{}={}",
            head.name().unwrap_or("<detached>"),
            head.target().map(|oid| oid.to_string()).unwrap_or_else(|| "<unborn>".to_string())
        ));
    }
    entries.sort();
    Ok(sha256_bytes(entries.join("\n").as_bytes()))
}

/// Refuse to copy the raw Git object database when reachable history contains
/// paths which the normal archive scanner excludes. Otherwise a committed
/// .env/credential file would bypass the working-tree secret filter through
/// historical blobs inside .git/objects.
fn validate_remote_archive_history(
    repository: &Repository,
    archive_root: &Path,
    repository_id: &str,
) -> Result<(), String> {
    let fingerprint = remote_refs_fingerprint(repository)?;
    let current_tips = remote_commit_tips(repository)?;
    let cache_path = archive_root
        .join("repositories")
        .join(repository_id)
        .join("remote")
        .join("history-safety.json");
    let mut previous_cache = None::<RemoteHistorySafetyCache>;
    if let Ok(bytes) = fs::read(&cache_path) {
        if bytes.len() <= 1024 * 1024 {
            if let Ok(cache) = serde_json::from_slice::<RemoteHistorySafetyCache>(&bytes) {
                if cache.format_version == 2 {
                    if cache.refs_fingerprint == fingerprint {
                        return if cache.safe {
                            Ok(())
                        } else if let Some(path) = cache.unsafe_path.as_deref() {
                            Err(format!(
                                "Remote Archive is blocked: reachable Git history contains excluded secret path '{path}'. Remove it from every reachable commit (rewrite history) before retrying. No archive was created."
                            ))
                        } else {
                            Err("Remote Archive is blocked because this repository's reachable Git history contains an excluded secret path. Remove it from every reachable commit (rewrite history) before retrying.".to_string())
                        };
                    }
                    previous_cache = Some(cache);
                }
            }
        }
    }

    let mut walker = repository.revwalk()
        .map_err(|error| format!("Could not inspect Git history for Remote Archive: {error}"))?;
    let old_tips = previous_cache.as_ref()
        .filter(|cache| cache.safe)
        .map(|cache| cache.known_commit_tips.as_slice())
        .unwrap_or(&[]);
    for tip in &current_tips {
        if !old_tips.iter().any(|known| known == tip) {
            let oid = Oid::from_str(tip)
                .map_err(|error| format!("Cached Git commit id is invalid: {error}"))?;
            walker.push(oid)
                .map_err(|error| format!("Could not walk Git history for Remote Archive: {error}"))?;
        }
    }
    // Previously verified ref tips are safe traversal boundaries. Hiding each
    // one lets Git walk only new commits added since the last successful scan.
    // After a failed scan, do not use these boundaries: a formerly unsafe
    // commit may still be reachable through a different ref.
    for tip in old_tips {
        if let Ok(oid) = Oid::from_str(tip) {
            if repository.find_commit(oid).is_ok() {
                walker.hide(oid)
                    .map_err(|error| format!("Could not bound the incremental Git history scan: {error}"))?;
            }
        }
    }
    walker.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .map_err(|error| format!("Could not sort Git history for Remote Archive: {error}"))?;

    let mut seen_trees = HashSet::new();
    let mut unsafe_path = None::<String>;
    for oid in walker {
        let oid = oid.map_err(|error| format!("Could not read a Git history object for Remote Archive: {error}"))?;
        let commit = repository.find_commit(oid)
            .map_err(|error| format!("Could not read Git commit {oid} for Remote Archive: {error}"))?;
        let tree = commit.tree()
            .map_err(|error| format!("Could not read tree for Git commit {oid}: {error}"))?;
        if !seen_trees.insert(tree.id()) {
            continue;
        }
        tree.walk(TreeWalkMode::PreOrder, |root, entry| {
            if unsafe_path.is_some() {
                return TreeWalkResult::Skip;
            }
            let relative = format!("{root}{}", entry.name().unwrap_or(""))
                .replace('\\', "/");
            if is_default_secret_path(Path::new(&relative)) {
                unsafe_path = Some(relative);
                // libgit2 reports TreeWalkResult::Abort as a traversal error.
                // Skip this entry's subtree instead, then stop after walk()
                // returns normally so the concrete offending path is retained.
                TreeWalkResult::Skip
            } else {
                TreeWalkResult::Ok
            }
        }).map_err(|error| format!("Could not inspect a Git history tree for Remote Archive: {error}"))?;
        if unsafe_path.is_some() {
            break;
        }
    }

    let current_fingerprint = remote_refs_fingerprint(repository)?;
    if current_fingerprint != fingerprint {
        return Err("Git refs changed while the Remote Archive security scan was running. Retry the archive.".to_string());
    }
    let safe = unsafe_path.is_none();
    let mut known_commit_tips = if safe {
        previous_cache
            .filter(|cache| cache.safe)
            .map(|cache| cache.known_commit_tips)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    known_commit_tips.extend(current_tips);
    known_commit_tips.sort();
    known_commit_tips.dedup();
    // Avoid an unbounded per-repository cache if a repo accumulates many ref tips.
    if known_commit_tips.len() > 4096 {
        known_commit_tips = known_commit_tips.into_iter().rev().take(2048).collect();
        known_commit_tips.sort();
    }
    let cache = RemoteHistorySafetyCache {
        format_version: 2,
        refs_fingerprint: fingerprint,
        known_commit_tips,
        unsafe_path: unsafe_path.clone(),
        safe,
    };
    let bytes = serde_json::to_vec(&cache)
        .map_err(|error| format!("Could not serialize Remote Archive security cache: {error}"))?;
    AtomicIo::new().write_atomic(&cache_path, &bytes)
        .map_err(|error| format!("Could not save Remote Archive security cache: {error}"))?;

    if let Some(path) = unsafe_path {
        return Err(format!(
            "Remote Archive is blocked: reachable Git history contains excluded secret path '{path}'. Remove it from every reachable commit (rewrite history) before retrying. No archive was created."
        ));
    }
    Ok(())
}

fn validate_remote_archive_source(source: &Path) -> Result<(), String> {
    let marker = source.join(".git");
    if marker.is_file() {
        return Err("Remote Archive requires a self-contained .git directory; linked worktrees keep Git metadata outside the worktree.".to_string());
    }
    if !marker.is_dir() {
        return Err("Remote Archive requires a repository-local .git directory.".to_string());
    }
    let shallow_file = marker.join("shallow");
    if shallow_file.is_file() {
        let bytes = fs::read(&shallow_file)
            .map_err(|error| format!("Could not inspect Git shallow metadata: {error}"))?;
        if !bytes.is_empty() {
            return Err("Remote Archive requires complete Git history; this repository is shallow. Fetch or unshallow it before archiving.".to_string());
        }
    }
    for alternate in [
        marker.join("objects").join("info").join("alternates"),
        marker.join("objects").join("info").join("http-alternates"),
    ] {
        if alternate.is_file() {
            let bytes = fs::read(&alternate)
                .map_err(|error| format!("Could not inspect Git object alternates: {error}"))?;
            if !bytes.is_empty() {
                return Err(format!(
                    "Remote Archive cannot be self-contained because {} references external Git objects. Remove the alternates dependency or repack the repository first.",
                    alternate.strip_prefix(source).unwrap_or(&alternate).display()
                ));
            }
        }
    }

    // A nested .git marker is normally a submodule/worktree pointer. Its
    // repository metadata lives outside this parent repository's .git folder,
    // so copying only the parent object database would not be self-contained.
    let root_git = marker;
    let walker = WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.path() != root_git);
    for entry in walker {
        let entry = entry.map_err(|error| {
            format!("Could not inspect nested Git metadata for Remote Archive: {error}")
        })?;
        if entry.file_name() == std::ffi::OsStr::new(".git") {
            return Err(format!(
                "Remote Archive does not support nested Git repositories or submodules yet: {}",
                entry.path().strip_prefix(source).unwrap_or(entry.path()).display()
            ));
        }
    }
    Ok(())
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
            let source_git_objects = kind == ArchiveKind::Remote && is_git_objects_directory(relative);
            let skip = file_type.is_symlink()
                || (kind == ArchiveKind::Local && starts_with_git_directory(relative))
                || source_git_objects
                || is_default_secret_path(relative)
                || (kind == ArchiveKind::Remote && is_git_sensitive_path(relative));
            if skip && !source_git_objects {
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

fn is_allowed_git_objects_directory(relative: &Path) -> bool {
    let normalized = relative.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    matches!(normalized.as_str(), ".git/objects" | ".git/objects/pack")
}

fn is_git_objects_directory(relative: &Path) -> bool {
    let components = relative.components()
        .filter_map(|part| part.as_os_str().to_str())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    components.len() == 2 && components[0] == ".git" && components[1] == "objects"
}

fn is_reachable_pack_file_name(name: &str) -> bool {
    let Some(name) = name.strip_prefix("pack-") else { return false; };
    let hash = name.strip_suffix(".pack")
        .or_else(|| name.strip_suffix(".idx"));
    hash.is_some_and(|hash| {
        (hash.len() == 40 || hash.len() == 64)
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
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

    if remaining[0] == "objects" {
        return !(remaining.len() == 3
            && remaining[1] == "pack"
            && is_reachable_pack_file_name(&remaining[2]));
    }

    matches!(
        remaining[0].as_str(),
        "config" | "config.lock" | "config.worktree" | "config.worktree.lock" |
        "index" | "index.lock" | "hooks" | "logs" | "rr-cache" | "modules" | "worktrees" |
        "shallow" | "orig_head" | "fetch_head" | "merge_head" | "cherry_pick_head" |
        "rebase_head" | "bisect_log" | "squash_msg" | "merge_msg" | "commit_editmsg" |
        "auto_merge" | "merge_rr" | "sequencer" | "rebase-apply" | "rebase-merge"
    )
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

fn default_archive_cloud_settings(root: &Path) -> Result<CloudNodeSettings, String> {
    let settings: CloudNodeSettings = serde_json::from_value(serde_json::json!({
        "formatVersion": 1,
        "node": {
            "id": "gitmanager-archives",
            "mode": "archive",
            "storageRoot": root.join(".cloud-node"),
            "backupVersions": MAX_RESTORE_POINTS,
            "preserveOriginal": true,
            "videoChunkBytes": 4194304
        }
    })).map_err(|error| format!("Could not initialize archive storage settings: {error}"))?;
    settings.validate().map_err(|error| error.to_string())?;
    Ok(settings)
}

fn load_archive_cloud_settings(root: &Path) -> Result<CloudNodeSettings, String> {
    let path = root.join(CLOUD_NODE_SETTINGS_FILE);
    if path.is_file() {
        let settings = CloudNodeSettings::load(&path)
            .map_err(|error| format!("Could not load cloud archive settings: {error}"))?;
        let expected = root.join(".cloud-node");
        if settings.node.storage_root != expected {
            return Err("Cloud archive storageRoot must remain inside GitManager's archive directory.".to_string());
        }
        Ok(settings)
    } else {
        default_archive_cloud_settings(root)
    }
}

fn open_archive_store(root: &Path) -> Result<CloudNodeStore, String> {
    let settings = load_archive_cloud_settings(root)?;
    CloudNodeStore::open(&settings)
        .map_err(|error| format!("Could not open the Cloud Node archive store: {error}"))
}

fn sync_archive_provider(
    root: &Path,
    store: &CloudNodeStore,
) -> Result<Option<ProviderSyncResult>, String> {
    let settings = load_archive_cloud_settings(root)?;
    if settings.provider.is_none() {
        return Ok(None);
    }
    let _sync_lock = ArchiveCaptureLock::acquire(root, "cloud-provider-sync", ArchiveKind::Local)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Could not start cloud archive runtime: {error}"))?;
    runtime.block_on(synchronize_provider(&settings, store))
        .map(Some)
        .map_err(|error| format!("Cloud archive provider synchronization failed: {error}"))
}

pub fn configure_archive_cloud_storage(
    archive_root: Option<&str>,
    provider_json: &str,
) -> Result<ArchiveCloudStorageInfo, String> {
    let root = resolve_archive_root(archive_root)?;
    let _cloud_settings_lock = ArchiveCaptureLock::acquire(
        &root,
        "cloud-provider-sync",
        ArchiveKind::Local,
    )?;
    let provider: serde_json::Value = serde_json::from_str(provider_json)
        .map_err(|error| format!("Cloud provider configuration is invalid JSON: {error}"))?;
    if !provider.is_object() {
        return Err("Cloud provider configuration must be a JSON object.".to_string());
    }
    let settings_json = serde_json::json!({
        "formatVersion": 1,
        "node": {
            "id": "gitmanager-archives",
            "mode": "archive",
            "storageRoot": root.join(".cloud-node"),
            "backupVersions": MAX_RESTORE_POINTS,
            "preserveOriginal": true,
            "videoChunkBytes": 4194304
        },
        "provider": provider
    });
    let settings: CloudNodeSettings = serde_json::from_value(settings_json.clone())
        .map_err(|error| format!("Cloud provider configuration is not supported: {error}"))?;
    settings.validate().map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(&settings_json)
        .map_err(|error| format!("Could not serialize cloud archive settings: {error}"))?;
    AtomicIo::new().write_atomic(&root.join(CLOUD_NODE_SETTINGS_FILE), &bytes)
        .map_err(|error| format!("Could not publish cloud archive settings: {error}"))?;
    archive_cloud_storage_info(&root)
}

pub fn disable_archive_cloud_storage(
    archive_root: Option<&str>,
) -> Result<ArchiveCloudStorageInfo, String> {
    let root = resolve_archive_root(archive_root)?;
    let _cloud_settings_lock = ArchiveCaptureLock::acquire(
        &root,
        "cloud-provider-sync",
        ArchiveKind::Local,
    )?;
    let path = root.join(CLOUD_NODE_SETTINGS_FILE);
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("Could not disable cloud archive storage: {error}"))?;
    }
    archive_cloud_storage_info(&root)
}

pub fn archive_cloud_storage_info(root: &Path) -> Result<ArchiveCloudStorageInfo, String> {
    let root = resolve_archive_root(Some(&root.to_string_lossy()))?;
    let settings = load_archive_cloud_settings(&root)?;
    let Some(provider) = settings.provider.as_ref() else {
        return Ok(ArchiveCloudStorageInfo {
            configured: false,
            provider: None,
            namespace: None,
            bucket: None,
            target: None,
            auth_environment_variables: Vec::new(),
            note: "Archives are local-only. A Google Drive/OneDrive desktop-synced folder can be selected as a local archive destination.".to_string(),
        });
    };
    let provider_name = match provider.kind {
        ProviderKind::AmazonS3 => "amazon-s3",
        ProviderKind::Supabase => "supabase",
        ProviderKind::AzureBlob => "azure-blob",
        ProviderKind::GoogleCloudStorage => "google-cloud-storage",
        ProviderKind::Http => "http",
    }.to_string();
    let auth = &provider.auth;
    let mut env_names = Vec::<String>::new();
    let mut push_env = |value: Option<&str>, fallback: &str| {
        let name = value.unwrap_or(fallback);
        if !env_names.iter().any(|existing| existing == name) {
            env_names.push(name.to_string());
        }
    };
    match provider.kind {
        ProviderKind::AmazonS3 => {
            push_env(
                auth.access_key_env.as_deref().or(provider.access_key_env.as_deref()),
                "RBE_CN_PROV_AMAZON_ACCESS_KEY",
            );
            push_env(
                auth.secret_key_env.as_deref().or(provider.secret_key_env.as_deref()),
                "RBE_CN_PROV_AMAZON_SECRET_KEY",
            );
            push_env(
                auth.session_token_env.as_deref().or(provider.session_token_env.as_deref()),
                "RBE_CN_PROV_AMAZON_SESSION_TOKEN",
            );
        }
        ProviderKind::Supabase => match auth.mode {
            ProviderAuthMode::Bearer => push_env(
                auth.bearer_token_env.as_deref().or(provider.credential_env.as_deref()),
                "RBE_CN_PROV_SUPABASE_API_KEY",
            ),
            ProviderAuthMode::Header => push_env(
                auth.header_value_env.as_deref().or(provider.credential_env.as_deref()),
                "RBE_CN_PROV_HTTP_HEADER_VALUE",
            ),
            _ => push_env(
                auth.api_key_env.as_deref().or(provider.credential_env.as_deref()),
                "RBE_CN_PROV_SUPABASE_API_KEY",
            ),
        },
        ProviderKind::AzureBlob => push_env(
            auth.sas_token_env.as_deref().or(provider.credential_env.as_deref()),
            "RBE_CN_PROV_AZURE_SAS_TOKEN",
        ),
        ProviderKind::GoogleCloudStorage => push_env(
            auth.oauth_token_env
                .as_deref()
                .or(auth.bearer_token_env.as_deref())
                .or(provider.credential_env.as_deref()),
            "RBE_CN_PROV_GOOGLE_OAUTH_TOKEN",
        ),
        ProviderKind::Http => match auth.mode {
            ProviderAuthMode::None => {}
            ProviderAuthMode::Auto => push_env(
                auth.bearer_token_env.as_deref().or(provider.credential_env.as_deref()),
                "RBE_CN_PROV_HTTP_BEARER_TOKEN",
            ),
            ProviderAuthMode::ApiKey => push_env(
                auth.api_key_env.as_deref(),
                "RBE_CN_PROV_HTTP_API_KEY",
            ),
            ProviderAuthMode::Bearer | ProviderAuthMode::OAuthBearer => push_env(
                auth.bearer_token_env.as_deref().or(auth.oauth_token_env.as_deref()),
                "RBE_CN_PROV_HTTP_BEARER_TOKEN",
            ),
            ProviderAuthMode::Basic => {
                push_env(auth.username_env.as_deref(), "RBE_CN_PROV_HTTP_USERNAME");
                push_env(auth.password_env.as_deref(), "RBE_CN_PROV_HTTP_PASSWORD");
            }
            ProviderAuthMode::Header => push_env(
                auth.header_value_env.as_deref().or(provider.credential_env.as_deref()),
                "RBE_CN_PROV_HTTP_HEADER_VALUE",
            ),
            ProviderAuthMode::AwsSigV4 => {
                push_env(auth.access_key_env.as_deref(), "RBE_CN_PROV_AMAZON_ACCESS_KEY");
                push_env(auth.secret_key_env.as_deref(), "RBE_CN_PROV_AMAZON_SECRET_KEY");
            }
            ProviderAuthMode::AzureSas => push_env(
                auth.sas_token_env.as_deref().or(provider.credential_env.as_deref()),
                "RBE_CN_PROV_AZURE_SAS_TOKEN",
            ),
        },
    }
    drop(push_env);
    let target = ProviderClient::new(provider)
        .map(|client| client.target_description())
        .map_err(|error| error.to_string())?;
    Ok(ArchiveCloudStorageInfo {
        configured: true,
        provider: Some(provider_name),
        namespace: Some(provider.namespace.clone()),
        bucket: Some(provider.bucket.clone()),
        target: Some(target),
        auth_environment_variables: env_names,
        note: "Only environment-variable names are saved in GitManager settings. Secret values remain in your OS environment and are not copied into archives.".to_string(),
    })
}
pub fn sync_archive_cloud_storage(archive_root: Option<&str>) -> Result<String, String> {
    let root = resolve_archive_root(archive_root)?;
    let store = open_archive_store(&root)?;
    match sync_archive_provider(&root, &store)? {
        Some(result) => Ok(format!("{:?}", result.action)),
        None => Ok("local-only".to_string()),
    }
}

fn read_index(path: &Path, store: &CloudNodeStore, logical_path: &str) -> Result<ArchiveIndex, String> {
    let cloud_bytes = store.read_logical_file(logical_path, MAX_INDEX_BYTES)
        .map_err(|error| format!("Could not recover archive index from Cloud Node: {error}"))?;
    let bytes = if let Some(bytes) = cloud_bytes {
        bytes
    } else if path.is_file() {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("Could not inspect archive index: {error}"))?;
        if metadata.len() > MAX_INDEX_BYTES {
            return Err("Archive index exceeds the 2 MiB safety limit.".to_string());
        }
        fs::read(path).map_err(|error| format!("Could not read archive index: {error}"))?
    } else {
        return Ok(ArchiveIndex { format_version: ARCHIVE_FORMAT_VERSION, snapshots: Vec::new() });
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
    let mut remote_names = HashSet::new();
    for remote in &manifest.remotes {
        if remote.name.is_empty()
            || remote.name.len() > 128
            || remote.name.chars().any(char::is_control)
            || remote.name.contains('/')
            || remote.name.contains('\\')
            || !remote_names.insert(remote.name.as_str())
        {
            return Err("Restore-point manifest contains an invalid or duplicate remote name.".to_string());
        }
        for url in [remote.fetch_url.as_deref(), remote.push_url.as_deref()].into_iter().flatten() {
            if sanitize_remote_url(url)? != url {
                return Err(format!("Restore-point manifest contains an unsanitized credential-bearing remote URL for {}.", remote.name));
            }
        }
    }
    if snapshot.kind == ArchiveKind::Local && !manifest.remotes.is_empty() {
        return Err("Local Archive manifest must not contain Git remote configuration.".to_string());
    }
    for directory in &manifest.directories {
        let path = validate_relative_path(directory)?;
        if snapshot.kind == ArchiveKind::Local && starts_with_git_directory(path) {
            return Err("Local Archive manifest unexpectedly contains .git.".to_string());
        }
        let allowed_pack_container = snapshot.kind == ArchiveKind::Remote
            && is_allowed_git_objects_directory(path);
        if is_default_secret_path(path)
            || (snapshot.kind == ArchiveKind::Remote
                && is_git_sensitive_path(path)
                && !allowed_pack_container)
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
    let normalized = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let normalized = normalized.to_ascii_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    hex::encode(&digest[..16])
}

/// Cloud archive identity follows the repository's primary Git remote so a
/// clone in another directory or on another machine sees the same restore
/// points. Repositories without a remote remain scoped to their local path.
fn repository_archive_id(repository: &Repository, path: &Path) -> Result<String, String> {
    let remotes = collect_safe_remotes(repository)?;
    let primary = remotes.iter()
        .find(|remote| remote.name.eq_ignore_ascii_case("origin"))
        .or_else(|| remotes.first());
    if let Some(url) = primary.and_then(|remote| {
        remote.fetch_url.as_deref().or(remote.push_url.as_deref())
    }) {
        let identity = format!("gitmanager-remote-v1\n{}", url.trim());
        let digest = Sha256::digest(identity.as_bytes());
        return Ok(hex::encode(&digest[..16]));
    }
    Ok(repository_id(path))
}

fn canonicalize_future_path(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path);
    }
    let mut missing = Vec::<std::ffi::OsString>::new();
    let mut ancestor = path;
    while !ancestor.exists() {
        let name = ancestor.file_name().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "restore path has no existing ancestor")
        })?;
        missing.push(name.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "restore path has no existing ancestor")
        })?;
    }
    let mut resolved = fs::canonicalize(ancestor)?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
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
    fn repository_archive_identity_is_stable_across_clone_paths() {
        let root = temp_root("stable-repository-id");
        let first_path = root.join("clone-a");
        let second_path = root.join("clone-b");
        fs::create_dir_all(&first_path).unwrap();
        fs::create_dir_all(&second_path).unwrap();
        let first = Repository::init(&first_path).unwrap();
        let second = Repository::init(&second_path).unwrap();
        first.remote("origin", "https://github.com/acme/project.git").unwrap();
        second.remote("origin", "https://github.com/acme/project.git").unwrap();
        assert_eq!(
            repository_archive_id(&first, &first_path).unwrap(),
            repository_archive_id(&second, &second_path).unwrap()
        );
        second.remote("backup", "https://gitlab.com/acme/project.git").unwrap();
        assert_eq!(
            repository_archive_id(&first, &first_path).unwrap(),
            repository_archive_id(&second, &second_path).unwrap(),
            "origin is the stable identity even when a second remote is configured"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repositories_without_remotes_keep_a_path_scoped_identity() {
        let root = temp_root("path-repository-id");
        let first_path = root.join("project-a");
        let second_path = root.join("project-b");
        fs::create_dir_all(&first_path).unwrap();
        fs::create_dir_all(&second_path).unwrap();
        let first = Repository::init(&first_path).unwrap();
        let second = Repository::init(&second_path).unwrap();
        assert_ne!(
            repository_archive_id(&first, &first_path).unwrap(),
            repository_archive_id(&second, &second_path).unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_archive_omits_reflogs_rerere_and_nested_git_module_metadata() {
        assert!(is_git_sensitive_path(Path::new(".git/logs/HEAD")));
        assert!(is_git_sensitive_path(Path::new(".git/rr-cache/abcd/postimage")));
        assert!(is_git_sensitive_path(Path::new(".git/modules/lib/config")));
        assert!(is_git_sensitive_path(Path::new(".git/worktrees/other/HEAD")));
    }

    #[test]
    fn remote_archive_pack_contains_only_objects_reachable_from_refs() {
        let root = temp_root("reachable-git-pack");
        let repository_path = root.join("source");
        fs::create_dir_all(&repository_path).unwrap();
        let repository = Repository::init(&repository_path).unwrap();
        commit_file(&repository, "README.md", "reachable commit");
        let head_oid = repository.head().unwrap().target().unwrap();

        let signature = git2::Signature::now("GitManager Test", "gitmanager-test@example.invalid").unwrap();
        let commit_object = repository.find_object(head_oid, None).unwrap();
        let tag_oid = repository.tag("v1", &commit_object, &signature, "release tag", false).unwrap();

        // A staged secret is written to the object database and the index, but is
        // not reachable from any commit/ref. A separate dangling blob is also left.
        fs::write(repository_path.join(".env"), "STAGED_SECRET=must-not-pack").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new(".env")).unwrap();
        index.write().unwrap();
        let staged_secret_oid = index.get_path(Path::new(".env"), 0).unwrap().id;
        let dangling_secret_oid = repository.blob(b"DANGLING_SECRET=must-not-pack").unwrap();

        let generated_pack_dir = root.join("generated-pack");
        let generated = write_reachable_git_pack(&repository, &generated_pack_dir).unwrap();
        assert_eq!(generated.len(), 2);
        assert!(generated.iter().any(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pack")));
        assert!(generated.iter().any(|path| path.extension().and_then(|ext| ext.to_str()) == Some("idx")));

        let mirror_path = root.join("mirror.git");
        let mirror = Repository::init_bare(&mirror_path).unwrap();
        let mirror_pack_dir = mirror.path().join("objects").join("pack");
        fs::create_dir_all(&mirror_pack_dir).unwrap();
        for file in &generated {
            fs::copy(file, mirror_pack_dir.join(file.file_name().unwrap())).unwrap();
        }
        let odb = mirror.odb().unwrap();
        odb.refresh().unwrap();

        assert!(mirror.find_commit(head_oid).is_ok());
        assert!(mirror.find_object(tag_oid, None).is_ok());
        assert!(mirror.odb().unwrap().exists(head_oid));
        assert!(!odb.exists(staged_secret_oid));
        assert!(!odb.exists(dangling_secret_oid));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_git_metadata_filter_only_allows_generated_pack_and_index_files() {
        assert!(is_git_sensitive_path(Path::new(".git/index")));
        assert!(is_allowed_git_objects_directory(Path::new(".git/objects")));
        assert!(is_allowed_git_objects_directory(Path::new(".git/objects/pack")));
        assert!(!is_allowed_git_objects_directory(Path::new(".git/objects/info")));
        assert!(is_git_sensitive_path(Path::new(".git/objects/loose-object")));
        assert!(is_git_sensitive_path(Path::new(".git/objects/pack/multi-pack-index")));
        assert!(is_git_sensitive_path(Path::new(".git/objects/info/alternates")));
        assert!(!is_git_sensitive_path(Path::new(".git/objects/pack/pack-0123456789abcdef0123456789abcdef01234567.pack")));
        assert!(!is_git_sensitive_path(Path::new(".git/objects/pack/pack-0123456789abcdef0123456789abcdef01234567.idx")));
    }

    #[test]
    fn remote_archive_refuses_secret_paths_in_reachable_git_history_and_caches_scan() {
        let root = temp_root("secret-history");
        let repository_path = root.join("repository");
        let archive_root = root.join("archives");
        fs::create_dir_all(&repository_path).unwrap();
        let repo = Repository::init(&repository_path).unwrap();
        fs::create_dir_all(repository_path.join("config")).unwrap();
        commit_file(&repo, ".env.example", "SAFE_TEMPLATE=1");
        assert!(validate_remote_archive_history(&repo, &archive_root, "secret-history-id").is_ok());
        let cache_path = archive_root.join("repositories/secret-history-id/remote/history-safety.json");
        assert!(cache_path.is_file());

        commit_file(&repo, "config/.env.production", "SECRET=never-copy");
        let error = validate_remote_archive_history(&repo, &archive_root, "secret-history-id").unwrap_err();
        assert!(error.contains("config/.env.production"));
        assert!(!error.contains("never-copy"));
        let cached_error = validate_remote_archive_history(
            &repo,
            &archive_root,
            "secret-history-id",
        ).unwrap_err();
        assert!(cached_error.contains("config/.env.production"));
        assert!(!archive_root.join(".cloud-node").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_archive_restores_history_and_only_sanitized_remote_urls() {
        let root = temp_root("remote");
        let repository_path = root.join("repository");
        let archive_root = root.join("archives");
        fs::create_dir_all(&repository_path).unwrap();
        let repo = Repository::init(&repository_path).unwrap();
        commit_file(&repo, "README.md", "first commit");
        // Preserve staged working-copy bytes separately from the reachable Git
        // pack; restore should reconstruct the HEAD index and show this as an
        // ordinary unstaged worktree modification.
        fs::write(repository_path.join("README.md"), "staged working copy").unwrap();
        let mut source_index = repo.index().unwrap();
        source_index.add_path(Path::new("README.md")).unwrap();
        source_index.write().unwrap();
        repo.remote(
            "origin",
            "https://alice:fetch-secret@github.com/acme/project.git?access_token=query-secret#private",
        ).unwrap();
        repo.config().unwrap().set_str(
            "remote.origin.pushurl",
            "https://bob:push-secret@github.com/acme/project.git?token=push-query-secret",
        ).unwrap();

        // Remote Archive must preserve all named local refs, not just the
        // current HEAD, because the pack contains objects reachable from refs.
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        let head_oid = head_commit.id();
        repo.branch("archive-feature", &head_commit, false).unwrap();
        let tag_target = head_commit.as_object();
        repo.tag_lightweight("v1.0", &tag_target, false).unwrap();

        let snapshot = capture_repository(
            repository_path.to_str().unwrap(),
            Some(archive_root.to_str().unwrap()),
            "remote",
        ).unwrap();
        let store = open_archive_store(&archive_root).unwrap();
        let manifest = read_manifest(&store, &snapshot).unwrap();
        assert_eq!(
            manifest.remotes[0].fetch_url.as_deref(),
            Some("https://github.com/acme/project.git")
        );
        assert_eq!(
            manifest.remotes[0].push_url.as_deref(),
            Some("https://github.com/acme/project.git")
        );
        let serialized = serde_json::to_string(&manifest).unwrap();
        for secret in ["fetch-secret", "push-secret", "query-secret", "private"] {
            assert!(!serialized.contains(secret));
        }

        let destination = root.join("remote-restored");
        restore_repository_archive(
            repository_path.to_str().unwrap(),
            destination.to_str().unwrap(),
            Some(archive_root.to_str().unwrap()),
            "remote",
            &snapshot.id,
            false,
        ).unwrap();
        let restored = Repository::open(&destination).unwrap();
        assert!(restored.head().is_ok());
        assert_eq!(
            restored.find_reference("refs/heads/archive-feature").unwrap().target(),
            Some(head_oid),
            "Remote Archive must restore non-current local branch refs"
        );
        assert_eq!(
            restored.find_reference("refs/tags/v1.0").unwrap().target(),
            Some(head_oid),
            "Remote Archive must restore tag refs"
        );
        assert!(destination.join(".git").join("index").is_file(), "Remote Archive must reconstruct an index from HEAD");
        let mut status_options = git2::StatusOptions::new();
        status_options.include_untracked(true).recurse_untracked_dirs(true);
        let restored_status = restored.statuses(Some(&mut status_options)).unwrap();
        assert_eq!(restored_status.len(), 1);
        assert_eq!(restored_status.get(0).unwrap().path(), Some("README.md"));
        assert!(
            restored_status.get(0).unwrap().status().contains(git2::Status::WT_MODIFIED),
            "staged source content must return as a normal worktree modification"
        );
        let restored_pack_dir = destination.join(".git").join("objects").join("pack");
        let restored_pack_files = fs::read_dir(&restored_pack_dir).unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .count();
        assert_eq!(restored_pack_files, 2, "Remote Archive should restore one generated pack and its index");
        let restored_remote = restored.find_remote("origin").unwrap();
        assert_eq!(restored_remote.url(), Some("https://github.com/acme/project.git"));
        assert_eq!(restored_remote.pushurl(), Some("https://github.com/acme/project.git"));
        let config = fs::read_to_string(destination.join(".git").join("config")).unwrap();
        for secret in ["fetch-secret", "push-secret", "query-secret"] {
            assert!(!config.contains(secret));
        }
        assert_eq!(fs::read_to_string(destination.join("README.md")).unwrap(), "staged working copy");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_archive_rejects_external_git_object_alternates() {
        let root = temp_root("alternates");
        let repository_path = root.join("repository");
        fs::create_dir_all(&repository_path).unwrap();
        let _repo = Repository::init(&repository_path).unwrap();
        let alternates = repository_path.join(".git/objects/info/alternates");
        fs::write(&alternates, root.join("external-objects").to_string_lossy().as_bytes()).unwrap();
        assert!(validate_remote_archive_source(&repository_path).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_cloud_settings_mutations_share_the_provider_sync_lock() {
        let root = temp_root("cloud-settings-lock");
        let sync = ArchiveCaptureLock::acquire(
            &root,
            "cloud-provider-sync",
            ArchiveKind::Local,
        ).unwrap();
        assert!(ArchiveCaptureLock::acquire(
            &root,
            "cloud-provider-sync",
            ArchiveKind::Local,
        ).is_err());
        drop(sync);
        let mutation = ArchiveCaptureLock::acquire(
            &root,
            "cloud-provider-sync",
            ArchiveKind::Local,
        ).unwrap();
        drop(mutation);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_cloud_provider_config_stores_environment_names_not_secret_values() {
        let root = temp_root("provider-config");
        let provider = r#"{
            "kind": "http",
            "namespace": "gitmanager-test",
            "bucket": "archive-store",
            "endpoint": "https://example.invalid",
            "auth": {
                "mode": "bearer",
                "bearerTokenEnv": "GITMANAGER_TEST_TOKEN"
            }
        }"#;
        let info = configure_archive_cloud_storage(
            Some(root.to_str().unwrap()),
            provider,
        ).unwrap();
        assert!(info.configured);
        assert_eq!(info.provider.as_deref(), Some("http"));
        assert!(info.auth_environment_variables.iter().any(|name| name == "GITMANAGER_TEST_TOKEN"));
        let settings = fs::read_to_string(root.join(CLOUD_NODE_SETTINGS_FILE)).unwrap();
        assert!(settings.contains("GITMANAGER_TEST_TOKEN"));
        assert!(!settings.contains("secret-value"));
        assert!(!settings.contains("bearer-secret"));
        let disabled = disable_archive_cloud_storage(Some(root.to_str().unwrap())).unwrap();
        assert!(!disabled.configured);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_cloud_info_reports_provider_default_environment_names() {
        let root = temp_root("provider-defaults");
        let provider = r#"{
            "kind": "amazon-s3",
            "namespace": "gitmanager-test",
            "bucket": "archive-store",
            "region": "us-east-1",
            "auth": {
                "mode": "aws-sig-v4",
                "accessKeyEnv": "GITMANAGER_CUSTOM_ACCESS"
            }
        }"#;
        configure_archive_cloud_storage(Some(root.to_str().unwrap()), provider).unwrap();
        let info = archive_cloud_storage_info(&root).unwrap();
        assert!(info.auth_environment_variables.iter().any(|name| name == "GITMANAGER_CUSTOM_ACCESS"));
        assert!(info.auth_environment_variables.iter().any(|name| name == "RBE_CN_PROV_AMAZON_SECRET_KEY"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_archive_rejects_nested_git_metadata() {
        let root = temp_root("nested-git");
        let repository_path = root.join("repository");
        fs::create_dir_all(&repository_path).unwrap();
        let _repo = Repository::init(&repository_path).unwrap();
        let nested = repository_path.join("vendor/submodule");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join(".git"), "gitdir: ../../.git/modules/submodule").unwrap();
        assert!(validate_remote_archive_source(&repository_path).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restore_refuses_archive_root_and_ancestor_destinations() {
        let root = temp_root("restore-overlap");
        let repository_path = root.join("repository");
        let archive_root = root.join("archives");
        fs::create_dir_all(&repository_path).unwrap();
        let repo = Repository::init(&repository_path).unwrap();
        commit_file(&repo, "README.md", "safe");
        let snapshot = capture_repository(
            repository_path.to_str().unwrap(),
            Some(archive_root.to_str().unwrap()),
            "local",
        ).unwrap();
        assert!(restore_repository_archive(
            repository_path.to_str().unwrap(),
            archive_root.to_str().unwrap(),
            Some(archive_root.to_str().unwrap()),
            "local",
            &snapshot.id,
            true,
        ).is_err());
        assert!(restore_repository_archive(
            repository_path.to_str().unwrap(),
            root.to_str().unwrap(),
            Some(archive_root.to_str().unwrap()),
            "local",
            &snapshot.id,
            true,
        ).is_err());
        assert!(archive_root.join(".cloud-node").is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_capture_lock_serializes_one_repository_across_archive_types() {
        let root = temp_root("lock");
        let first = ArchiveCaptureLock::acquire(&root, "repository-id", ArchiveKind::Local).unwrap();
        assert!(ArchiveCaptureLock::acquire(&root, "repository-id", ArchiveKind::Remote).is_err());
        drop(first);
        let second = ArchiveCaptureLock::acquire(&root, "repository-id", ArchiveKind::Remote).unwrap();
        drop(second);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_url_sanitizer_handles_https_credentials_and_scp_style_urls() {
        assert_eq!(
            sanitize_remote_url("https://alice:secret@example.com/org/repo.git?token=hidden#frag").unwrap(),
            "https://example.com/org/repo.git"
        );
        assert_eq!(
            sanitize_remote_url("alice@example.com:org/repo.git").unwrap(),
            "alice@example.com:org/repo.git"
        );
        assert_eq!(
            sanitize_remote_url("alice:password@example.com:org/repo.git").unwrap(),
            "git@example.com:org/repo.git"
        );
        assert_eq!(
            sanitize_remote_url("ssh://alice:password@example.com/org/repo.git").unwrap(),
            "ssh://alice@example.com/org/repo.git"
        );
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
