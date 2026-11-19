//! Shared notification history for the GitManager UI and OS notification delivery.
//! The service publishes the same notification record that the UI reads.

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_NOTIFICATION_COUNT: usize = 200;
const MAX_NOTIFICATION_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppNotification {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub created_at_unix_ms: u64,
    pub read: bool,
    pub repository_path: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct NotificationStore {
    schema_version: u32,
    notifications: Vec<AppNotification>,
}

fn notification_file() -> Result<PathBuf, String> {
    Ok(crate::storage::data_dir()?
        .join("notifications")
        .join("notifications.json"))
}

fn notification_generation_path(store_path: &PathBuf) -> PathBuf {
    store_path.with_extension("generation")
}

fn read_notification_generation_file(path: &Path) -> Result<Option<u64>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not inspect notification generation: {error}"))?;
    if metadata.len() > 32 {
        return Err("Notification generation file is invalid.".to_string());
    }
    let content = fs::read_to_string(path)
        .map_err(|error| format!("Could not read notification generation: {error}"))?;
    content.trim().parse::<u64>().map(Some)
        .map_err(|_| "Notification generation file is invalid.".to_string())
}

fn notification_generation_for_path(store_path: &Path) -> Result<u64, String> {
    let sidecar = notification_generation_path(&store_path.to_path_buf());
    let sidecar_generation = read_notification_generation_file(&sidecar)
        .ok()
        .flatten()
        .unwrap_or(0);
    let file_generation = if store_path.is_file() {
        let modified = fs::metadata(store_path)
            .map_err(|error| format!("Could not inspect notification history: {error}"))?
            .modified()
            .map_err(|error| format!("Could not read notification history timestamp: {error}"))?;
        modified.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
            .min(u64::MAX as u128) as u64
    } else {
        0
    };
    Ok(sidecar_generation.max(file_generation))
}

fn next_notification_generation(previous: u64, current_unix_ns: u128) -> u64 {
    previous.saturating_add(1).max(current_unix_ns.min(u64::MAX as u128) as u64)
}

