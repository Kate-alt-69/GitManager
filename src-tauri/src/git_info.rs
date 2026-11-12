use git2::{BranchType, Repository, Sort, StatusOptions};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEndpoint {
    pub name: String,
    pub url: String,
    pub provider: String, // "github", "gitlab", "custom", "none"
    pub host: String,
    pub owner: Option<String>,
    pub repo_slug: Option<String>,
    pub web_url: Option<String>,
    pub icon_url: String,
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
    pub primary_provider: String, // "github" | "gitlab" | "custom" | "local"
    pub primary_remote_url: Option<String>,
    pub web_url: Option<String>,
    pub icon_url: String,
    pub remotes: Vec<RemoteEndpoint>,
    pub branches: Vec<BranchInfo>,
    pub commits: Vec<CommitNode>,
    pub dirty_files_count: usize,
    pub total_commits_loaded: usize,
    pub source_kind: String, // "local_scan" | "remote_account" | "synced"
}

/// Parses a Git remote URL (SSH or HTTPS) to extract host, provider, owner, repo slug, and avatar/icon URL
pub fn parse_remote_url(remote_name: &str, raw_url: &str) -> RemoteEndpoint {
    let trimmed = raw_url.trim().trim_end_matches(".git").trim_end_matches('/');
    let mut host = String::new();
    let mut path_part = String::new();

    if let Some(rest) = trimmed.strip_prefix("git@") {
        // SSH syntax: git@github.com:owner/repo
        if let Some((h, p)) = rest.split_once(':') {
            host = h.to_lowercase();
            path_part = p.trim_start_matches('/').to_string();
        }
    } else if let Some( idx ) = trimmed.find("://") {
        // HTTPS / SSH URL syntax: https://github.com/owner/repo
        let after_scheme = &trimmed[idx + 3..];
        let after_auth = if let Some((_, host_and_path)) = after_scheme.split_once('@') {
            host_and_path
        } else {
            after_scheme
        };
        if let Some((h, p)) = after_auth.split_once('/') {
            host = h.to_lowercase();
            path_part = p.trim_start_matches('/').to_string();
        } else {
            host = after_auth.to_lowercase();
        }
    }

    let segments: Vec<&str> = path_part.split('/').filter(|s| !s.is_empty()).collect();
    let owner = segments.first().map(|s| s.to_string());
    let repo_slug = segments.last().map(|s| s.to_string());

    let provider = if host.contains("github.com") || host.contains("github") {
        "github".to_string()
    } else if host.contains("gitlab.com") || host.contains("gitlab") {
        "gitlab".to_string()
    } else if !host.is_empty() {
        "custom".to_string()
    } else {
        "none".to_string()
    };

    // Resolve repository/owner icon via github.com or the git remote host
    let icon_url = match (provider.as_str(), owner.as_deref()) {
        ("github", Some(o)) => format!("https://github.com/{}.png?size=128", o),
        ("gitlab", Some(o)) => format!("https://gitlab.com/{}.png?size=128", o),
        (_, Some(_)) if !host.is_empty() => format!("https://{}/favicon.ico", host),
        _ => "local://git-icon".to_string(),
    };

    let web_url = if !host.is_empty() && !path_part.is_empty() {
        Some(format!("https://{}/{}", host, path_part))
    } else {
        None
    };

    RemoteEndpoint {
        name: remote_name.to_string(),
        url: raw_url.to_string(),
        provider,
        host,
        owner,
        repo_slug,
        web_url,
        icon_url,
    }
}

