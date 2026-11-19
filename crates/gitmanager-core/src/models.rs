//! Serializable data contracts shared by the Tauri UI and the required service.
//!
//! These types intentionally have no libgit2 or filesystem implementation
//! dependencies so the desktop coordinator can deserialize service responses
//! without linking the repository engine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitNode {
    pub oid: String,
    pub short_oid: String,
    pub message: String,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: i64,
    pub date_iso: String,
    pub parents: Vec<String>,
    pub branches: Vec<String>,
    pub tags: Vec<String>,
    pub is_head: bool,
    pub lane: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub target_oid: String,
    pub upstream: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEndpoint {
    pub name: String,
    pub url: String,
    pub provider: String,
    pub host: String,
    pub owner: Option<String>,
    pub repo_slug: Option<String>,
    pub web_url: Option<String>,
    pub icon_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetails {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_at: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRepositoryInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub git_dir: String,
    pub parent_folder: String,
    pub owner_name: String,
    pub owner_email: String,
    pub current_branch: String,
    pub has_remote: bool,
    pub primary_provider: String,
    pub primary_remote_url: Option<String>,
    pub web_url: Option<String>,
    pub icon_url: String,
    pub remotes: Vec<RemoteEndpoint>,
    pub branches: Vec<BranchInfo>,
    pub commits: Vec<CommitNode>,
    pub dirty_files_count: usize,
    pub total_commits_loaded: usize,
    pub source_kind: String,
    #[serde(default)]
    pub details_loaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitHistoryPage {
    pub commits: Vec<CommitNode>,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChange {
    pub path: String,
    pub index_status: String,
    pub worktree_status: String,
    pub staged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullExecutionResult {
    pub message: String,
    pub commit_created: bool,
    pub stash_created: bool,
    pub rebased: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryCommitPreview {
    pub changes: Vec<WorktreeChange>,
    pub index_tree: String,
}
