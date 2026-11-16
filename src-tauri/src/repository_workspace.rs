use git2::{BranchType, ObjectType, Repository, Status, StatusOptions};
use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};
use crate::process_utils::hidden_command;

const MAX_PREVIEW_FILE_BYTES: u64 = 1024 * 1024;

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

fn canonical_local_path(repo: &Repository, relative_path: &str) -> Result<(PathBuf, PathBuf), String> {
    let workdir = repo.workdir().ok_or_else(|| "This repository has no working directory.".to_string())?;
    let root = fs::canonicalize(workdir).map_err(|error| format!("Could not resolve repository directory: {error}"))?;
    let relative = validate_relative_path(relative_path)?;
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
    let path = validate_relative_path(relative_path)?;
    let entry = root.get_path(&path).map_err(|error| format!("Could not open remote directory: {error}"))?;
    if entry.kind() != Some(ObjectType::Tree) {
        return Err("The selected remote path is not a directory.".to_string());
    }
    entry.to_object(repo).map_err(|error| error.to_string())?
        .into_tree().map_err(|error| format!("Could not open remote directory tree: {error}"))
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
            if name.is_empty() { continue; }
            let path = if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
            let is_dir = entry.kind() == Some(ObjectType::Tree);
            let is_symlink = entry.filemode() == 0o120000;
            let size_bytes = if entry.kind() == Some(ObjectType::Blob) {
                repo.find_blob(entry.id()).map(|blob| blob.size() as u64).unwrap_or(0)
            } else { 0 };
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
            if name == ".git" { continue; }
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
            entries.push(RepositoryFileEntry { name, path, is_dir, is_symlink, size_bytes });
        }
    }

    entries.sort_by(|left, right| right.is_dir.cmp(&left.is_dir)
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())));
    Ok(entries)
}

pub fn read_repository_file(
    repo_path: &str,
    relative_path: &str,
    source: &str,
    branch: Option<&str>,
) -> Result<String, String> {
    let repo = Repository::open(repo_path).map_err(|error| format!("Could not open Git repository: {error}"))?;
    let path = validate_relative_path(relative_path)?;
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
    let token = status_token(&changes);
    let repo_name = repo_path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next()
        .filter(|value| !value.is_empty()).unwrap_or("Repository").to_string();

    Ok(PullPreview {
        repo_name, branch: branch_name, remote_name, remote_branch,
        local_oid: local_oid.to_string(), remote_oid: remote_oid.to_string(),
        commits_ahead, commits_behind, changes, status_token: token,
        can_pull: commits_behind > 0 && commits_ahead == 0,
    })
}

fn run_git(repo_path: &str, args: &[&str]) -> Result<String, String> {
    let output = hidden_command("git").arg("-C").arg(repo_path).args(args)
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
    let path = validate_relative_path(value)?;
    if path.as_os_str().is_empty() { return Err("A selected file path is empty.".to_string()); }
    Ok(())
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
            let selected: std::collections::HashSet<&str> = stage_paths.iter().map(String::as_str).collect();
            let all_paths: std::collections::HashSet<&str> = preview.changes.iter().map(|change| change.path.as_str()).collect();
            if selected != all_paths {
                return Err("Select every changed path to commit, or choose Stash all changes before pulling. Unselected edits must never be hidden from the pull.".to_string());
            }
            for path in stage_paths { validate_stage_path(path)?; }
            if !stage_paths.is_empty() {
                let mut args = vec!["add", "--"];
                for path in stage_paths { args.push(path.as_str()); }
                run_git(repo_path, &args)
                    .map_err(|error| format!("Could not stage the selected files; pull was not started: {error}"))?;
            }
            run_git(repo_path, &["commit", "-m", message])
                .map_err(|error| format!("Could not create the requested commit; pull was not started: {error}"))?;
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
    let output = if commit_created {
        run_git(repo_path, &["pull", "--rebase", "--no-autostash", remote, branch])
            .map_err(|error| format!("The local commit was created, but rebase/pull did not complete: {error}. Resolve any conflicts, then use Git's rebase continue or abort workflow."))?
    } else {
        run_git(repo_path, &["pull", "--ff-only", remote, branch])
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
    use super::{status_token, validate_relative_path, WorktreeChange};

    #[test]
    fn repository_paths_reject_parent_traversal() {
        assert!(validate_relative_path("../outside").is_err());
        assert!(validate_relative_path("src/../../outside").is_err());
        assert!(validate_relative_path("src/main.rs").is_ok());
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
}