/// Extracts complete Git metadata, `.git/config` details, owner/email, remotes, branches, and full Git commit tree
pub fn extract_git_repository_info(workdir: &Path, git_dir_path: &Path) -> Result<GitRepositoryInfo, String> {
    let repo = Repository::open(workdir).map_err(|e| format!("Failed to open git repo at {:?}: {}", workdir, e))?;

    let name = workdir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed-repo".to_string());

    let parent_folder = workdir
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());

    // Read git config for user.name and user.email
    let mut owner_name = String::new();
    let mut owner_email = String::new();
    if let Ok(cfg) = repo.config() {
        if let Ok(n) = cfg.get_string("user.name") {
            owner_name = n;
        }
        if let Ok(e) = cfg.get_string("user.email") {
            owner_email = e;
        }
    }

    // Extract remotes from .git/config
    let mut remotes = Vec::new();
    if let Ok(remote_names) = repo.remotes() {
        for r_name in remote_names.iter().flatten() {
            if let Ok(remote) = repo.find_remote(r_name) {
                if let Some(url) = remote.url() {
                    remotes.push(parse_remote_url(r_name, url));
                }
            }
        }
    }

    // Prefer "origin" or first available remote
    let primary_remote = remotes
        .iter()
        .find(|r| r.name == "origin")
        .or_else(|| remotes.first())
        .cloned();

    // If git config didn't have local user.name, fall back to remote owner or most recent commit author
    if owner_name.is_empty() {
        if let Some(ref pr) = primary_remote {
            if let Some(ref o) = pr.owner {
                owner_name = o.clone();
            }
        }
    }

    // Map OIDs to branch names & tag names for the full Git tree
    let mut oid_branches: HashMap<String, Vec<String>> = HashMap::new();
    let mut branches_info: Vec<BranchInfo> = Vec::new();
    let mut current_branch = "HEAD (detached)".to_string();
    let head_oid = repo.head().ok().and_then(|h| h.target()).map(|id| id.to_string());

    if let Ok(head_ref) = repo.head() {
        if head_ref.is_branch() {
            if let Some(short) = head_ref.shorthand() {
                current_branch = short.to_string();
            }
        }
    }

    if let Ok(branches) = repo.branches(None) {
        for branch_res in branches.flatten() {
            let (branch, b_type) = branch_res;
            let b_name = branch
                .name()
                .ok()
                .flatten()
                .unwrap_or("unknown")
                .to_string();
            let is_head = branch.is_head();
            let is_remote = matches!(b_type, BranchType::Remote);
            let target_oid = branch
                .get()
                .target()
                .map(|id| id.to_string())
                .unwrap_or_default();
            let upstream = branch
                .upstream()
                .ok()
                .and_then(|u| u.name().ok().flatten().map(|s| s.to_string()));

            if !target_oid.is_empty() {
                oid_branches
                    .entry(target_oid.clone())
                    .or_default()
                    .push(b_name.clone());
            }

            branches_info.push(BranchInfo {
                name: b_name,
                is_head,
                is_remote,
                target_oid,
                upstream,
            });
        }
    }

    let mut oid_tags: HashMap<String, Vec<String>> = HashMap::new();
    let _ = repo.tag_foreach(|oid, name_bytes| {
        let raw_name = String::from_utf8_lossy(name_bytes).to_string();
        let clean_tag = raw_name.trim_start_matches("refs/tags/").to_string();
        let resolved_oid = repo
            .find_tag(oid)
            .ok()
            .map(|t| t.target_id().to_string())
            .unwrap_or_else(|| oid.to_string());
        oid_tags.entry(resolved_oid).or_default().push(clean_tag);
        true
    });

    // Walk commit graph across all refs to build the visual Git Tree
    let mut commits: Vec<CommitNode> = Vec::new();
    if let Ok(mut revwalk) = repo.revwalk() {
        let _ = revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME);
        if revwalk.push_glob("refs/heads/*").is_err() {
            let _ = revwalk.push_head();
        }
        let _ = revwalk.push_glob("refs/remotes/*");

        let mut active_lanes: Vec<Option<String>> = Vec::new();

        for (idx, oid_res) in revwalk.enumerate() {
            if idx >= 180 {
                break; // Cap per-repo initial tree nodes for snappy UI rendering
            }
            if let Ok(oid) = oid_res {
                if let Ok(commit) = repo.find_commit(oid) {
                    let oid_str = oid.to_string();
                    let short_oid = oid_str.chars().take(7).collect();
                    let author = commit.author();
                    let a_name = author.name().unwrap_or("Unknown Author").to_string();
                    let a_email = author.email().unwrap_or("unknown@local").to_string();

                    if owner_name.is_empty() {
                        owner_name = a_name.clone();
                    }
                    if owner_email.is_empty() {
                        owner_email = a_email.clone();
                    }

                    let parents: Vec<String> = commit.parent_ids().map(|p| p.to_string()).collect();
                    let lane = assign_commit_lane(&oid_str, &parents, &mut active_lanes);

                    let secs = commit.time().seconds();
                    let date_iso = chrono::DateTime::from_timestamp(secs, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "Unknown date".to_string());

                    commits.push(CommitNode {
                        oid: oid_str.clone(),
                        short_oid,
                        message: commit.message().unwrap_or("").trim().to_string(),
                        summary: commit.summary().unwrap_or("(no commit message)").to_string(),
                        author_name: a_name,
                        author_email: a_email,
                        timestamp: secs,
                        date_iso,
                        parents,
                        branches: oid_branches.remove(&oid_str).unwrap_or_default(),
                        tags: oid_tags.remove(&oid_str).unwrap_or_default(),
                        is_head: head_oid.as_deref() == Some(oid_str.as_str()),
                        lane,
                    });
                }
            }
        }
    }

    if owner_name.is_empty() {
        owner_name = "Local Developer".to_string();
    }
    if owner_email.is_empty() {
        owner_email = "local@unconfigured.git".to_string();
    }

    // Count modified/untracked working tree files
    let mut dirty_files_count = 0;
    let mut status_opts = StatusOptions::new();
    status_opts.include_untracked(true);
    if let Ok(statuses) = repo.statuses(Some(&mut status_opts)) {
        dirty_files_count = statuses.len();
    }

    let has_remote = !remotes.is_empty();
    let primary_provider = primary_remote
        .as_ref()
        .map(|r| r.provider.clone())
        .unwrap_or_else(|| "local".to_string());
    let primary_remote_url = primary_remote.as_ref().map(|r| r.url.clone());
    let web_url = primary_remote.as_ref().and_then(|r| r.web_url.clone());
    let icon_url = primary_remote
        .as_ref()
        .map(|r| r.icon_url.clone())
        .unwrap_or_else(|| format!("https://github.com/{}.png?size=128", owner_name.replace(' ', "")));

    let canonical_workdir = fs::canonicalize(workdir)
        .unwrap_or_else(|_| workdir.to_path_buf())
        .to_string_lossy()
        .to_string();

    Ok(GitRepositoryInfo {
        id: format!("local::{}", canonical_workdir),
        name,
        path: canonical_workdir,
        git_dir: git_dir_path.to_string_lossy().to_string(),
        parent_folder,
        owner_name,
        owner_email,
        current_branch,
        has_remote,
        primary_provider,
        primary_remote_url,
        web_url,
        icon_url,
        remotes,
        branches: branches_info,
        total_commits_loaded: commits.len(),
        commits,
        dirty_files_count,
        source_kind: "local_scan".to_string(),
    })
}

