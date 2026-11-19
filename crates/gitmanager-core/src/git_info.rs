use git2::build::CheckoutBuilder;
use git2::{BranchType, Oid, Repository, RepositoryState, Sort, StatusOptions};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::models::{
    BranchInfo,
    CommitDetails,
    CommitHistoryPage,
    CommitNode,
    GitRepositoryInfo,
    RemoteEndpoint,
};


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

/// Extracts full Git metadata including the first, bounded commit-history snapshot.
pub fn extract_git_repository_info(workdir: &Path, git_dir_path: &Path) -> Result<GitRepositoryInfo, String> {
    extract_git_repository_info_with_detail(workdir, git_dir_path, true, true)
}

/// Extracts repository, branch and working-tree metadata without walking commit history.
pub fn extract_git_repository_overview(workdir: &Path, git_dir_path: &Path) -> Result<GitRepositoryInfo, String> {
    extract_git_repository_info_with_detail(workdir, git_dir_path, true, false)
}

/// Extracts only repository-card metadata and HEAD.
pub fn extract_git_repository_summary(workdir: &Path, git_dir_path: &Path) -> Result<GitRepositoryInfo, String> {
    extract_git_repository_info_with_detail(workdir, git_dir_path, false, false)
}

fn extract_git_repository_info_with_detail(
    workdir: &Path,
    git_dir_path: &Path,
    load_details: bool,
    load_commit_history: bool,
) -> Result<GitRepositoryInfo, String> {
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

    let head_reference = repo.head().ok();
    let head_oid = head_reference.as_ref()
        .and_then(|reference| reference.target())
        .map(|oid| oid.to_string());
    let mut current_branch = "HEAD (detached)".to_string();
    if let Some(reference) = head_reference.as_ref() {
        if reference.is_branch() {
            if let Some(short_name) = reference.shorthand() {
                current_branch = short_name.to_string();
            }
        }
    }

    let default_branch = if load_details {
        repo.find_reference("refs/remotes/origin/HEAD").ok()
            .and_then(|reference| reference.symbolic_target().map(str::to_string))
            .and_then(|target| target.strip_prefix("refs/remotes/origin/").map(str::to_string))
    } else { None };

    let mut oid_branches: HashMap<String, Vec<String>> = HashMap::new();
    let mut branches_info: Vec<BranchInfo> = Vec::new();
    if load_details {
        if let Ok(branches) = repo.branches(None) {
            for branch_res in branches.flatten() {
                let (branch, branch_type) = branch_res;
                let branch_name = branch.name().ok().flatten().unwrap_or("unknown").to_string();
                let is_head = branch.is_head();
                let is_remote = matches!(branch_type, BranchType::Remote);
                let target_oid = branch.get().target().map(|id| id.to_string()).unwrap_or_default();
                let upstream = branch.upstream().ok()
                    .and_then(|value| value.name().ok().flatten().map(str::to_string));
                if !target_oid.is_empty() {
                    oid_branches.entry(target_oid.clone()).or_default().push(branch_name.clone());
                }
                let is_default = default_branch.as_ref().map(|name| {
                    branch_name == name.as_str() || branch_name == format!("origin/{name}")
                }).unwrap_or(false);
                branches_info.push(BranchInfo { name: branch_name, is_head, is_remote, target_oid, upstream, is_default });
            }
        }
    }

    let mut commits: Vec<CommitNode> = Vec::new();
    if load_commit_history {
        let mut oid_tags: HashMap<String, Vec<String>> = HashMap::new();
        let _ = repo.tag_foreach(|oid, name_bytes| {
            let raw_name = String::from_utf8_lossy(name_bytes).to_string();
            let clean_tag = raw_name.trim_start_matches("refs/tags/").to_string();
            let resolved_oid = repo.find_tag(oid).ok()
                .map(|tag| tag.target_id().to_string())
                .unwrap_or_else(|| oid.to_string());
            oid_tags.entry(resolved_oid).or_default().push(clean_tag);
            true
        });
        if let Ok(mut revwalk) = repo.revwalk() {
            let _ = revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME);
            if revwalk.push_glob("refs/heads/*").is_err() { let _ = revwalk.push_head(); }
            let _ = revwalk.push_glob("refs/remotes/*");
            let mut active_lanes: Vec<Option<String>> = Vec::new();
            for (index, oid_result) in revwalk.enumerate() {
                if index >= 180 { break; }
                if let Ok(oid) = oid_result {
                    if let Ok(commit) = repo.find_commit(oid) {
                        let oid_string = oid.to_string();
                        let short_oid = oid_string.chars().take(7).collect();
                        let author = commit.author();
                        let author_name = author.name().unwrap_or("Unknown Author").to_string();
                        let author_email = author.email().unwrap_or("unknown@local").to_string();
                        if owner_name.is_empty() { owner_name = author_name.clone(); }
                        if owner_email.is_empty() { owner_email = author_email.clone(); }
                        let parents: Vec<String> = commit.parent_ids().map(|parent| parent.to_string()).collect();
                        let lane = assign_commit_lane(&oid_string, &parents, &mut active_lanes);
                        let seconds = commit.time().seconds();
                        let date_iso = chrono::DateTime::from_timestamp(seconds, 0)
                            .map(|date| date.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| "Unknown date".to_string());
                        commits.push(CommitNode {
                            oid: oid_string.clone(), short_oid,
                            message: commit.message().unwrap_or("").trim().to_string(),
                            summary: commit.summary().unwrap_or("(no commit message)").to_string(),
                            author_name, author_email, timestamp: seconds, date_iso, parents,
                            branches: oid_branches.remove(&oid_string).unwrap_or_default(),
                            tags: oid_tags.remove(&oid_string).unwrap_or_default(),
                            is_head: head_oid.as_deref() == Some(oid_string.as_str()), lane,
                        });
                    }
                }
            }
        }
    } else if let Ok(head_reference) = repo.head() {
        if let Ok(commit) = head_reference.peel_to_commit() {
            let oid = commit.id().to_string();
            let author = commit.author();
            let author_name = author.name().unwrap_or("Unknown Author").to_string();
            let author_email = author.email().unwrap_or("unknown@local").to_string();
            if owner_name.is_empty() { owner_name = author_name.clone(); }
            if owner_email.is_empty() { owner_email = author_email.clone(); }
            let seconds = commit.time().seconds();
            let date_iso = chrono::DateTime::from_timestamp(seconds, 0)
                .map(|date| date.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "Unknown date".to_string());
            let short_oid = oid.chars().take(7).collect();
            commits.push(CommitNode {
                oid, short_oid, message: commit.message().unwrap_or("").trim().to_string(),
                summary: commit.summary().unwrap_or("(no commit message)").to_string(),
                author_name, author_email, timestamp: seconds, date_iso,
                parents: commit.parent_ids().map(|parent| parent.to_string()).collect(),
                branches: if current_branch == "HEAD (detached)" { Vec::new() } else { vec![current_branch.clone()] },
                tags: Vec::new(), is_head: true, lane: 0,
            });
        }
    }

    if owner_name.is_empty() {
        owner_name = "Local Developer".to_string();
    }
    if owner_email.is_empty() {
        owner_email = "local@unconfigured.git".to_string();
    }

    // Avoid recursively enumerating untracked trees during list discovery.
    let mut dirty_files_count = 0;
    if load_details {
        let mut status_opts = StatusOptions::new();
        status_opts.include_untracked(true).recurse_untracked_dirs(false);
        if let Ok(statuses) = repo.statuses(Some(&mut status_opts)) {
            dirty_files_count = statuses.len();
        }
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
        details_loaded: load_details,
    })
}



