use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::git_info::{extract_git_repository_info, GitRepositoryInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub root_paths: Vec<String>,
    pub max_depth: Option<usize>,
    pub include_hidden_dirs: Option<bool>,
}

/// Default common developer directories across Windows 10/11, Linux, and macOS
pub fn default_scan_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        let candidates = [
            "Projects",
            "Repos",
            "Repositories",
            "Development",
            "dev",
            "workspace",
            "src",
            "code",
            "GitHub",
            "GitLab",
            "source/repos", // Visual Studio default on Windows 10/11
            "Documents/GitHub",
            "Documents/Projects",
            "Desktop",
        ];
        for rel in candidates {
            let p = home.join(rel);
            if p.exists() && p.is_dir() {
                roots.push(p);
            }
        }
        if roots.is_empty() {
            roots.push(home);
        }
    }
    roots
}

/// Scans the specified directories (or system defaults) for `.git` folders or git worktrees
/// and indexes all discovered repositories by their parent folder and `.git` metadata.
pub fn scan_repositories(options: ScanOptions) -> Result<Vec<GitRepositoryInfo>, String> {
    let max_depth = options.max_depth.unwrap_or(6);
    let include_hidden = options.include_hidden_dirs.unwrap_or(false);

    let roots: Vec<PathBuf> = if options.root_paths.is_empty() {
        default_scan_roots()
    } else {
        options
            .root_paths
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect()
    };

    let mut discovered_workdirs: HashSet<PathBuf> = HashSet::new();
    let mut repositories: Vec<GitRepositoryInfo> = Vec::new();

    // Ignored heavy/system folders across Windows, macOS, and Linux
    let ignored_names: HashSet<&str> = [
        "node_modules",
        "target",
        "dist",
        "build",
        "vendor",
        ".cargo",
        ".rustup",
        ".npm",
        ".cache",
        "AppData",
        "Library",
        "System Volume Information",
        "$RECYCLE.BIN",
        "venv",
        ".venv",
        "__pycache__",
    ]
    .into_iter()
    .collect();

    for root in roots {
        let walker = WalkDir::new(&root)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                if name == ".git" {
                    return true;
                }
                if ignored_names.contains(name.as_ref()) {
                    return false;
                }
                if !include_hidden && name.starts_with('.') && entry.depth() > 0 {
                    return false;
                }
                true
            });

        for entry in walker.flatten() {
            let file_name = entry.file_name().to_string_lossy();
            if file_name == ".git" {
                let git_path = entry.path();
                if let Some(repo_root) = git_path.parent() {
                    let canonical = fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
                    if discovered_workdirs.insert(canonical.clone()) {
                        if let Ok(info) = extract_git_repository_info(&canonical, git_path) {
                            repositories.push(info);
                        }
                    }
                }
            }
        }
    }

    // Sort alphabetically by folder category then repository name
    repositories.sort_by(|a, b| {
        a.parent_folder
            .to_lowercase()
            .cmp(&b.parent_folder.to_lowercase())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(repositories)
}

/// Groups indexed repositories by parent directory and computes summary counts for Remote vs Local
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedRepoSummary {
    pub repositories: Vec<GitRepositoryInfo>,
    pub by_folder: HashMap<String, Vec<String>>,
    pub remote_count: usize,
    pub local_only_count: usize,
    pub effective_filter_mode: String,
}

pub fn build_indexed_summary(repos: Vec<GitRepositoryInfo>, requested_filter: &str) -> IndexedRepoSummary {
    let remote_count = repos.iter().filter(|r| r.has_remote).count();
    let local_only_count = repos.iter().filter(|r| !r.has_remote).count();

    let mut by_folder: HashMap<String, Vec<String>> = HashMap::new();
    for repo in &repos {
        by_folder
            .entry(repo.parent_folder.clone())
            .or_default()
            .push(repo.id.clone());
    }

    // Rule requested by user:
    // Can be filtered by Remote (available) and Local (no remote, available if exists,
    // but if one side doesn't exist / isn't available, automatically group both into 1 unified group).
    let effective_filter_mode = match requested_filter {
        "remote" if remote_count > 0 => "remote".to_string(),
        "local" if local_only_count > 0 => "local".to_string(),
        "remote" | "local" | "grouped" | _ => {
            if remote_count == 0 || local_only_count == 0 {
                "grouped_fallback".to_string()
            } else if requested_filter == "remote" || requested_filter == "local" {
                requested_filter.to_string()
            } else {
                "grouped".to_string()
            }
        }
    };

    IndexedRepoSummary {
        repositories: repos,
        by_folder,
        remote_count,
        local_only_count,
        effective_filter_mode,
    }
}
