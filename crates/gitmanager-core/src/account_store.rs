//! Persistent, non-secret account metadata and cached provider repositories.
//!
//! Access tokens are deliberately excluded. Provider credentials remain in the
//! active connection flow (or in the provider CLI's own credential manager).

use crate::git_info::GitRepositoryInfo;
use crate::storage::data_dir;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const ACCOUNT_SCHEMA_VERSION: u32 = 1;
const MAX_ACCOUNT_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ACCOUNT_REPOSITORY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountMetadata {
    pub provider: String,
    pub account_login: String,
    pub display_name: String,
    pub email: String,
    pub avatar_url: String,
    pub profile_url: String,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccount {
    pub metadata: AccountMetadata,
    pub repositories: Vec<GitRepositoryInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccountMetadataFile {
    schema_version: u32,
    accounts: Vec<AccountMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccountRepositoriesFile {
    schema_version: u32,
    provider: String,
    account_login: String,
    repositories: Vec<GitRepositoryInfo>,
}

pub fn load_accounts() -> Result<Vec<StoredAccount>, String> {
    crate::storage::ensure_data_layout()?;
    load_accounts_at(&data_dir()?)
}

pub fn save_account(
    metadata: &AccountMetadata,
    repositories: &[GitRepositoryInfo],
) -> Result<(), String> {
    crate::storage::ensure_data_layout()?;
    save_account_at(&data_dir()?, metadata, repositories)
}

fn metadata_path_at(root: &Path) -> PathBuf {
    root.join("account").join("metadata").join("accounts.json")
}

fn repositories_path_at(root: &Path, provider: &str, account_login: &str) -> PathBuf {
    let identity = format!(
        "{}::{}",
        provider.trim().to_ascii_lowercase(),
        account_login.trim().to_ascii_lowercase()
    );
    let mut hash = 0x811c9dc5u32;
    for byte in identity.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    let provider_slug = provider
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    let provider_slug = if provider_slug.is_empty() {
        "provider".to_string()
    } else {
        provider_slug
    };
    root.join("account")
        .join("git_data")
        .join(format!("{provider_slug}-{hash:08x}.json"))
}

fn load_account_metadata_at(root: &Path) -> Result<Vec<AccountMetadata>, String> {
    let path = metadata_path_at(root);
    let Some(file) = read_json::<AccountMetadataFile>(&path, MAX_ACCOUNT_METADATA_BYTES)? else {
        return Ok(Vec::new());
    };
    if file.schema_version != ACCOUNT_SCHEMA_VERSION {
        return Err(format!(
            "Account metadata schema {} is unsupported; expected {}.",
            file.schema_version, ACCOUNT_SCHEMA_VERSION
        ));
    }
    Ok(file.accounts)
}

fn load_account_repositories_at(
    root: &Path,
    metadata: &AccountMetadata,
) -> Result<Vec<GitRepositoryInfo>, String> {
    let path = repositories_path_at(root, &metadata.provider, &metadata.account_login);
    let Some(file) = read_json::<AccountRepositoriesFile>(&path, MAX_ACCOUNT_REPOSITORY_BYTES)? else {
        return Ok(Vec::new());
    };
    if file.schema_version != ACCOUNT_SCHEMA_VERSION {
        return Err(format!(
            "Cached repositories for {} have unsupported schema {}.",
            metadata.account_login, file.schema_version
        ));
    }
    if !file.provider.eq_ignore_ascii_case(&metadata.provider)
        || !file.account_login.eq_ignore_ascii_case(&metadata.account_login)
    {
        return Err(format!(
            "Cached account repository identity does not match {}.",
            metadata.account_login
        ));
    }
    Ok(file.repositories)
}

fn load_accounts_at(root: &Path) -> Result<Vec<StoredAccount>, String> {
    let metadata = load_account_metadata_at(root)?;
    metadata
        .into_iter()
        .map(|metadata| {
            let repositories = load_account_repositories_at(root, &metadata)?;
            Ok(StoredAccount { metadata, repositories })
        })
        .collect()
}

fn save_account_at(
    root: &Path,
    metadata: &AccountMetadata,
    repositories: &[GitRepositoryInfo],
) -> Result<(), String> {
    let path = metadata_path_at(root);
    let parent = path
        .parent()
        .ok_or_else(|| "Account metadata path has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create account metadata directory: {error}"))?;

    let lock_path = parent.join(".accounts.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| format!("Could not open account metadata lock: {error}"))?;
    set_private_permissions(&lock_path)?;
    lock_file
        .lock_exclusive()
        .map_err(|error| format!("Could not lock account metadata: {error}"))?;

    // Write the repository cache first. If publishing metadata fails, this
    // leaves only an orphaned cache file rather than an account without data.
    let repositories_path =
        repositories_path_at(root, &metadata.provider, &metadata.account_login);
    let repository_file = AccountRepositoriesFile {
        schema_version: ACCOUNT_SCHEMA_VERSION,
        provider: metadata.provider.clone(),
        account_login: metadata.account_login.clone(),
        repositories: repositories.to_vec(),
    };
    let repository_bytes = serde_json::to_vec(&repository_file)
        .map_err(|error| format!("Could not encode cached account repositories: {error}"))?;
    if repository_bytes.len() as u64 > MAX_ACCOUNT_REPOSITORY_BYTES {
        return Err("Cached account repository data exceeds the 64 MiB safety limit.".to_string());
    }
    atomic_write(&repositories_path, &repository_bytes)?;

    let mut accounts = load_account_metadata_at(root)?;
    let mut saved_metadata = metadata.clone();
    if saved_metadata.updated_at_unix_ms == 0 {
        saved_metadata.updated_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
    }
    accounts.retain(|existing| {
        !(existing.provider.eq_ignore_ascii_case(&saved_metadata.provider)
            && existing.account_login.eq_ignore_ascii_case(&saved_metadata.account_login))
    });
    accounts.push(saved_metadata);

    let metadata_file = AccountMetadataFile {
        schema_version: ACCOUNT_SCHEMA_VERSION,
        accounts,
    };
    let metadata_bytes = serde_json::to_vec(&metadata_file)
        .map_err(|error| format!("Could not encode account metadata: {error}"))?;
    if metadata_bytes.len() as u64 > MAX_ACCOUNT_METADATA_BYTES {
        return Err("Account metadata exceeds the 4 MiB safety limit.".to_string());
    }
    atomic_write(&path, &metadata_bytes)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    maximum_bytes: u64,
) -> Result<Option<T>, String> {
    // Readers never move a backup: a writer may be between its two rename
    // operations, and restoring here could race with the writer's publish.
    let backup = path.with_extension("previous");
    let source = if path.is_file() {
        path
    } else if backup.is_file() {
        backup.as_path()
    } else {
        return Ok(None);
    };
    let metadata = fs::metadata(source)
        .map_err(|error| format!("Could not inspect {}: {error}", source.display()))?;
    if metadata.len() > maximum_bytes {
        return Err(format!(
            "{} exceeds its {} MiB safety limit.",
            source.display(),
            maximum_bytes / (1024 * 1024)
        ));
    }
    let mut file = File::open(source)
        .map_err(|error| format!("Could not open {}: {error}", source.display()))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read {}: {error}", source.display()))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("{} contains invalid JSON: {error}", source.display()))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Account storage path has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create account storage directory: {error}"))?;
    recover_backup(path)?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .ok_or_else(|| "Account storage path has no filename.".to_string())?
        .to_string_lossy();
    let temporary = parent.join(format!(
        "{file_name}.tmp-{}-{timestamp}",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("Could not create {}: {error}", temporary.display()))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not write {}: {error}", temporary.display()));
    }
    drop(file);
    set_private_permissions(&temporary)?;

    let backup = path.with_extension("previous");
    if path.is_file() {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Could not clear {}: {error}", backup.display()))?;
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("Could not stage {}: {error}", path.display()))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not publish {}: {error}", path.display()));
    }
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    Ok(())
}