fn write_notification_generation_file(path: &Path, generation: u64) -> Result<(), String> {
    let temporary = path.with_extension("generation.tmp");
    let backup = path.with_extension("generation.previous");
    {
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Could not create notification generation: {error}"))?;
        write!(output, "{generation}\n")
            .map_err(|error| format!("Could not write notification generation: {error}"))?;
        output.sync_all()
            .map_err(|error| format!("Could not flush notification generation: {error}"))?;
    }
    let had_previous = path.is_file();
    if had_previous {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Could not clear notification generation backup: {error}"))?;
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("Could not stage notification generation: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_previous {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("Could not publish notification generation: {error}"));
    }
    if backup.exists() {
        let _ = fs::remove_file(backup);
    }

    Ok(())
}

/// Returns a small monotonic notification generation for native event monitoring.
pub fn notification_store_modified_unix_nanos() -> Result<u128, String> {
    Ok(u128::from(notification_generation_for_path(&notification_file()?)?))
}

fn lock_file(path: &PathBuf) -> Result<File, String> {
    let parent = path.parent()
        .ok_or_else(|| "Notification history path has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create notification history directory: {error}"))?;
    let lock_path = path.with_extension("lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| format!("Could not open notification history lock: {error}"))?;
    file.lock_exclusive()
        .map_err(|error| format!("Could not lock notification history: {error}"))?;
    Ok(file)
}

fn read_store(path: &PathBuf) -> Result<NotificationStore, String> {
    let backup = path.with_extension("json.previous");
    if !path.is_file() && backup.is_file() {
        fs::rename(&backup, path)
            .map_err(|error| format!("Could not recover the previous notification history: {error}"))?;
    }
    if !path.is_file() {
        return Ok(NotificationStore { schema_version: 1, notifications: Vec::new() });
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not inspect notification history: {error}"))?;
    if metadata.len() > MAX_NOTIFICATION_FILE_BYTES {
        return Err("Notification history exceeds the 2 MiB safety limit.".to_string());
    }
    let bytes = fs::read(path)
        .map_err(|error| format!("Could not read notification history: {error}"))?;
    let store: NotificationStore = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Notification history is invalid: {error}"))?;
    if store.schema_version != 1 {
        return Err(format!("Notification history schema {} is unsupported.", store.schema_version));
    }
    Ok(store)
}

fn write_store(path: &PathBuf, store: &NotificationStore) -> Result<(), String> {
    let previous_generation = notification_generation_for_path(path).unwrap_or(0);
    let current_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let next_generation = next_notification_generation(previous_generation, current_unix_ns);
    let bytes = serde_json::to_vec(store)
        .map_err(|error| format!("Could not encode notification history: {error}"))?;
    if bytes.len() as u64 > MAX_NOTIFICATION_FILE_BYTES {
        return Err("Notification history exceeds the 2 MiB safety limit.".to_string());
    }
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.previous");
    {
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Could not create temporary notification history: {error}"))?;
        output.write_all(&bytes)
            .map_err(|error| format!("Could not write temporary notification history: {error}"))?;
        output.sync_all()
            .map_err(|error| format!("Could not flush temporary notification history: {error}"))?;
    }

    let had_previous = path.is_file();
    if had_previous {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Could not clear previous notification backup: {error}"))?;
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("Could not stage notification history: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_previous {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("Could not publish notification history: {error}"));
    }
    if backup.exists() {
        let _ = fs::remove_file(backup);
    }

    // History is already committed at this point. A generation-sidecar failure
    // must not suppress a matching OS toast; file-mtime fallback remains.
    let generation_path = notification_generation_path(path);
    if let Err(error) = write_notification_generation_file(&generation_path, next_generation) {
        eprintln!("GitManager notification generation could not be published: {error}");
    }
    Ok(())
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub fn list_notifications() -> Result<Vec<AppNotification>, String> {
    let path = notification_file()?;
    let _lock = lock_file(&path)?;
    let store = read_store(&path)?;
    Ok(store.notifications.into_iter().rev().collect())
}

pub fn publish_notification(
    id: String,
    kind: &str,
    title: &str,
    body: &str,
    repository_path: Option<&str>,
) -> Result<(AppNotification, bool), String> {
    let path = notification_file()?;
    let _lock = lock_file(&path)?;
    let mut store = read_store(&path)?;
    if let Some(existing) = store.notifications.iter().find(|item| item.id == id) {
        return Ok((existing.clone(), false));
    }
    let notification = AppNotification {
        id,
        kind: kind.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        created_at_unix_ms: now_unix_ms(),
        read: false,
        repository_path: repository_path.map(str::to_string),
    };
    store.schema_version = 1;
    store.notifications.push(notification.clone());
    if store.notifications.len() > MAX_NOTIFICATION_COUNT {
        let excess = store.notifications.len() - MAX_NOTIFICATION_COUNT;
        store.notifications.drain(0..excess);
    }
    write_store(&path, &store)?;
    Ok((notification, true))
}

fn repository_update_notification_id(repo_id: &str, remote_oid: &str) -> String {
    // FNV-1a gives a deterministic identifier across the UI and service processes.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{repo_id}\n{remote_oid}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("repo-update-{hash:016x}")
}

pub fn publish_repository_update(
    repo_id: &str,
    repo_name: &str,
    repo_path: &str,
    remote_oid: &str,
    message: &str,
) -> Result<(AppNotification, bool), String> {
    let id = repository_update_notification_id(repo_id, remote_oid);
    publish_notification(
        id,
        "repository-update",
        &format!("{} — Update available", repo_name),
        message,
        Some(repo_path),
    )
}

pub fn mark_notification_read(id: &str) -> Result<(), String> {
    mutate_notification(id, |notification| notification.read = true)
}

fn mark_store_notifications_read(store: &mut NotificationStore) -> usize {
    let mut changed = 0usize;
    for notification in &mut store.notifications {
        if !notification.read {
            notification.read = true;
            changed += 1;
        }
    }
    changed
}

/// Marks every currently unread notification as read in one atomic store update.
pub fn mark_all_notifications_read() -> Result<usize, String> {
    let path = notification_file()?;
    let _lock = lock_file(&path)?;
    let mut store = read_store(&path)?;
    let changed = mark_store_notifications_read(&mut store);
    if changed > 0 {
        write_store(&path, &store)?;
    }
    Ok(changed)
}

pub fn dismiss_notification(id: &str) -> Result<(), String> {
    let path = notification_file()?;
    let _lock = lock_file(&path)?;
    let mut store = read_store(&path)?;
    store.notifications.retain(|notification| notification.id != id);
    write_store(&path, &store)
}

fn mutate_notification(
    id: &str,
    update: impl FnOnce(&mut AppNotification),
) -> Result<(), String> {
    let path = notification_file()?;
    let _lock = lock_file(&path)?;
    let mut store = read_store(&path)?;
    if let Some(notification) = store.notifications.iter_mut().find(|item| item.id == id) {
        update(notification);
        write_store(&path, &store)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        next_notification_generation, notification_generation_for_path,
        read_notification_generation_file, write_notification_generation_file,
    };
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn notification_generation_advances_even_when_the_clock_does_not() {
        assert_eq!(next_notification_generation(100, 100), 101);
        assert_eq!(next_notification_generation(100, 99), 101);
        assert_eq!(next_notification_generation(100, 150), 150);
    }

    #[test]
    fn notification_generation_sidecar_round_trips() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("gitmanager-notification-generation-{nonce}"));
        fs::create_dir_all(&directory).expect("create notification generation test directory");
        let store_path = directory.join("notifications.json");
        let generation_path = PathBuf::from(directory.join("notifications.generation"));

        fs::write(&store_path, "{}").expect("create notification history fixture");
        write_notification_generation_file(&generation_path, 200).expect("publish notification generation");
        assert_eq!(read_notification_generation_file(&generation_path).unwrap(), Some(200));
        assert_eq!(notification_generation_for_path(&store_path).unwrap(), 200);
        write_notification_generation_file(&generation_path, 201).expect("replace notification generation");
        assert_eq!(notification_generation_for_path(&store_path).unwrap(), 201);

        fs::remove_dir_all(directory).expect("remove notification generation fixture");
    }

    use super::{
        mark_store_notifications_read, repository_update_notification_id, AppNotification,
        NotificationStore, MAX_NOTIFICATION_COUNT,
    };

    #[test]
    fn repository_update_ids_are_stable_per_repository_and_remote_commit() {
        let first = repository_update_notification_id("local::repo-1", "abc123");
        let repeated = repository_update_notification_id("local::repo-1", "abc123");
        let different_commit = repository_update_notification_id("local::repo-1", "def456");
        let different_repository = repository_update_notification_id("local::repo-2", "abc123");
        assert_eq!(first, repeated);
        assert_ne!(first, different_commit);
        assert_ne!(first, different_repository);
    }


    #[test]
    fn bulk_read_only_changes_unread_notifications() {
        let mut store = NotificationStore {
            schema_version: 1,
            notifications: vec![
                AppNotification {
                    id: "unread".to_string(), kind: "repository-update".to_string(),
                    title: "Update".to_string(), body: "Body".to_string(),
                    created_at_unix_ms: 1, read: false, repository_path: None,
                },
                AppNotification {
                    id: "read".to_string(), kind: "system".to_string(),
                    title: "Finished".to_string(), body: "Done".to_string(),
                    created_at_unix_ms: 2, read: true, repository_path: None,
                },
            ],
        };
        assert_eq!(mark_store_notifications_read(&mut store), 1);
        assert!(store.notifications.iter().all(|notification| notification.read));
        assert_eq!(mark_store_notifications_read(&mut store), 0);
    }

    #[test]
    fn notification_store_write_advances_generation() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("gitmanager-notification-store-{nonce}"));
        fs::create_dir_all(&directory).expect("create notification store fixture");
        let path = directory.join("notifications.json");
        let store = NotificationStore { schema_version: 1, notifications: Vec::new() };

        super::write_store(&path, &store).expect("write initial notification store");
        let first = notification_generation_for_path(&path).expect("read first generation");
        super::write_store(&path, &store).expect("write updated notification store");
        let second = notification_generation_for_path(&path).expect("read second generation");
        assert!(second > first);

        fs::remove_dir_all(directory).expect("remove notification store fixture");
    }

    #[test]
    fn notification_store_schema_defaults_and_round_trips() {
        let store = NotificationStore {
            schema_version: 1,
            notifications: vec![AppNotification {
                id: "test".to_string(),
                kind: "test".to_string(),
                title: "Test".to_string(),
                body: "Body".to_string(),
                created_at_unix_ms: 1,
                read: false,
                repository_path: None,
            }],
        };
        let encoded = serde_json::to_vec(&store).unwrap();
        let decoded: NotificationStore = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.notifications[0].id, "test");
        assert_eq!(MAX_NOTIFICATION_COUNT, 200);
    }
}
