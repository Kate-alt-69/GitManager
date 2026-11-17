use git2::{BranchType, ObjectType, Repository, Status, StatusOptions};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::time::UNIX_EPOCH;
use std::ffi::OsStr;
use walkdir::WalkDir;
use crate::process_utils::hidden_command;

const MAX_PREVIEW_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 2_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChange {
    pub path: String,
    pub index_status: String,
    pub worktree_status: String,
    pub staged: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullPreview {
    pub repo_name: String,
    pub branch: String,
    pub remote_name: String,
    pub remote_branch: String,
    pub local_oid: String,
    pub remote_oid: String,
    pub commits_ahead: usize,
    pub commits_behind: usize,
    pub changes: Vec<WorktreeChange>,
    pub status_token: String,
    pub can_pull: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullExecutionResult {
    pub message: String,
    pub commit_created: bool,
    pub stash_created: bool,
    pub rebased: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryCommitPreview {
    pub changes: Vec<WorktreeChange>,
    pub index_tree: String,
}

fn validate_relative_path(value: &str) -> Result<PathBuf, String> {
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute() {
        return Err("An absolute path cannot be opened inside a repository.".to_string());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err("The requested path escapes the repository.".to_string()),
        }
    }
    Ok(path.to_path_buf())
}

fn validate_browser_relative_path(value: &str) -> Result<PathBuf, String> {
    let path = validate_relative_path(value)?;
    if path.components().any(|component| {
        matches!(component, Component::Normal(part) if part.to_string_lossy().eq_ignore_ascii_case(".git"))
    }) {
        return Err("Git metadata is not available in the repository file browser.".to_string());
    }
    Ok(path)
}

fn canonical_local_path(repo: &Repository, relative_path: &str) -> Result<(PathBuf, PathBuf), String> {
    let workdir = repo.workdir().ok_or_else(|| "This repository has no working directory.".to_string())?;
    let root = fs::canonicalize(workdir).map_err(|error| format!("Could not resolve repository directory: {error}"))?;
    let relative = validate_browser_relative_path(relative_path)?;
    let mut cursor = root.clone();
    for component in relative.components() {
        if let Component::Normal(part) = component {
            cursor.push(part);
            let metadata = fs::symlink_metadata(&cursor)
                .map_err(|error| format!("Could not inspect repository path: {error}"))?;
            if metadata.file_type().is_symlink() {
                return Err("Repository file browsing does not follow symbolic links.".to_string());
            }
        }
    }
    let candidate = root.join(relative);
    let canonical = fs::canonicalize(&candidate).map_err(|error| format!("Could not open repository path: {error}"))?;
    if !canonical.starts_with(&root) {
        return Err("The requested path resolves outside the repository.".to_string());
    }
    Ok((root, canonical))
}

fn remote_commit<'repo>(repo: &'repo Repository, branch_name: &str) -> Result<git2::Commit<'repo>, String> {
    let branch = repo.find_branch(branch_name, BranchType::Remote)
        .map_err(|error| format!("Could not find remote branch {branch_name}: {error}"))?;
    let target = branch.get().target().ok_or_else(|| "The selected remote branch has no commit.".to_string())?;
    repo.find_commit(target).map_err(|error| format!("Could not load remote branch commit: {error}"))
}

fn remote_tree_for_path<'repo>(repo: &'repo Repository, branch_name: &str, relative_path: &str) -> Result<git2::Tree<'repo>, String> {
    let commit = remote_commit(repo, branch_name)?;
    let root = commit.tree().map_err(|error| format!("Could not load remote repository tree: {error}"))?;
    if relative_path.trim().is_empty() { return Ok(root); }
    let path = validate_browser_relative_path(relative_path)?;
    let entry = root.get_path(&path).map_err(|error| format!("Could not open remote directory: {error}"))?;
    if entry.kind() != Some(ObjectType::Tree) {
        return Err("The selected remote path is not a directory.".to_string());
    }
    entry.to_object(repo).map_err(|error| error.to_string())?
        .into_tree().map_err(|_| "Could not open remote directory tree.".to_string())
}