/// Reads a page of commit history on demand, optionally scoped to one local or
/// remote branch and/or a case-insensitive SHA/message/author search.
pub fn load_repository_commit_history(
    workdir: &Path,
    offset: usize,
    limit: usize,
    branch_filter: Option<&str>,
    search_query: Option<&str>,
) -> Result<CommitHistoryPage, String> {
    const MAX_PAGE_SIZE: usize = 100;
    const MAX_OFFSET: usize = 250_000;

    if limit == 0 || limit > MAX_PAGE_SIZE {
        return Err(format!("Commit history page size must be between 1 and {MAX_PAGE_SIZE}."));
    }
    if offset > MAX_OFFSET {
        return Err(format!("Commit history offset exceeds the {MAX_OFFSET} safety limit."));
    }

    let repo = Repository::open(workdir)
        .map_err(|error| format!("Failed to open Git repository at {}: {error}", workdir.display()))?;
    let normalized_branch = branch_filter
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let normalized_query = search_query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);

    let mut oid_branches: HashMap<String, Vec<String>> = HashMap::new();
    if let Ok(branches) = repo.branches(None) {
        for branch_result in branches.flatten() {
            let (branch, _) = branch_result;
            let name = branch.name().ok().flatten().unwrap_or("unknown").to_string();
            if let Some(oid) = branch.get().target() {
                oid_branches.entry(oid.to_string()).or_default().push(name);
            }
        }
    }

    let mut oid_tags: HashMap<String, Vec<String>> = HashMap::new();
    let _ = repo.tag_foreach(|oid, name_bytes| {
        let raw_name = String::from_utf8_lossy(name_bytes).to_string();
        let clean_tag = raw_name.trim_start_matches("refs/tags/").to_string();
        let resolved_oid = repo.find_tag(oid).ok()
            .map(|tag| tag.target_id().to_string())
            .unwrap_or_else(|| oid.to_string());
        oid_tags.entry(resolved_oid).or_default().push(clean_tag);
        true
    });

    let mut revwalk = repo.revwalk()
        .map_err(|error| format!("Could not create commit history iterator: {error}"))?;
    revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .map_err(|error| format!("Could not configure commit history order: {error}"))?;

    if let Some(branch_name) = normalized_branch {
        let branch = repo.find_branch(branch_name, BranchType::Local)
            .or_else(|_| repo.find_branch(branch_name, BranchType::Remote))
            .map_err(|_| format!("Branch '{branch_name}' does not exist in this repository."))?;
        let reference_name = branch.get().name()
            .ok_or_else(|| format!("Branch '{branch_name}' has no valid Git reference name."))?
            .to_string();
        revwalk.push_ref(&reference_name)
            .map_err(|error| format!("Could not traverse branch '{branch_name}': {error}"))?;
    } else {
        if revwalk.push_glob("refs/heads/*").is_err() {
            if let Ok(head) = repo.head() {
                if let Some(oid) = head.target() {
                    let _ = revwalk.push(oid);
                }
            }
        }
        let _ = revwalk.push_glob("refs/remotes/*");
    }

    let head_oid = repo.head().ok()
        .and_then(|reference| reference.target())
        .map(|oid| oid.to_string());
    let mut active_lanes: Vec<Option<String>> = Vec::new();
    let mut matched = 0usize;
    let mut commits = Vec::with_capacity(limit);
    let mut has_more = false;

    for oid_result in revwalk {
        let Ok(oid) = oid_result else { continue; };
        let Ok(commit) = repo.find_commit(oid) else { continue; };
        let oid_string = oid.to_string();
        let parents: Vec<String> = commit.parent_ids().map(|parent| parent.to_string()).collect();
        // Advance graph lanes even for filtered/skipped commits so page N has
        // the same lane assignments as a full walk from the beginning.
        let lane = assign_commit_lane(&oid_string, &parents, &mut active_lanes);
        let author = commit.author();
        let author_name = author.name().unwrap_or("Unknown Author").to_string();
        let author_email = author.email().unwrap_or("unknown@local").to_string();
        let message = commit.message().unwrap_or("").trim().to_string();
        let summary = commit.summary().unwrap_or("(no commit message)").to_string();
        let short_oid = oid_string.chars().take(7).collect::<String>();

        let matches_query = normalized_query.as_ref().is_none_or(|query| {
            oid_string.to_lowercase().contains(query)
                || short_oid.to_lowercase().contains(query)
                || summary.to_lowercase().contains(query)
                || message.to_lowercase().contains(query)
                || author_name.to_lowercase().contains(query)
                || author_email.to_lowercase().contains(query)
        });
        if !matches_query {
            continue;
        }

        if matched < offset {
            matched += 1;
            continue;
        }
        if commits.len() == limit {
            has_more = true;
            break;
        }

        let seconds = commit.time().seconds();
        let date_iso = chrono::DateTime::from_timestamp(seconds, 0)
            .map(|date| date.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "Unknown date".to_string());
        let mut commit_branches = oid_branches.remove(&oid_string).unwrap_or_default();
        if let Some(branch_name) = normalized_branch {
            if !commit_branches.iter().any(|name| name == branch_name) {
                commit_branches.push(branch_name.to_string());
            }
        }
        commits.push(CommitNode {
            oid: oid_string.clone(),
            short_oid,
            message,
            summary,
            author_name,
            author_email,
            timestamp: seconds,
            date_iso,
            parents,
            branches: commit_branches,
            tags: oid_tags.remove(&oid_string).unwrap_or_default(),
            is_head: head_oid.as_deref() == Some(oid_string.as_str()),
            lane,
        });
        matched += 1;
    }

    Ok(CommitHistoryPage {
        commits,
        offset,
        limit,
        has_more,
    })
}