fn recover_backup(path: &Path) -> Result<(), String> {
    let backup = path.with_extension("previous");
    if !path.exists() && backup.is_file() {
        fs::rename(&backup, path)
            .map_err(|error| format!("Could not recover {}: {error}", path.display()))?;
    }
    Ok(())
}

fn set_private_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not restrict permissions on {}: {error}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn account_metadata_and_repository_cache_are_stored_separately() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("gitmanager-account-store-{suffix}"));
        let metadata = AccountMetadata {
            provider: "github".to_string(),
            account_login: "example-user".to_string(),
            display_name: "Example User".to_string(),
            email: "example@example.invalid".to_string(),
            avatar_url: String::new(),
            profile_url: "https://github.com/example-user".to_string(),
            updated_at_unix_ms: 1,
        };

        save_account_at(&root, &metadata, &[]).expect("save account metadata");
        let accounts = load_accounts_at(&root).expect("load saved account metadata");

        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].metadata, metadata);
        assert!(accounts[0].repositories.is_empty());
        assert!(root.join("account/metadata/accounts.json").is_file());
        let repository_cache = repositories_path_at(&root, "github", "example-user");
        assert!(repository_cache.is_file());
        let repository_cache_dir = root.join("account/git_data");
        assert_eq!(repository_cache.parent(), Some(repository_cache_dir.as_path()));

        let encoded = fs::read_to_string(root.join("account/metadata/accounts.json"))
            .expect("read persisted metadata");
        assert!(!encoded.contains("access_token"));
        assert!(!encoded.contains("token"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn readers_use_previous_snapshot_without_moving_it_during_recovery_window() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("gitmanager-account-recovery-{suffix}"));
        let path = metadata_path_at(&root);
        fs::create_dir_all(path.parent().expect("metadata parent")).expect("create metadata parent");
        let backup = path.with_extension("previous");
        let saved = AccountMetadataFile { schema_version: ACCOUNT_SCHEMA_VERSION, accounts: Vec::new() };
        fs::write(&backup, serde_json::to_vec(&saved).expect("encode backup")).expect("write backup");

        let loaded = read_json::<AccountMetadataFile>(&path, MAX_ACCOUNT_METADATA_BYTES)
            .expect("read backup snapshot")
            .expect("backup snapshot exists");
        assert_eq!(loaded.schema_version, ACCOUNT_SCHEMA_VERSION);
        assert!(!path.exists());
        assert!(backup.is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn account_repository_cache_filename_cannot_escape_its_storage_directory() {
        let root = std::env::temp_dir().join("gitmanager-account-cache-path-test");
        let path = repositories_path_at(&root, "github", "../../outside");
        assert_eq!(
            path.parent(),
            Some(root.join("account/git_data").as_path())
        );
    }
}