pub fn list_repository_files(
    repo_path: &str,
    relative_path: Option<&str>,
    source: &str,
    branch: Option<&str>,
) -> Result<Vec<RepositoryFileEntry>, String> {
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    let relative = relative_path.unwrap_or("");
    let mut entries = Vec::new();

    if source.eq_ignore_ascii_case("remote") {
        let branch_name = branch.filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Select a remote branch to browse.".to_string())?;
        let tree = remote_tree_for_path(&repo, branch_name, relative)?;
        for entry in tree.iter() {
            let name = entry.name().unwrap_or("").to_string();
            if name.is_empty() || name.eq_ignore_ascii_case(".git") { continue; }
            let path = if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
            let is_dir = entry.kind() == Some(ObjectType::Tree);
            let is_symlink = entry.filemode() == 0o120000;
            let size_bytes = if entry.kind() == Some(ObjectType::Blob) {
                repo.find_blob(entry.id()).map(|blob| blob.size() as u64).unwrap_or(0)
            } else { 0 };
            if entries.len() >= MAX_DIRECTORY_ENTRIES {
                return Err(format!(
                    "This remote folder contains more than {MAX_DIRECTORY_ENTRIES} items. Open a subfolder to browse a smaller section."
                ));
            }
            entries.push(RepositoryFileEntry { name, path, is_dir, is_symlink, size_bytes });
        }
    } else {
        let (_, directory) = canonical_local_path(&repo, relative)?;
        if !directory.is_dir() { return Err("The selected local path is not a directory.".to_string()); }
        let root = fs::canonicalize(repo.workdir().ok_or_else(|| "This repository has no working directory.".to_string())?)
            .map_err(|error| error.to_string())?;
        let read_dir = fs::read_dir(&directory).map_err(|error| format!("Could not list repository directory: {error}"))?;
        for item in read_dir {
            let item = item.map_err(|error| format!("Could not read directory entry: {error}"))?;
            let name = item.file_name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case(".git") { continue; }
            let file_type = item.file_type().map_err(|error| format!("Could not inspect {name}: {error}"))?;
            let is_symlink = file_type.is_symlink();
            let is_dir = file_type.is_dir();
            let path = if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
            let size_bytes = if !is_symlink && !is_dir {
                item.metadata().map(|metadata| metadata.len()).unwrap_or(0)
            } else { 0 };
            if !is_symlink {
                let canonical = fs::canonicalize(item.path()).map_err(|error| error.to_string())?;
                if !canonical.starts_with(&root) { continue; }
            }
            if entries.len() >= MAX_DIRECTORY_ENTRIES {
                return Err(format!(
                    "This local folder contains more than {MAX_DIRECTORY_ENTRIES} items. Open a subfolder to browse a smaller section."
                ));
            }
            entries.push(RepositoryFileEntry { name, path, is_dir, is_symlink, size_bytes });
        }
    }

    entries.sort_by(|left, right| right.is_dir.cmp(&left.is_dir)
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())));
    Ok(entries)
}

/// Write an edited conflict file back to the local working tree. This is
/// deliberately limited to paths Git currently reports as unmerged, regular
/// UTF-8 files inside the repository, and the same 1 MiB editor limit as preview.
pub fn save_conflict_resolution(
    repo_path: &str,
    relative_path: &str,
    content: &str,
) -> Result<String, String> {
    if content.len() as u64 > MAX_PREVIEW_FILE_BYTES {
        return Err("Conflict editor is limited to 1 MiB per file.".to_string());
    }

    let repo = Repository::open(repo_path)
        .map_err(|error| format!("Could not open Git repository: {error}"))?;
    let relative = validate_browser_relative_path(relative_path)?;
    if relative.as_os_str().is_empty() {
        return Err("Select a conflicted file first.".to_string());
    }
    let normalized = relative.to_string_lossy().replace('\\', "/");
    let is_conflicted = collect_worktree_changes(&repo)?
        .iter()
        .any(|change| {
            change.path == normalized
                && (change.index_status == "Conflict" || change.worktree_status == "Conflict")
        });
    if !is_conflicted {
        return Err("This file is no longer marked as conflicted by Git. Refresh the conflict list before editing it.".to_string());
    }

    let (root, file) = canonical_local_path(&repo, relative_path)?;
    if !file.starts_with(&root) || !file.is_file() {
        return Err("The selected conflict must be an existing regular file inside this repository.".to_string());
    }
    let metadata = fs::symlink_metadata(&file)
        .map_err(|error| format!("Could not inspect the selected conflict file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Conflict editing does not follow symbolic links or write non-file entries.".to_string());
    }
    if metadata.len() > MAX_PREVIEW_FILE_BYTES {
        return Err("Conflict editor is limited to 1 MiB per file.".to_string());
    }
    let parent = file.parent()
        .ok_or_else(|| "The selected conflict file has no parent directory.".to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".gitmanager-conflict-{}-{nonce}.tmp",
        std::process::id(),
    ));

    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Could not create temporary conflict file: {error}"))?;
    if let Err(error) = output.write_all(content.as_bytes()).and_then(|_| output.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not save the temporary conflict contents: {error}"));
    }
    drop(output);
    fs::set_permissions(&temporary, metadata.permissions())
        .map_err(|error| format!("Could not preserve conflict file permissions: {error}"))?;

    #[cfg(windows)]
    {
        let backup = parent.join(format!(
            ".gitmanager-conflict-{}-{nonce}.previous",
            std::process::id(),
        ));
        fs::rename(&file, &backup)
            .map_err(|error| format!("Could not stage the previous conflict file: {error}"))?;
        if let Err(error) = fs::rename(&temporary, &file) {
            let _ = fs::rename(&backup, &file);
            let _ = fs::remove_file(&temporary);
            return Err(format!("Could not publish the resolved conflict file: {error}"));
        }
        let _ = fs::remove_file(&backup);
    }

    #[cfg(not(windows))]
    {
        if let Err(error) = fs::rename(&temporary, &file) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Could not publish the resolved conflict file: {error}"));
        }
    }

    Ok(format!(
        "Saved {normalized}. Stage the resolved file to mark this conflict resolved."
    ))
}