/// Reads metadata and aggregate diff statistics for a single commit.
pub fn load_commit_details(workdir: &Path, oid_text: &str) -> Result<CommitDetails, String> {
    let repo = Repository::open(workdir)
        .map_err(|error| format!("Could not open Git repository: {error}"))?;
    let oid = Oid::from_str(oid_text)
        .map_err(|_| "The selected commit SHA is invalid.".to_string())?;
    let commit = repo.find_commit(oid)
        .map_err(|error| format!("Could not load selected commit: {error}"))?;
    let tree = commit.tree()
        .map_err(|error| format!("Could not read commit tree: {error}"))?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)
            .and_then(|parent| parent.tree())
            .map_err(|error| format!("Could not read parent commit tree: {error}"))?)
    } else {
        None
    };
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
        .map_err(|error| format!("Could not compare commit trees: {error}"))?;
    let stats = diff.stats()
        .map_err(|error| format!("Could not summarize commit changes: {error}"))?;
    let author = commit.author();
    let authored_at = chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
        .map(|value| value.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown date".to_string());
    let oid_string = oid.to_string();

    Ok(CommitDetails {
        oid: oid_string.clone(),
        short_oid: oid_string.chars().take(7).collect(),
        summary: commit.summary().unwrap_or("(no commit message)").to_string(),
        message: commit.message().unwrap_or("").trim().to_string(),
        author_name: author.name().unwrap_or("Unknown Author").to_string(),
        author_email: author.email().unwrap_or("unknown@local").to_string(),
        authored_at,
        files_changed: stats.files_changed(),
        insertions: stats.insertions(),
        deletions: stats.deletions(),
    })
}

