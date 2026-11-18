use git2::{BranchType, ObjectType, Repository};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_PREVIEW_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size_bytes: u64,
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

fn canonical_local_path(
    repo: &Repository,
    relative_path: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let workdir = repo.workdir()
        .ok_or_else(|| "This repository has no working directory.".to_string())?;
    let root = fs::canonicalize(workdir)
        .map_err(|error| format!("Could not resolve repository directory: {error}"))?;
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
    let canonical = fs::canonicalize(&candidate)
        .map_err(|error| format!("Could not open repository path: {error}"))?;
    if !canonical.starts_with(&root) {
        return Err("The requested path resolves outside the repository.".to_string());
    }
    Ok((root, canonical))
}

fn remote_commit<'repo>(
    repo: &'repo Repository,
    branch_name: &str,
) -> Result<git2::Commit<'repo>, String> {
    let branch = repo.find_branch(branch_name, BranchType::Remote)
        .map_err(|error| format!("Could not find remote branch {branch_name}: {error}"))?;
    let target = branch.get().target()
        .ok_or_else(|| "The selected remote branch has no commit.".to_string())?;
    repo.find_commit(target)
        .map_err(|error| format!("Could not load remote branch commit: {error}"))
}

fn remote_tree_for_path<'repo>(
    repo: &'repo Repository,
    branch_name: &str,
    relative_path: &str,
) -> Result<git2::Tree<'repo>, String> {
    let commit = remote_commit(repo, branch_name)?;
    let root = commit.tree()
        .map_err(|error| format!("Could not load remote repository tree: {error}"))?;
    if relative_path.trim().is_empty() {
        return Ok(root);
    }
    let path = validate_browser_relative_path(relative_path)?;
    let entry = root.get_path(&path)
        .map_err(|error| format!("Could not open remote directory: {error}"))?;
    if entry.kind() != Some(ObjectType::Tree) {
        return Err("The selected remote path is not a directory.".to_string());
    }
    entry.to_object(repo)
        .map_err(|error| error.to_string())?
        .into_tree()
        .map_err(|_| "Could not open remote directory tree.".to_string())
}

/// Lists one local working-tree folder or one fetched remote Git tree.
pub fn list_repository_files(
    repo_path: &str,
    relative_path: Option<&str>,
    source: &str,
    branch: Option<&str>,
) -> Result<Vec<RepositoryFileEntry>, String> {
    let repo = Repository::open(repo_path)
        .map_err(|error| format!("Could not open Git repository: {error}"))?;
    let relative = relative_path.unwrap_or("");
    let mut entries = Vec::new();

    if source.eq_ignore_ascii_case("remote") {
        let branch_name = branch.filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Select a remote branch to browse.".to_string())?;
        let tree = remote_tree_for_path(&repo, branch_name, relative)?;
        for entry in tree.iter() {
            let name = entry.name().unwrap_or("").to_string();
            if name.is_empty() || name.eq_ignore_ascii_case(".git") {
                continue;
            }
            let path = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            let is_dir = entry.kind() == Some(ObjectType::Tree);
            let is_symlink = entry.filemode() == 0o120000;
            let size_bytes = if entry.kind() == Some(ObjectType::Blob) {
                repo.find_blob(entry.id()).map(|blob| blob.size() as u64).unwrap_or(0)
            } else {
                0
            };
            if entries.len() >= MAX_DIRECTORY_ENTRIES {
                return Err(format!(
                    "This remote folder contains more than {MAX_DIRECTORY_ENTRIES} items. Open a subfolder to browse a smaller section."
                ));
            }
            entries.push(RepositoryFileEntry { name, path, is_dir, is_symlink, size_bytes });
        }
    } else {
        let (_, directory) = canonical_local_path(&repo, relative)?;
        if !directory.is_dir() {
            return Err("The selected local path is not a directory.".to_string());
        }
        let workdir = repo.workdir()
            .ok_or_else(|| "This repository has no working directory.".to_string())?;
        let root = fs::canonicalize(workdir).map_err(|error| error.to_string())?;
        let read_dir = fs::read_dir(&directory)
            .map_err(|error| format!("Could not list repository directory: {error}"))?;
        for item in read_dir {
            let item = item.map_err(|error| format!("Could not read directory entry: {error}"))?;
            let name = item.file_name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case(".git") {
                continue;
            }
            let file_type = item.file_type()
                .map_err(|error| format!("Could not inspect {name}: {error}"))?;
            let is_symlink = file_type.is_symlink();
            let is_dir = file_type.is_dir();
            let path = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            let size_bytes = if !is_symlink && !is_dir {
                item.metadata().map(|metadata| metadata.len()).unwrap_or(0)
            } else {
                0
            };
            if !is_symlink {
                let canonical = fs::canonicalize(item.path()).map_err(|error| error.to_string())?;
                if !canonical.starts_with(&root) {
                    continue;
                }
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

/// Reads a bounded UTF-8 text file from the local tree or a fetched remote branch.
pub fn read_repository_file(
    repo_path: &str,
    relative_path: &str,
    source: &str,
    branch: Option<&str>,
) -> Result<String, String> {
    let repo = Repository::open(repo_path)
        .map_err(|error| format!("Could not open Git repository: {error}"))?;
    let path = validate_browser_relative_path(relative_path)?;
    if path.as_os_str().is_empty() {
        return Err("Select a file to preview.".to_string());
    }

    let bytes = if source.eq_ignore_ascii_case("remote") {
        let branch_name = branch.filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Select a remote branch to browse.".to_string())?;
        let commit = remote_commit(&repo, branch_name)?;
        let tree = commit.tree().map_err(|error| error.to_string())?;
        let entry = tree.get_path(&path)
            .map_err(|error| format!("Could not open remote file: {error}"))?;
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
    String::from_utf8(bytes)
        .map_err(|_| "This file is not UTF-8 text and cannot be previewed in the text viewer.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_paths_reject_parent_traversal_absolute_paths_and_git_metadata() {
        assert!(validate_relative_path("../outside").is_err());
        assert!(validate_relative_path("/outside").is_err());
        assert!(validate_browser_relative_path(".git/config").is_err());
        assert!(validate_browser_relative_path("src/.GIT/config").is_err());
        assert!(validate_browser_relative_path("src/main.rs").is_ok());
    }
}