pub fn read_repository_file(
    repo_path: &str,
    relative_path: &str,
    source: &str,
    branch: Option<&str>,
) -> Result<String, String> {
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    let path = validate_browser_relative_path(relative_path)?;
    if path.as_os_str().is_empty() { return Err("Select a file to preview.".to_string()); }

    let bytes = if source.eq_ignore_ascii_case("remote") {
        let branch_name = branch.filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Select a remote branch to browse.".to_string())?;
        let commit = remote_commit(&repo, branch_name)?;
        let tree = commit.tree().map_err(|error| error.to_string())?;
        let entry = tree.get_path(&path).map_err(|error| format!("Could not open remote file: {error}"))?;
        if entry.kind() != Some(ObjectType::Blob) || entry.filemode() == 0o120000 {
            return Err("The selected remote item is not a regular text file.".to_string());
        }
        let blob = repo.find_blob(entry.id()).map_err(|error| error.to_string())?;
        if blob.size() as u64 > MAX_PREVIEW_FILE_BYTES {
            return Err("File preview is limited to 1 MiB. Open the file externally to inspect larger files.".to_string());
        }
        blob.content().to_vec()
    } else {
        let (root, file) = canonical_local_path(&repo, relative_path)?;
        if !file.starts_with(&root) || !file.is_file() {
            return Err("The selected local path is not a regular file inside this repository.".to_string());
        }
        let metadata = fs::metadata(&file).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_PREVIEW_FILE_BYTES {
            return Err("File preview is limited to 1 MiB. Open the file externally to inspect larger files.".to_string());
        }
        fs::read(&file).map_err(|error| format!("Could not read repository file: {error}"))?
    };
    String::from_utf8(bytes).map_err(|_| "This file is not UTF-8 text and cannot be previewed in the text viewer.".to_string())
}

fn index_status_label(status: Status) -> String {
    if status.contains(Status::CONFLICTED) { "Conflict".to_string() }
    else if status.contains(Status::INDEX_NEW) { "Added".to_string() }
    else if status.contains(Status::INDEX_MODIFIED) { "Modified".to_string() }
    else if status.contains(Status::INDEX_DELETED) { "Deleted".to_string() }
    else if status.contains(Status::INDEX_RENAMED) { "Renamed".to_string() }
    else if status.contains(Status::INDEX_TYPECHANGE) { "Type changed".to_string() }
    else { "Clean".to_string() }
}

fn worktree_status_label(status: Status) -> String {
    if status.contains(Status::WT_NEW) { "Untracked".to_string() }
    else if status.contains(Status::WT_MODIFIED) { "Modified".to_string() }
    else if status.contains(Status::WT_DELETED) { "Deleted".to_string() }
    else if status.contains(Status::WT_RENAMED) { "Renamed".to_string() }
    else if status.contains(Status::WT_TYPECHANGE) { "Type changed".to_string() }
    else if status.contains(Status::CONFLICTED) { "Conflict".to_string() }
    else { "Clean".to_string() }
}

fn collect_worktree_changes(repo: &Repository) -> Result<Vec<WorktreeChange>, String> {
    let mut options = StatusOptions::new();
    options.include_untracked(true).recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut options))
        .map_err(|error| format!("Could not inspect repository changes: {error}"))?;
    let mut changes = Vec::new();
    for entry in statuses.iter() {
        let status = entry.status();
        let path = entry.path().unwrap_or("<non-UTF-8 path>").replace('\\', "/");
        let index_status = index_status_label(status);
        let worktree_status = worktree_status_label(status);
        let staged = index_status != "Clean";
        changes.push(WorktreeChange { path, index_status, worktree_status, staged });
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(changes)
}

