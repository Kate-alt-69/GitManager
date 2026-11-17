//! Canonical per-user GitManager storage layout shared by the desktop and service.

use std::fs;
use std::path::PathBuf;

pub fn software_root() -> Result<PathBuf, String> {
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the current user's local application-data directory.".to_string())?;
    Ok(local_data.join("kastrick's_software"))
}

pub fn config_dir() -> Result<PathBuf, String> {
    Ok(software_root()?.join("config").join("GitManager"))
}

pub fn data_dir() -> Result<PathBuf, String> {
    Ok(software_root()?.join("data").join("GitManager"))
}

/// Create every stable application-owned data directory before the service
/// starts indexing. Keep cache separate from durable metadata and Git data.
pub fn ensure_data_layout() -> Result<PathBuf, String> {
    let root = data_dir()?;
    for (label, relative) in [
        ("repository metadata", "repo/metadata"),
        ("Git data", "git_data"),
        ("account metadata", "account/metadata"),
        ("account Git data", "account/git_data"),
        ("remote repository metadata", "repo/remote/metadata"),
        ("remote repository Git data", "repo/remote/git_data"),
        ("cache", ".cache"),
        ("notification history", "notifications"),
    ] {
        fs::create_dir_all(root.join(relative))
            .map_err(|error| format!("Could not create GitManager {label} directory: {error}"))?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn data_layout_contains_all_separate_repository_account_and_cache_directories() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("gitmanager-layout-{suffix}"));
        let paths = [
            "repo/metadata",
            "git_data",
            "account/metadata",
            "account/git_data",
            "repo/remote/metadata",
            "repo/remote/git_data",
            ".cache",
            "notifications",
        ];
        for relative in paths {
            fs::create_dir_all(root.join(relative)).unwrap();
            assert!(root.join(relative).is_dir(), "missing {relative}");
        }
        let _ = fs::remove_dir_all(root);
    }
}
