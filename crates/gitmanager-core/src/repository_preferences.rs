//! Per-repository settings stored in the repository's Git info directory.
//!
//! Tauri validates user input and forwards settings operations. The core
//! service owns the Git repository lookup and all persistent reads/writes.

use git2::Repository;
use serde_json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::app_preferences::{load_background_preferences, RepositoryPreferences};

const MAX_REPOSITORY_SETTINGS_BYTES: u64 = 64 * 1024;
const MAX_EXCLUDE_PATTERNS: usize = 256;

fn repository_settings_path(repo_path: &str) -> Result<PathBuf, String> {
    if repo_path.trim().is_empty() {
        return Err("Select a local Git repository first.".to_string());
    }
    let repo = Repository::open(repo_path)
        .map_err(|error| format!("Could not open Git repository: {error}"))?;
    if repo.is_bare() {
        return Err("Repository-local settings require a non-bare working tree.".to_string());
    }
    let info_dir = repo.path().join("info");
    fs::create_dir_all(&info_dir)
        .map_err(|error| format!("Could not create repository-local Git info directory: {error}"))?;
    Ok(info_dir.join("gitmanager-settings.json"))
}

fn recover_previous_settings(path: &Path) -> Result<(), String> {
    if path.is_file() {
        return Ok(());
    }
    let backup = path.with_extension("json.bak");
    if backup.is_file() {
        fs::rename(&backup, path)
            .map_err(|error| format!("Could not recover previous repository settings: {error}"))?;
    }
    Ok(())
}

fn read_settings_file(path: &Path) -> Result<Option<RepositoryPreferences>, String> {
    recover_previous_settings(path)?;
    if !path.is_file() {
        return Ok(None);
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not inspect repository-local settings: {error}"))?;
    if metadata.len() > MAX_REPOSITORY_SETTINGS_BYTES {
        return Err("Repository settings exceed the 64 KiB safety limit.".to_string());
    }
    let content = fs::read(path)
        .map_err(|error| format!("Could not read repository-local settings: {error}"))?;
    let settings = serde_json::from_slice(&content)
        .map_err(|error| format!("Repository-local settings are invalid JSON: {error}"))?;
    Ok(Some(settings))
}

fn normalize_settings(settings: RepositoryPreferences) -> Result<RepositoryPreferences, String> {
    if settings.cleaner_exclude_patterns.len() > MAX_EXCLUDE_PATTERNS {
        return Err("A repository can have at most 256 cleaner exclusion patterns.".to_string());
    }
    let mut normalized = settings;
    normalized.cleaner_exclude_patterns = normalized.cleaner_exclude_patterns
        .into_iter()
        .map(|value| value.trim().replace('\\', "/"))
        .filter(|value| {
            !value.is_empty()
                && !value.starts_with('/')
                && !value.split('/').any(|part| part == "..")
        })
        .take(MAX_EXCLUDE_PATTERNS)
        .collect();
    Ok(normalized)
}

fn write_settings_file(path: &Path, settings: &RepositoryPreferences) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("Could not serialize repository settings: {error}"))?;
    if bytes.len() as u64 > MAX_REPOSITORY_SETTINGS_BYTES {
        return Err("Repository settings exceed the 64 KiB safety limit.".to_string());
    }
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    {
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Could not create temporary repository settings: {error}"))?;
        output.write_all(&bytes)
            .map_err(|error| format!("Could not write temporary repository settings: {error}"))?;
        output.sync_all()
            .map_err(|error| format!("Could not flush temporary repository settings: {error}"))?;
    }
    let had_previous = path.is_file();
    if had_previous {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Could not clear repository settings backup: {error}"))?;
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("Could not stage previous repository settings: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_previous {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("Could not save repository settings: {error}"));
    }
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    Ok(())
}

/// Loads local settings, migrating the prior global-settings entry when needed.
pub fn load_repository_preferences(
    repo_path: &str,
    repo_id: Option<&str>,
) -> Result<RepositoryPreferences, String> {
    let path = repository_settings_path(repo_path)?;
    if let Some(settings) = read_settings_file(&path)? {
        return Ok(settings);
    }

    let legacy = match repo_id {
        Some(id) if !id.trim().is_empty() => {
            load_background_preferences()?.repository_settings.get(id).cloned()
        }
        _ => None,
    };
    let settings = normalize_settings(legacy.unwrap_or_default())?;
    if settings != RepositoryPreferences::default() {
        write_settings_file(&path, &settings)?;
    }
    Ok(settings)
}

/// Saves repository-specific cleaner and update preferences beside the Git repo.
pub fn save_repository_preferences(
    repo_path: &str,
    settings: RepositoryPreferences,
) -> Result<RepositoryPreferences, String> {
    let normalized = normalize_settings(settings)?;
    let path = repository_settings_path(repo_path)?;
    write_settings_file(&path, &normalized)?;
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::normalize_settings;
    use crate::app_preferences::RepositoryPreferences;

    #[test]
    fn repository_exclude_patterns_are_normalized_and_traversal_is_removed() {
        let settings = RepositoryPreferences {
            cleaner_exclude_patterns: vec![
                "  build\\cache  ".to_string(),
                "../outside".to_string(),
                "/rooted".to_string(),
                "   ".to_string(),
                "src/generated".to_string(),
            ],
            ..RepositoryPreferences::default()
        };
        let normalized = normalize_settings(settings).expect("normalize settings");
        assert_eq!(
            normalized.cleaner_exclude_patterns,
            vec!["build/cache".to_string(), "src/generated".to_string()]
        );
    }

    #[test]
    fn too_many_exclude_patterns_are_rejected() {
        let settings = RepositoryPreferences {
            cleaner_exclude_patterns: (0..257).map(|index| format!("pattern-{index}")).collect(),
            ..RepositoryPreferences::default()
        };
        assert!(normalize_settings(settings).is_err());
    }
}