/// Lists local branches without loading commit history or walking the working tree.
pub fn list_local_repository_branches(workdir: &Path) -> Result<Vec<BranchInfo>, String> {
    let repo = Repository::open(workdir)
        .map_err(|error| format!("Failed to open Git repository: {error}"))?;
    let head_name = repo.head().ok()
        .and_then(|reference| reference.shorthand().map(str::to_string));
    let default_branch = repo.find_reference("refs/remotes/origin/HEAD").ok()
        .and_then(|reference| reference.symbolic_target().map(str::to_string))
        .and_then(|target| target.strip_prefix("refs/remotes/origin/").map(str::to_string));
    let branches = repo.branches(Some(BranchType::Local))
        .map_err(|error| format!("Could not list local branches: {error}"))?;
    let mut result = Vec::new();

    for branch_result in branches {
        let (branch, _) = branch_result
            .map_err(|error| format!("Could not read local branch: {error}"))?;
        let name = branch.name()
            .map_err(|error| format!("Could not decode branch name: {error}"))?
            .unwrap_or("unknown")
            .to_string();
        let target_oid = branch.get().target().map(|oid| oid.to_string()).unwrap_or_default();
        let upstream = branch.upstream().ok()
            .and_then(|value| value.name().ok().flatten().map(str::to_string));
        result.push(BranchInfo {
            is_head: head_name.as_deref() == Some(name.as_str()),
            is_remote: false,
            is_default: default_branch.as_deref() == Some(name.as_str()),
            name,
            target_oid,
            upstream,
        });
    }

    result.sort_by(|left, right| {
        right.is_head.cmp(&left.is_head)
            .then_with(|| right.is_default.cmp(&left.is_default))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(result)
}

/// Switches to an existing local branch only when doing so cannot overwrite working-tree changes.
pub fn switch_repository_branch(workdir: &Path, branch_name: &str) -> Result<GitRepositoryInfo, String> {
    let requested = branch_name.trim();
    if requested.is_empty() {
        return Err("Choose a local branch before switching.".to_string());
    }

    let git_dir = {
        let repo = Repository::open(workdir)
            .map_err(|error| format!("Failed to open Git repository: {error}"))?;

        if repo.state() != RepositoryState::Clean {
            return Err("Branch switching is unavailable while a Git operation is in progress. Finish or abort that operation first.".to_string());
        }

        let mut status_options = StatusOptions::new();
        status_options.include_untracked(true).recurse_untracked_dirs(false);
        let statuses = repo.statuses(Some(&mut status_options))
            .map_err(|error| format!("Could not inspect the working tree before switching branches: {error}"))?;
        if !statuses.is_empty() {
            return Err("Branch switching is blocked because this repository has uncommitted or untracked changes. Commit or stash them first.".to_string());
        }

        let branch = repo.find_branch(requested, BranchType::Local)
            .map_err(|_| format!("Local branch '{requested}' does not exist. Refresh the branch list and try again."))?;
        let reference_name = branch.get().name()
            .ok_or_else(|| "The selected branch has an invalid reference name.".to_string())?
            .to_string();
        let target_commit = branch.get().peel_to_commit()
            .map_err(|error| format!("Local branch '{requested}' does not point to a commit: {error}"))?;
        let current_reference = repo.head().ok()
            .and_then(|reference| reference.name().map(str::to_string));

        if current_reference.as_deref() != Some(reference_name.as_str()) {
            let mut checkout = CheckoutBuilder::new();
            checkout.safe();
            repo.checkout_tree(target_commit.as_object(), Some(&mut checkout))
                .map_err(|error| format!("Could not safely check out '{requested}': {error}"))?;
            repo.set_head(&reference_name)
                .map_err(|error| format!("Files were checked out, but HEAD could not be moved to '{requested}': {error}"))?;
        }

        repo.path().to_path_buf()
    };

    extract_git_repository_overview(workdir, &git_dir)
        .map_err(|error| format!("Branch switched, but repository details could not be refreshed: {error}"))
}

#[cfg(test)]
mod branch_switch_tests {
    use super::*;
    use git2::Signature;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn repository_fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gitmanager-branch-switch-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();

        let repo = Repository::init(&path).unwrap();
        let signature = Signature::now("GitManager Test", "gitmanager-test@invalid.local").unwrap();

        fs::write(path.join("tracked.txt"), "main\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        let tree_id = index.write_tree().unwrap();
        drop(index);
        let tree = repo.find_tree(tree_id).unwrap();
        let main_oid = repo.commit(
            Some("refs/heads/main"),
            &signature,
            &signature,
            "initial commit",
            &tree,
            &[],
        ).unwrap();
        drop(tree);

        repo.set_head("refs/heads/main").unwrap();
        let main_commit = repo.find_commit(main_oid).unwrap();
        repo.branch("feature", &main_commit, false).unwrap();

        repo.set_head("refs/heads/feature").unwrap();
        repo.checkout_head(None).unwrap();
        fs::write(path.join("tracked.txt"), "feature\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        let tree_id = index.write_tree().unwrap();
        drop(index);
        let tree = repo.find_tree(tree_id).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "feature commit",
            &tree,
            &[&parent],
        ).unwrap();
        drop(tree);
        drop(parent);

        repo.set_head("refs/heads/main").unwrap();
        let mut checkout = CheckoutBuilder::new();
        checkout.force();
        repo.checkout_head(Some(&mut checkout)).unwrap();
        drop(main_commit);
        drop(repo);
        path
    }

    #[test]
    fn branch_list_places_the_current_local_branch_first() {
        let path = repository_fixture();
        let branches = list_local_repository_branches(&path).unwrap();
        assert_eq!(branches.first().map(|branch| branch.name.as_str()), Some("main"));
        assert!(branches.iter().any(|branch| branch.name == "feature" && !branch.is_remote));
        assert!(branches.first().map(|branch| branch.is_head).unwrap_or(false));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn clean_worktree_can_switch_to_another_local_branch() {
        let path = repository_fixture();
        let updated = switch_repository_branch(&path, "feature").unwrap();
        assert_eq!(updated.current_branch, "feature");
        assert_eq!(fs::read_to_string(path.join("tracked.txt")).unwrap(), "feature\n");
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn dirty_worktree_is_preserved_and_branch_switch_is_rejected() {
        let path = repository_fixture();
        fs::write(path.join("untracked.txt"), "keep this file\n").unwrap();
        let error = switch_repository_branch(&path, "feature").unwrap_err();
        assert!(error.contains("uncommitted or untracked changes"));
        let repo = Repository::open(&path).unwrap();
        assert_eq!(repo.head().unwrap().shorthand(), Some("main"));
        assert_eq!(fs::read_to_string(path.join("untracked.txt")).unwrap(), "keep this file\n");
        drop(repo);
        fs::remove_dir_all(path).unwrap();
    }
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


#[cfg(test)]
mod commit_history_tests {
    use super::*;
    use git2::Signature;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn repository_with_commits(count: usize) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gitmanager-history-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();

        let repo = Repository::init(&path).unwrap();
        let signature = Signature::now("GitManager Test", "gitmanager-test@invalid.local").unwrap();
        let mut previous_oid = None;

        for index in 0..count {
            fs::write(path.join("history.txt"), format!("commit {index}\n")).unwrap();
            let mut git_index = repo.index().unwrap();
            git_index.add_path(Path::new("history.txt")).unwrap();
            let tree_id = git_index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let message = if index == 125 {
                format!("needle commit {index}")
            } else {
                format!("commit {index}")
            };

            let oid = if let Some(parent_oid) = previous_oid {
                let parent = repo.find_commit(parent_oid).unwrap();
                let oid = repo.commit(
                    Some("refs/heads/main"),
                    &signature,
                    &signature,
                    &message,
                    &tree,
                    &[&parent],
                ).unwrap();
                drop(parent);
                oid
            } else {
                repo.commit(
                    Some("refs/heads/main"),
                    &signature,
                    &signature,
                    &message,
                    &tree,
                    &[],
                ).unwrap()
            };
            previous_oid = Some(oid);
            drop(tree);
            drop(git_index);
        }

        repo.set_head("refs/heads/main").unwrap();
        assert_eq!(repo.head().unwrap().target(), previous_oid);
        drop(repo);
        path
    }

    #[test]
    fn commit_history_pages_include_commits_beyond_the_legacy_180_limit() {
        let path = repository_with_commits(205);

        let first = load_repository_commit_history(&path, 0, 100, None, None).unwrap();
        let second = load_repository_commit_history(&path, 100, 100, None, None).unwrap();
        let third = load_repository_commit_history(&path, 200, 100, None, None).unwrap();

        assert_eq!(first.commits.len(), 100);
        assert_eq!(second.commits.len(), 100);
        assert_eq!(third.commits.len(), 5);
        assert!(first.has_more);
        assert!(second.has_more);
        assert!(!third.has_more);
        assert_eq!(first.commits[0].summary, "commit 204");
        assert_eq!(third.commits.last().map(|commit| commit.summary.as_str()), Some("commit 0"));

        let overview = extract_git_repository_overview(&path, &path.join(".git")).unwrap();
        assert!(overview.details_loaded);
        assert_eq!(overview.commits.len(), 1);
        assert_eq!(overview.commits[0].summary, "commit 204");
        assert!(overview.branches.iter().any(|branch| branch.name == "main"));

        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn commit_history_search_and_branch_filters_apply_before_pagination() {
        let path = repository_with_commits(205);

        let page = load_repository_commit_history(
            &path,
            0,
            20,
            Some("main"),
            Some("NEEDLE"),
        ).unwrap();

        assert_eq!(page.commits.len(), 1);
        assert!(!page.has_more);
        assert_eq!(page.commits[0].summary, "needle commit 125");
        assert!(page.commits[0].branches.iter().any(|name| name == "main"));

        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn commit_history_rejects_unbounded_page_requests() {
        let path = repository_with_commits(1);
        assert!(load_repository_commit_history(&path, 0, 101, None, None).is_err());
        assert!(load_repository_commit_history(&path, 250_001, 20, None, None).is_err());
        assert!(load_repository_commit_history(&path, 0, 0, None, None).is_err());
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn commit_details_are_loaded_from_the_shared_core() {
        let path = repository_with_commits(1);
        let repo = Repository::open(&path).unwrap();
        let oid = repo.head().unwrap().target().unwrap().to_string();
        drop(repo);

        let details = load_commit_details(&path, &oid).unwrap();
        assert_eq!(details.oid, oid);
        assert_eq!(details.summary, "commit 0");
        assert_eq!(details.files_changed, 1);
        assert_eq!(details.insertions, 1);
        assert_eq!(details.deletions, 0);

        let _ = fs::remove_dir_all(path);
    }
}