fn status_token(changes: &[WorktreeChange]) -> String {
    let canonical = changes.iter().map(|change| {
        format!("{}\0{}\0{}\n", change.path, change.index_status, change.worktree_status)
    }).fold(String::new(), |mut output, line| { output.push_str(&line); output });
    let hash = canonical.as_bytes().iter().fold(0xcbf29ce484222325_u64, |value, byte| {
        (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

fn repository_status_token(repo: &Repository, changes: &[WorktreeChange]) -> String {
    let mut hasher = DefaultHasher::new();
    status_token(changes).hash(&mut hasher);
    if let Some(workdir) = repo.workdir() {
        for change in changes {
            let path = workdir.join(&change.path);
            match fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    metadata.len().hash(&mut hasher);
                    metadata.modified().ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map(|duration| duration.as_nanos())
                        .unwrap_or(0)
                        .hash(&mut hasher);
                    if metadata.is_file() && metadata.len() <= 8 * 1024 * 1024 {
                        fs::read(&path).unwrap_or_default().hash(&mut hasher);
                    }
                }
                Err(_) => 0_u8.hash(&mut hasher),
            }
        }
    }
    if let Ok(index) = repo.index() {
        if let Some(index_path) = index.path() {
            if let Ok(metadata) = fs::metadata(index_path) {
                metadata.len().hash(&mut hasher);
                metadata.modified().ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0)
                    .hash(&mut hasher);
                if metadata.len() <= 32 * 1024 * 1024 {
                    fs::read(index_path).unwrap_or_default().hash(&mut hasher);
                }
            }
        }
    }
    format!("{:016x}", hasher.finish())
}

pub fn preview_repository_pull(repo_path: &str) -> Result<PullPreview, String> {
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    let head = repo.head().map_err(|error| format!("Could not read repository HEAD: {error}"))?;
    if !head.is_branch() {
        return Err("Pull is unavailable while HEAD is detached. Switch to a local branch with an upstream first.".to_string());
    }
    let branch_name = head.shorthand().ok_or_else(|| "Could not resolve the current branch name.".to_string())?.to_string();
    let local_oid = head.target().ok_or_else(|| "The current branch has no commit yet.".to_string())?;
    let branch = repo.find_branch(&branch_name, BranchType::Local)
        .map_err(|error| format!("Could not inspect local branch: {error}"))?;
    let upstream = branch.upstream()
        .map_err(|_| format!("Branch {branch_name} has no configured upstream. Configure an upstream before syncing."))?;
    let upstream_name = upstream.name().map_err(|error| error.to_string())?
        .ok_or_else(|| "The configured upstream has an invalid name.".to_string())?.to_string();
    let remote_path = upstream_name.strip_prefix("refs/remotes/")
        .ok_or_else(|| format!("Unsupported upstream reference: {upstream_name}"))?;
    let (remote_name, remote_branch) = remote_path.split_once('/')
        .ok_or_else(|| format!("Unsupported upstream reference: {upstream_name}"))?;
    let remote_name = remote_name.to_string();
    let remote_branch = remote_branch.to_string();

    let mut remote = repo.find_remote(&remote_name)
        .map_err(|error| format!("Could not open remote {remote_name}: {error}"))?;
    let refspec = format!("+refs/heads/{remote_branch}:refs/remotes/{remote_name}/{remote_branch}");
    remote.fetch(&[&refspec], None, None)
        .map_err(|error| format!("Could not fetch {remote_name}/{remote_branch}: {error}"))?;
    let remote_oid = repo.refname_to_id(&upstream_name)
        .map_err(|error| format!("Could not resolve fetched upstream {upstream_name}: {error}"))?;
    let (commits_ahead, commits_behind) = repo.graph_ahead_behind(local_oid, remote_oid)
        .map_err(|error| format!("Could not compare local and remote history: {error}"))?;
    let changes = collect_worktree_changes(&repo)?;
    let token = repository_status_token(&repo, &changes);
    let repo_name = repo_path.trim_end_matches(|character| character == '/' || character == '\\')
        .rsplit(|character| character == '/' || character == '\\')
        .find(|value| !value.is_empty()).unwrap_or("Repository").to_string();

    Ok(PullPreview {
        repo_name, branch: branch_name, remote_name, remote_branch,
        local_oid: local_oid.to_string(), remote_oid: remote_oid.to_string(),
        commits_ahead, commits_behind, changes, status_token: token,
        can_pull: commits_behind > 0 && commits_ahead == 0,
    })
}

fn run_git(repo_path: &str, args: &[&str]) -> Result<String, String> {
    let output = hidden_command("git").arg("-C").arg(repo_path).arg("--literal-pathspecs").args(args)
        .output().map_err(|error| format!("Could not start Git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if !stderr.is_empty() { stderr } else if !stdout.is_empty() { stdout } else { format!("Git exited with status {}.", output.status) })
    }
}

fn validate_stage_path(value: &str) -> Result<(), String> {
    validate_git_operation_path(value).map(|_| ())
}

fn validate_git_operation_path(value: &str) -> Result<PathBuf, String> {
    let path = validate_relative_path(value)?;
    if path.as_os_str().is_empty() {
        return Err("A selected repository path is empty.".to_string());
    }
    if path.components().any(|component| matches!(component, Component::Normal(part) if part.to_string_lossy().eq_ignore_ascii_case(".git"))) {
        return Err("GitManager does not allow file operations inside the .git directory.".to_string());
    }
    Ok(path)
}

pub fn get_repository_changes(repo_path: &str) -> Result<Vec<WorktreeChange>, String> {
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    collect_worktree_changes(&repo)
}

pub fn read_repository_file_diff(repo_path: &str, relative_path: &str, staged: bool) -> Result<String, String> {
    validate_git_operation_path(relative_path)?;
    let args = if staged {
        vec!["diff", "--cached", "--no-ext-diff", "--no-color", "--", relative_path]
    } else {
        vec!["diff", "--no-ext-diff", "--no-color", "--", relative_path]
    };
    let diff = run_git(repo_path, &args)
        .map_err(|error| format!("Could not read the selected file diff: {error}"))?;
    if diff.len() > 512 * 1024 {
        return Err("Diff preview is limited to 512 KiB. Use the file preview or open the diff in an external Git tool.".to_string());
    }
    if diff.trim().is_empty() {
        return Ok(if staged {
            "No staged diff for this file. It may be untracked, unchanged in the index, or already fully unstaged.".to_string()
        } else {
            "No unstaged diff for this file. Untracked files do not have a Git diff; preview the file contents instead.".to_string()
        });
    }
    Ok(diff)
}

pub fn fetch_repository_remote(repo_path: &str, remote_name: &str) -> Result<String, String> {
    if remote_name.trim().is_empty() {
        return Err("Select a configured remote before fetching.".to_string());
    }
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    let remotes = repo.remotes().map_err(|error| format!("Could not list repository remotes: {error}"))?;
    if !remotes.iter().flatten().any(|name| name == remote_name) {
        return Err(format!("Remote {remote_name} is not configured in this repository."));
    }
    let mut remote = repo.find_remote(remote_name)
        .map_err(|error| format!("Could not open remote {remote_name}: {error}"))?;
    remote.fetch::<&str>(&[], None, None)
        .map_err(|error| format!("Could not fetch {remote_name}: {error}"))?;
    Ok(format!("Fetched remote-tracking references for {remote_name}."))
}

pub fn stage_repository_paths(repo_path: &str, paths: &[String]) -> Result<Vec<WorktreeChange>, String> {
    if paths.is_empty() { return Err("Select at least one file to stage.".to_string()); }
    for path in paths { validate_git_operation_path(path)?; }
    for batch in paths.chunks(64) {
        let mut args = vec!["add", "--"];
        for path in batch { args.push(path.as_str()); }
        run_git(repo_path, &args).map_err(|error| format!("Could not stage the selected files: {error}"))?;
    }
    get_repository_changes(repo_path)
}

pub fn unstage_repository_paths(repo_path: &str, paths: &[String]) -> Result<Vec<WorktreeChange>, String> {
    if paths.is_empty() { return Err("Select at least one staged file to unstage.".to_string()); }
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    for value in paths { validate_git_operation_path(value)?; }
    let has_head = repo.head().ok().and_then(|head| head.target()).is_some();
    // Reset selected index entries to HEAD when one exists. In an unborn
    // repository, remove only the index entries so first-commit files stay on disk.
    for batch in paths.chunks(64) {
        let mut args = if has_head {
            vec!["reset", "--"]
        } else {
            vec!["rm", "--cached", "--ignore-unmatch", "--"]
        };
        for path in batch { args.push(path.as_str()); }
        run_git(repo_path, &args)
            .map_err(|error| format!("Could not unstage the selected files: {error}"))?;
    }
    collect_worktree_changes(&repo)
}

pub fn preview_repository_commit(repo_path: &str) -> Result<RepositoryCommitPreview, String> {
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    let mut index = repo.index().map_err(|error| format!("Could not open repository index: {error}"))?;
    let index_tree = index.write_tree().map_err(|error| format!("Could not preview staged files: {error}"))?;
    Ok(RepositoryCommitPreview {
        changes: collect_worktree_changes(&repo)?,
        index_tree: index_tree.to_string(),
    })
}

pub fn create_repository_commit(repo_path: &str, message: &str, expected_index_tree: &str) -> Result<(String, Vec<WorktreeChange>), String> {
    let message = message.trim();
    if message.is_empty() { return Err("Enter a commit message before creating a commit.".to_string()); }
    if message.len() > 200 { return Err("Commit messages must be 200 characters or fewer.".to_string()); }
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    let mut index = repo.index().map_err(|error| format!("Could not open repository index: {error}"))?;
    let staged_tree = index.write_tree().map_err(|error| format!("Could not write staged tree: {error}"))?;
    if staged_tree.to_string() != expected_index_tree {
        return Err("The staged index changed after this commit preview. Nothing was committed; review the current staged files again.".to_string());
    }
    if let Ok(head) = repo.head() {
        if let Ok(commit) = head.peel_to_commit() {
            let current_tree = commit.tree().map_err(|error| format!("Could not read HEAD tree: {error}"))?;
            if current_tree.id() == staged_tree {
                return Err("There are no staged changes to commit. Stage files first.".to_string());
            }
        }
    } else if index.is_empty() {
        return Err("There are no staged changes to commit. Stage files first.".to_string());
    }
    run_git(repo_path, &["commit", "-m", message])
        .map_err(|error| format!("Git could not create the commit: {error}"))?;
    let updated = Repository::open(repo_path).map_err(|error| error.to_string())?;
    let oid = updated.head().map_err(|error| error.to_string())?
        .target().ok_or_else(|| "Git created a commit but HEAD has no target.".to_string())?;
    let changes = collect_worktree_changes(&updated)?;
    Ok((oid.to_string(), changes))
}

pub fn delete_repository_path(repo_path: &str, relative_path: &str) -> Result<String, String> {
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    let workdir = repo.workdir().ok_or_else(|| "This repository has no working directory.".to_string())?;
    let root = fs::canonicalize(workdir).map_err(|error| format!("Could not resolve repository directory: {error}"))?;
    let relative = validate_git_operation_path(relative_path)?;
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    let mut parent_cursor = root.clone();
    for component in parent_relative.components() {
        if let Component::Normal(part) = component {
            parent_cursor.push(part);
            let metadata = fs::symlink_metadata(&parent_cursor)
                .map_err(|error| format!("Could not inspect the selected item's parent: {error}"))?;
            if metadata.file_type().is_symlink() {
                return Err("Deletion does not follow symbolic-link directories.".to_string());
            }
        }
    }
    let parent = fs::canonicalize(&parent_cursor)
        .map_err(|error| format!("Could not resolve the selected item's parent directory: {error}"))?;
    if !parent.starts_with(&root) {
        return Err("The selected item is outside the repository.".to_string());
    }
    let name = relative.file_name().ok_or_else(|| "Select a file or folder inside the repository.".to_string())?;
    let target = parent.join(name);
    let metadata = fs::symlink_metadata(&target)
        .map_err(|error| format!("Could not inspect the selected item: {error}"))?;
    if metadata.file_type().is_symlink() {
        fs::remove_file(&target).map_err(|error| format!("Could not remove the symbolic link: {error}"))?;
    } else if metadata.is_dir() {
        let mut contains_git_metadata = false;
        for item in WalkDir::new(&target).follow_links(false) {
            let entry = item.map_err(|error| format!(
                "Could not safely inspect this folder before deletion: {error}"
            ))?;
            if entry.file_name().to_string_lossy().eq_ignore_ascii_case(".git") {
                contains_git_metadata = true;
                break;
            }
        }
        if contains_git_metadata {
            return Err("This folder contains a nested Git repository or .git metadata. Remove that repository separately first.".to_string());
        }
        fs::remove_dir_all(&target).map_err(|error| format!("Could not remove the selected directory: {error}"))?;
    } else {
        fs::remove_file(&target).map_err(|error| format!("Could not remove the selected file: {error}"))?;
    }
    Ok(format!("Deleted {relative_path}. Tracked deletions are now visible in Changes and can still be restored before committing."))
}

pub fn prepare_and_pull_repository_update(
    repo_path: &str,
    expected_branch: &str,
    expected_local_oid: &str,
    expected_remote_oid: &str,
    expected_status_token: &str,
    mode: &str,
    stage_paths: &[String],
    commit_message: &str,
) -> Result<PullExecutionResult, String> {
    let preview = preview_repository_pull(repo_path)?;
    if preview.branch != expected_branch || preview.local_oid != expected_local_oid
        || preview.remote_oid != expected_remote_oid || preview.status_token != expected_status_token {
        return Err("The repository changed after this preview. Nothing was staged, committed, stashed or pulled. Review a fresh Sync preview.".to_string());
    }
    if !preview.can_pull {
        return Err("This branch is not a fast-forward-only pull candidate. Review its ahead/behind state before syncing.".to_string());
    }
    if preview.changes.iter().any(|change| change.index_status == "Conflict" || change.worktree_status == "Conflict") {
        return Err("Resolve the existing merge conflicts before preparing a Pull.".to_string());
    }

    let mut commit_created = false;
    let mut stash_created = false;
    let has_changes = !preview.changes.is_empty();

    match mode {
        "stash" if has_changes => {
            let message = format!("GitManager: before pull from {}/{}", preview.remote_name, preview.remote_branch);
            run_git(repo_path, &["stash", "push", "--include-untracked", "--message", &message])
                .map_err(|error| format!("Could not stash local changes; pull was not started: {error}"))?;
            stash_created = true;
        }
        "commit" if has_changes => {
            let message = commit_message.trim();
            if message.is_empty() { return Err("Enter a commit message before confirming Pull.".to_string()); }
            let all_paths: std::collections::HashSet<&str> = preview.changes.iter().map(|change| change.path.as_str()).collect();
            let selected: std::collections::HashSet<&str> = stage_paths.iter().map(String::as_str).collect();
            if selected.is_empty() || selected.iter().any(|path| !all_paths.contains(path)) {
                return Err("Select at least one changed path to commit. Unchecked paths will be stashed and preserved.".to_string());
            }
            for path in stage_paths { validate_stage_path(path)?; }
            let stash_paths: Vec<&str> = preview.changes.iter()
                .filter(|change| !selected.contains(change.path.as_str()))
                .map(|change| change.path.as_str())
                .collect();
            // Remove every unchecked path from the index/worktree before creating the commit.
            // Otherwise an already-staged, unchecked file could be included by the commit.
            if !stash_paths.is_empty() {
                let stash_message = format!("GitManager: unselected changes before pull from {}/{}", preview.remote_name, preview.remote_branch);
                let mut stash_args = vec!["stash", "push", "--include-untracked", "--message", stash_message.as_str(), "--"];
                stash_args.extend(stash_paths.iter().copied());
                run_git(repo_path, &stash_args)
                    .map_err(|error| format!("Could not stash the unchecked paths; no commit or Pull was created: {error}"))?;
                stash_created = true;
            }
            let mut add_args = vec!["add", "--"];
            for path in stage_paths { add_args.push(path.as_str()); }
            run_git(repo_path, &add_args)
                .map_err(|error| format!("Could not stage the selected files; Pull was not started. Any created stash was preserved: {error}"))?;
            run_git(repo_path, &["commit", "-m", message])
                .map_err(|error| format!("Could not create the requested commit; Pull was not started. Any created stash was preserved: {error}"))?;
            commit_created = true;
        }
        "none" if !has_changes => {}
        "none" => return Err("The working tree has changes. Choose Commit or Stash before pulling.".to_string()),
        "stash" | "commit" => return Err("There are no local changes to handle. Refresh the Sync preview.".to_string()),
        _ => return Err("Unknown Sync preparation option.".to_string()),
    }

    let after_prepare = Repository::open(repo_path).map_err(|error| error.to_string())?;
    let remaining = collect_worktree_changes(&after_prepare)?;
    if !remaining.is_empty() {
        let preparation = if commit_created { "The local commit was created" } else if stash_created { "The stash was saved" } else { "The worktree still has changes" };
        return Err(format!("{preparation}, but {} change(s) remain. Pull was not started; review the repository before continuing.", remaining.len()));
    }

    let remote = preview.remote_name.as_str();
    let branch = preview.remote_branch.as_str();
    let upstream_ref = format!("refs/remotes/{remote}/{branch}");
    let output = if commit_created {
        run_git(repo_path, &["rebase", &upstream_ref])
            .map_err(|error| format!("The local commit was created, but rebase/pull did not complete: {error}. Resolve any conflicts, then use Git's rebase continue or abort workflow."))?
    } else {
        run_git(repo_path, &["merge", "--ff-only", &upstream_ref])
            .map_err(|error| {
                let recovery = if stash_created { " Your stash was preserved; it has not been applied." } else { "" };
                format!("Pull failed: {error}.{recovery}")
            })?
    };

    Ok(PullExecutionResult {
        message: if output.is_empty() { format!("Updated {} from {}/{}.", preview.branch, remote, branch) } else { output },
        commit_created, stash_created, rebased: commit_created,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        hidden_command, status_token, validate_browser_relative_path, validate_git_operation_path,
        validate_relative_path, Repository, WorktreeChange,
    };
    use std::fs;

    #[test]
    fn conflict_resolution_uses_the_same_repository_path_guard() {
        assert!(super::validate_browser_relative_path("src/main.rs").is_ok());
        assert!(super::validate_browser_relative_path("../outside.txt").is_err());
        assert!(super::validate_browser_relative_path(".git/config").is_err());
    }

    #[test]
    fn repository_paths_reject_parent_traversal() {
        assert!(validate_relative_path("../outside").is_err());
        assert!(validate_relative_path("src/../../outside").is_err());
        assert!(validate_relative_path("src/main.rs").is_ok());
    }

    #[test]
    fn git_operations_cannot_target_git_metadata() {
        assert!(validate_git_operation_path(".git").is_err());
        assert!(validate_git_operation_path(".git/config").is_err());
        assert!(validate_git_operation_path("src/main.rs").is_ok());
        assert!(validate_git_operation_path(".gitignore").is_ok());
    }

    #[test]
    fn file_browser_cannot_read_git_metadata_at_any_depth() {
        assert!(validate_browser_relative_path(".git/config").is_err());
        assert!(validate_browser_relative_path("nested/.git/config").is_err());
        assert!(validate_browser_relative_path("nested/src/main.rs").is_ok());
    }

    #[test]
    fn status_token_changes_when_a_file_status_changes() {
        let before = vec![WorktreeChange {
            path: "src/main.rs".to_string(), index_status: "Clean".to_string(),
            worktree_status: "Modified".to_string(), staged: false,
        }];
        let mut after = before.clone();
        after[0].worktree_status = "Deleted".to_string();
        assert_ne!(status_token(&before), status_token(&after));
    }

    fn create_test_repository() -> (std::path::PathBuf, Repository) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("gitmanager-unstage-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        let repo = Repository::init(&path).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "GitManager Tests").unwrap();
        config.set_str("user.email", "gitmanager-tests@example.invalid").unwrap();
        fs::write(path.join("tracked.txt"), "original\n").unwrap();
        let add = hidden_command("git").arg("-C").arg(&path).args(["add", "--", "tracked.txt"]).output().unwrap();
        assert!(add.status.success(), "git add failed: {}", String::from_utf8_lossy(&add.stderr));
        let commit = hidden_command("git").arg("-C").arg(&path).args(["commit", "-m", "initial"]).output().unwrap();
        assert!(commit.status.success(), "git commit failed: {}", String::from_utf8_lossy(&commit.stderr));
        (path, repo)
    }

    #[test]
    fn unstage_preserves_tracked_worktree_edits() {
        let (path, repo) = create_test_repository();
        fs::write(path.join("tracked.txt"), "edited locally\n").unwrap();
        let add = hidden_command("git").arg("-C").arg(&path).args(["add", "--", "tracked.txt"]).output().unwrap();
        assert!(add.status.success());
        let changes = super::unstage_repository_paths(
            path.to_str().unwrap(),
            &["tracked.txt".to_string()],
        ).unwrap();
        let tracked = changes.iter().find(|change| change.path == "tracked.txt").unwrap();
        assert_eq!(tracked.index_status, "Clean");
        assert_eq!(tracked.worktree_status, "Modified");
        assert!(!tracked.staged);
        assert_eq!(fs::read_to_string(path.join("tracked.txt")).unwrap(), "edited locally\n");
        drop(repo);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn unstage_turns_a_staged_addition_back_into_an_untracked_file() {
        let (path, repo) = create_test_repository();
        fs::write(path.join("new.txt"), "new local file\n").unwrap();
        let add = hidden_command("git").arg("-C").arg(&path).args(["add", "--", "new.txt"]).output().unwrap();
        assert!(add.status.success());
        let changes = super::unstage_repository_paths(
            path.to_str().unwrap(),
            &["new.txt".to_string()],
        ).unwrap();
        let added = changes.iter().find(|change| change.path == "new.txt").unwrap();
        assert_eq!(added.index_status, "Clean");
        assert_eq!(added.worktree_status, "Untracked");
        assert!(!added.staged);
        assert_eq!(fs::read_to_string(path.join("new.txt")).unwrap(), "new local file\n");
        drop(repo);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn unstage_works_before_the_first_commit_exists() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("gitmanager-unborn-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        let repo = Repository::init(&path).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "GitManager Tests").unwrap();
        config.set_str("user.email", "gitmanager-tests@example.invalid").unwrap();
        fs::write(path.join("first.txt"), "first commit candidate\n").unwrap();
        let add = hidden_command("git").arg("-C").arg(&path).args(["add", "--", "first.txt"]).output().unwrap();
        assert!(add.status.success());
        assert!(repo.head().is_err());
        let changes = super::unstage_repository_paths(
            path.to_str().unwrap(),
            &["first.txt".to_string()],
        ).unwrap();
        let added = changes.iter().find(|change| change.path == "first.txt").unwrap();
        assert_eq!(added.index_status, "Clean");
        assert_eq!(added.worktree_status, "Untracked");
        assert!(!added.staged);
        assert_eq!(fs::read_to_string(path.join("first.txt")).unwrap(), "first commit candidate\n");
        drop(repo);
        fs::remove_dir_all(path).unwrap();
    }
}