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
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| "Could not resolve the user's local data directory for notifications.".to_string())?;
    Ok(local_data
        .join("kastrick's_software")
        .join("data")
        .join("GitManager")
        .join("notifications")
        .join("notifications.json"))
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

pub fn publish_repository_update(
    repo_id: &str,
    repo_name: &str,
    repo_path: &str,
    remote_oid: &str,
    message: &str,
) -> Result<(AppNotification, bool), String> {
    // FNV-1a gives a deterministic identifier across the UI and service processes.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{repo_id}\n{remote_oid}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let id = format!("repo-update-{hash:016x}");
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
    use super::{AppNotification, NotificationStore, MAX_NOTIFICATION_COUNT};

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
