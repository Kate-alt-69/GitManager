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
    pub index_dependency_repositories: Option<bool>,
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
    let index_dependencies = options.index_dependency_repositories.unwrap_or(true);

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

    // These folders are never useful scan roots; project dependency folders are
    // handled separately so nested Git repositories can still be indexed.
    let ignored_names: HashSet<&str> = [
        "appdata",
        "library",
        "system volume information",
        "$recycle.bin",
        "windows",
        "program files",
        "program files (x86)",
        "programdata",
        "recovery",
        "proc",
        "sys",
        "dev",
        "volumes",
    ]
    .into_iter()
    .collect();

    let dependency_names: HashSet<&str> = [
        "node_modules",
        "target",
        "vendor",
        ".cargo",
        ".rustup",
        ".npm",
        ".pnpm-store",
        ".yarn",
        "venv",
        ".venv",
        "env",
        "site-packages",
        "dist-packages",
        "__pycache__",
        ".gradle",
        ".nuget",
        "bower_components",
        "Pods",
        "dist",
        "build",
        "bin",
        "obj",
        "third_party",
        "third-party",
    ]
    .into_iter()
    .collect();

    const MAX_DEPENDENCY_DESCENT: usize = 2;

    for root in roots {
        let walker = WalkDir::new(&root)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                let relative = match entry.path().strip_prefix(&root) {
                    Ok(path) => path,
                    Err(_) => return false,
                };
                let components: Vec<String> = relative
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
                    .collect();

                // Yield the .git marker itself but never walk through its object database.
                if components.iter().take(components.len().saturating_sub(1)).any(|part| part == ".git") {
                    return false;
                }
                if name == ".git" {
                    return true;
                }
                let lower_name = name.to_lowercase();
                if ignored_names.contains(lower_name.as_str()) {
                    return false;
                }

                let dependency_index = components
                    .iter()
                    .position(|part| dependency_names.contains(part.as_str()));
                if let Some(index) = dependency_index {
                    if !index_dependencies {
                        return false;
                    }
                    let depth_inside_dependency = components.len().saturating_sub(index + 1);
                    if depth_inside_dependency > MAX_DEPENDENCY_DESCENT {
                        return false;
                    }
                }

                let inside_dependency = dependency_index.is_some();
                if !include_hidden
                    && name.starts_with('.')
                    && entry.depth() > 0
                    && !inside_dependency
                    && !dependency_names.contains(name.as_ref())
                {
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

const DEPENDENCY_PATH_NAMES: &[&str] = &[
    "node_modules", "target", "vendor", ".cargo", ".rustup", ".npm",
    ".pnpm-store", ".yarn", "venv", ".venv", "env", "site-packages",
    "dist-packages", "__pycache__", ".gradle", ".nuget", "bower_components",
    "pods", "dist", "build", "bin", "obj", "third_party", "third-party",
];

fn is_dependency_repository_path(path: &str) -> bool {
    Path::new(path).components().any(|component| {
        let part = component.as_os_str().to_string_lossy().to_lowercase();
        DEPENDENCY_PATH_NAMES.contains(&part.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::is_dependency_repository_path;
    use std::path::MAIN_SEPARATOR;

    #[test]
    fn classifies_known_dependency_directories() {
        let path = format!("projects{0}app{0}node_modules{0}some-package", MAIN_SEPARATOR);
        assert!(is_dependency_repository_path(&path));
    }

    #[test]
    fn does_not_classify_similarly_named_project_directories() {
        let path = format!("projects{0}targeting-tool{0}src", MAIN_SEPARATOR);
        assert!(!is_dependency_repository_path(&path));
    }
}

/// Groups indexed repositories by parent directory and computes summary counts for Remote vs Local
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedRepoSummary {
    pub repositories: Vec<GitRepositoryInfo>,
    pub by_folder: HashMap<String, Vec<String>>,
    pub remote_count: usize,
    pub local_only_count: usize,
    pub dependency_count: usize,
    pub dependency_repo_ids: Vec<String>,
    pub effective_filter_mode: String,
}

pub fn build_indexed_summary(repos: Vec<GitRepositoryInfo>, requested_filter: &str) -> IndexedRepoSummary {
    let remote_count = repos.iter().filter(|r| r.has_remote).count();
    let local_only_count = repos.iter().filter(|r| !r.has_remote).count();

    let mut dependency_repo_ids = Vec::new();
    for repo in &repos {
        if is_dependency_repository_path(&repo.path) {
            dependency_repo_ids.push(repo.id.clone());
        }
    }

    let dependency_count = dependency_repo_ids.len();
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
        dependency_count,
        dependency_repo_ids,
        effective_filter_mode,
    }
}