/// Assigns a visual swimlane index (0..N) for each commit node in the Git graph tree
fn assign_commit_lane(
    oid: &str,
    parents: &[String],
    active_lanes: &mut Vec<Option<String>>,
) -> usize {
    let lane = if let Some(pos) = active_lanes
        .iter()
        .position(|slot| slot.as_deref() == Some(oid))
    {
        pos
    } else if let Some(empty_pos) = active_lanes.iter().position(|slot| slot.is_none()) {
        active_lanes[empty_pos] = Some(oid.to_string());
        empty_pos
    } else {
        active_lanes.push(Some(oid.to_string()));
        active_lanes.len() - 1
    };

    // Deduplicate any other lanes pointing to the same merged commit
    for (idx, slot) in active_lanes.iter_mut().enumerate() {
        if idx != lane && slot.as_deref() == Some(oid) {
            *slot = None;
        }
    }

    if let Some(first_parent) = parents.first() {
        active_lanes[lane] = Some(first_parent.clone());
        for extra_parent in parents.iter().skip(1) {
            let already_tracked = active_lanes
                .iter()
                .any(|s| s.as_deref() == Some(extra_parent.as_str()));
            if !already_tracked {
                if let Some(free) = active_lanes.iter().position(|s| s.is_none()) {
                    active_lanes[free] = Some(extra_parent.clone());
                } else {
                    active_lanes.push(Some(extra_parent.clone()));
                }
            }
        }
    } else {
        active_lanes[lane] = None;
    }

    lane
}
