use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::process_utils;

/// Supported TaskManager operation kinds executed inside per-repo serialized lanes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
	Clone,
	Stage,
	Commit,
	Push,
	Pull,
	Fetch,
	StashPush,
	StashPop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedTaskItem {
	pub id: u64,
	pub repo_key: String,
	pub repo_name: String,
	pub disk_path: String,
	pub kind: String,
	pub title: String,
	pub command_preview: String,
	pub status: String, // queued | running | completed | failed | canceled
	pub progress: u8,
	pub phase: String,
	pub reversible: bool,
	pub logs: Vec<String>,
	#[serde(skip)]
	pub args_payload: TaskPayload,
}

#[derive(Debug, Clone, Default)]
pub struct TaskPayload {
	pub clone_url: Option<String>,
	pub commit_message: Option<String>,
	pub branch: Option<String>,
	pub paths: Vec<String>,
}

/// Reversible Undo/Redo history entry with per-repo overwrite-conflict protection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryEntry {
	pub id: u64,
	pub repo_key: String,
	pub repo_name: String,
	pub disk_path: String,
	pub kind: String,
	pub title: String,
	pub finished_at: u64,
	pub reversible: bool,
	pub status: String, // applied | reverted | failed
	pub overwritten: bool,
	pub revert_note: String,
	pub before_ref: Option<String>,
	#[serde(default)]
	pub after_ref: Option<String>,
	#[serde(default)]
	pub before_index_snapshot: Option<String>,
	#[serde(default)]
	pub after_index_snapshot: Option<String>,
	#[serde(default)]
	pub before_index_existed: bool,
	#[serde(default)]
	pub stash_oid: Option<String>,
	pub undo_cache_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoTaskGroup {
	pub repo_key: String,
	pub repo_name: String,
	pub disk_path: String,
	pub running_count: usize,
	pub queued_count: usize,
	pub tasks: Vec<QueuedTaskItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskManagerSnapshot {
	pub paused: bool,
	pub active_lanes: usize,
	pub total_running: usize,
	pub total_queued: usize,
	pub groups: Vec<RepoTaskGroup>,
	pub history: Vec<TaskHistoryEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueTaskRequest {
	pub repo_name: String,
	pub disk_path: String,
	pub kind: String,
	pub title: Option<String>,
	pub clone_url: Option<String>,
	pub commit_message: Option<String>,
	pub branch: Option<String>,
	pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
struct TaskExecutionMetadata {
	before_ref: Option<String>,
	after_ref: Option<String>,
	before_index_snapshot: Option<String>,
	after_index_snapshot: Option<String>,
	before_index_existed: bool,
	stash_oid: Option<String>,
	reversible: bool,
}

struct HistoryOperationGuard {
	repo_key: String,
}

impl Drop for HistoryOperationGuard {
	fn drop(&mut self) {
		let mgr = manager();
		let lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
		let has_pending = lanes.get(&self.repo_key).map(|queue| !queue.is_empty()).unwrap_or(false);
		let mut active = mgr.active_lane_keys.lock().unwrap_or_else(|p| p.into_inner());
		active.remove(&self.repo_key);
		drop(active);
		drop(lanes);
		if has_pending && !mgr.paused.load(Ordering::Relaxed) {
			spawn_lane_worker_if_idle(self.repo_key.clone());
		}
	}
}

fn reserve_history_operation(repo_key: &str) -> Result<HistoryOperationGuard, String> {
	let mgr = manager();
	let lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
	if lanes.get(repo_key).map(|queue| !queue.is_empty()).unwrap_or(false) {
		return Err("This repository has queued Git tasks; wait for its lane to become idle before Undo/Redo.".to_string());
	}
	let mut active = mgr.active_lane_keys.lock().unwrap_or_else(|p| p.into_inner());
	if active.get(repo_key).copied().unwrap_or(false) {
		return Err("A Git operation is still running for this repository; retry Undo/Redo when it finishes.".to_string());
	}
	active.insert(repo_key.to_string(), true);
	Ok(HistoryOperationGuard { repo_key: repo_key.to_string() })
}

struct TaskManagerState {
	paused: AtomicBool,
	next_id: AtomicU64,
	lanes: Mutex<HashMap<String, Vec<QueuedTaskItem>>>,
	active_lane_keys: Mutex<HashMap<String, bool>>,
	history: Mutex<Vec<TaskHistoryEntry>>,
}

fn manager() -> &'static Arc<TaskManagerState> {
	static INSTANCE: OnceLock<Arc<TaskManagerState>> = OnceLock::new();
	INSTANCE.get_or_init(|| {
		Arc::new(TaskManagerState {
			paused: AtomicBool::new(false),
			next_id: AtomicU64::new(1),
			lanes: Mutex::new(HashMap::new()),
			active_lane_keys: Mutex::new(HashMap::new()),
			history: Mutex::new(Vec::new()),
		})
	})
}

fn now_epoch() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or(0)
}

fn normalize_repo_key(disk_path: &str) -> String {
	let normalized = disk_path.trim().replace('\\', "/");
	// Windows paths are case-insensitive; do not collapse case-distinct paths
	// on Linux or on case-sensitive macOS volumes.
	#[cfg(windows)]
	{
		normalized.to_lowercase()
	}
	#[cfg(not(windows))]
	{
		normalized
	}
}

fn unique_undo_cache_path(root: &Path, task_id: u64) -> PathBuf {
	static CACHE_NONCE: AtomicU64 = AtomicU64::new(0);
	let timestamp = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_nanos())
		.unwrap_or_default();
	let nonce = CACHE_NONCE.fetch_add(1, Ordering::Relaxed);
	root.join(format!("task-{task_id}-{timestamp}-{nonce}"))
}

/// Expands shorthand `github:owner/repo` or `gitlab:owner/repo` into HTTPS clone URLs.
pub fn expand_clone_shorthand(raw: &str) -> String {
	let trimmed = raw.trim();
	if let Some(slug) = trimmed.strip_prefix("github:") {
		let clean = slug.trim().trim_end_matches(".git");
		return format!("https://github.com/{clean}.git");
	}
	if let Some(slug) = trimmed.strip_prefix("gitlab:") {
		let clean = slug.trim().trim_end_matches(".git");
		return format!("https://gitlab.com/{clean}.git");
	}
	trimmed.to_string()
}

fn current_head_oid(repo_path: &Path) -> Option<String> {
	let out = process_utils::hidden_command("git")
		.args(["rev-parse", "HEAD"])
		.current_dir(repo_path)
		.output()
		.ok()?;
	if out.status.success() {
		let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
		if !oid.is_empty() {
			return Some(oid);
		}
	}
	None
}

fn undo_cache_dir() -> PathBuf {
	dirs::data_local_dir()
		.unwrap_or_else(std::env::temp_dir)
		.join("kastrick's_software")
		.join("data")
		.join("GitManager")
		.join("task-undo-cache")
}

fn git_output(repo_path: &Path, args: &[&str]) -> Result<std::process::Output, String> {
	process_utils::hidden_command("git")
		.args(args)
		.current_dir(repo_path)
		.output()
		.map_err(|error| format!("Could not execute git {}: {error}", args.first().copied().unwrap_or("")))
}

fn git_text(repo_path: &Path, args: &[&str]) -> Result<String, String> {
	let output = git_output(repo_path, args)?;
	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
		return Err(if stderr.is_empty() { format!("git {} failed.", args.first().copied().unwrap_or("")) } else { stderr });
	}
	Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_index_path(repo_path: &Path) -> Result<PathBuf, String> {
	let raw = git_text(repo_path, &["rev-parse", "--git-path", "index"])?;
	let path = PathBuf::from(raw);
	Ok(if path.is_absolute() { path } else { repo_path.join(path) })
}

fn capture_index_snapshot(repo_path: &Path, task_id: u64, label: &str) -> Result<(bool, Option<String>), String> {
	let index = git_index_path(repo_path)?;
	let lock = index.with_file_name("index.lock");
	if lock.exists() {
		return Err("Git index is locked by another Git operation; cannot capture a safe staging snapshot.".to_string());
	}
	if !index.exists() {
		return Ok((false, None));
	}
	let before = std::fs::read(&index).map_err(|error| format!("Could not read Git index for staging snapshot: {error}"))?;
	if lock.exists() {
		return Err("Git index changed while preparing a staging snapshot; retry the task.".to_string());
	}
	let root = undo_cache_dir();
	std::fs::create_dir_all(&root).map_err(|error| format!("Could not prepare index snapshot cache: {error}"))?;
	let snapshot_dir = unique_undo_cache_path(&root, task_id);
	std::fs::create_dir_all(&snapshot_dir).map_err(|error| format!("Could not create index snapshot directory: {error}"))?;
	let snapshot = snapshot_dir.join(format!("{label}.index"));
	std::fs::write(&snapshot, &before).map_err(|error| format!("Could not save Git index snapshot: {error}"))?;
	let after = std::fs::read(&index).map_err(|error| format!("Could not verify Git index snapshot: {error}"))?;
	if before != after || lock.exists() {
		let _ = std::fs::remove_file(&snapshot);
		let _ = std::fs::remove_dir(&snapshot_dir);
		return Err("Git index changed while the staging snapshot was being captured; retry the task.".to_string());
	}
	Ok((true, Some(snapshot.to_string_lossy().to_string())))
}

fn index_matches_snapshot(repo_path: &Path, expected_path: Option<&str>, expected_exists: bool) -> Result<bool, String> {
	let index = git_index_path(repo_path)?;
	if !expected_exists {
		return Ok(!index.exists());
	}
	let Some(snapshot) = expected_path else {
		return Err("The saved Git index snapshot is missing.".to_string());
	};
	if !index.is_file() {
		return Ok(false);
	}
	let current = std::fs::read(&index).map_err(|error| format!("Could not read current Git index: {error}"))?;
	let expected = std::fs::read(snapshot).map_err(|error| format!("Could not read saved Git index snapshot: {error}"))?;
	Ok(current == expected)
}

fn restore_index_snapshot(
	repo_path: &Path,
	expected_path: Option<&str>,
	expected_exists: bool,
	restore_path: Option<&str>,
	restore_exists: bool,
) -> Result<(), String> {
	let index = git_index_path(repo_path)?;
	let lock = index.with_file_name("index.lock");
	let mut lock_file = std::fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(&lock)
		.map_err(|error| {
			if error.kind() == std::io::ErrorKind::AlreadyExists {
				"Git index is locked by another Git operation; retry Undo/Redo after it finishes.".to_string()
			} else {
				format!("Could not acquire the Git index lock for Undo/Redo: {error}")
			}
		})?;

	let matches = match index_matches_snapshot(repo_path, expected_path, expected_exists) {
		Ok(matches) => matches,
		Err(error) => {
			drop(lock_file);
			let _ = std::fs::remove_file(&lock);
			return Err(error);
		}
	};
	if !matches {
		drop(lock_file);
		let _ = std::fs::remove_file(&lock);
		return Err("Git index has changed since this task; refusing to overwrite newer staged work.".to_string());
	}
	if restore_exists {
		let Some(snapshot) = restore_path else {
			drop(lock_file);
			let _ = std::fs::remove_file(&lock);
			return Err("The Git index snapshot needed for Undo/Redo is unavailable.".to_string());
		};
		let bytes = match std::fs::read(snapshot) {
			Ok(bytes) => bytes,
			Err(error) => {
				drop(lock_file);
				let _ = std::fs::remove_file(&lock);
				return Err(format!("Could not read the saved Git index snapshot: {error}"));
			}
		};
		if let Err(error) = lock_file.write_all(&bytes).and_then(|_| lock_file.sync_all()) {
			drop(lock_file);
			let _ = std::fs::remove_file(&lock);
			return Err(format!("Could not prepare the restored Git index: {error}"));
		}
	}
	drop(lock_file);

	if !restore_exists {
		if index.exists() {
			if let Err(error) = std::fs::remove_file(&index) {
				let _ = std::fs::remove_file(&lock);
				return Err(format!("Could not restore the previously absent Git index: {error}"));
			}
		}
		std::fs::remove_file(&lock).map_err(|error| format!("Could not release the Git index lock: {error}"))?;
		return Ok(());
	}

	static INDEX_RESTORE_NONCE: AtomicU64 = AtomicU64::new(0);
	let nonce = INDEX_RESTORE_NONCE.fetch_add(1, Ordering::Relaxed);
	let backup = index.with_file_name(format!("index.gitmanager-{nonce}.bak"));
	let had_index = index.exists();
	if had_index {
		if let Err(error) = std::fs::rename(&index, &backup) {
			let _ = std::fs::remove_file(&lock);
			return Err(format!("Could not safely replace the Git index: {error}"));
		}
	}
	if let Err(error) = std::fs::rename(&lock, &index) {
		if had_index {
			let _ = std::fs::rename(&backup, &index);
		}
		let _ = std::fs::remove_file(&lock);
		return Err(format!("Could not install the restored Git index: {error}"));
	}
	if had_index {
		let _ = std::fs::remove_file(backup);
	}
	Ok(())
}

fn current_stash_oid(repo_path: &Path) -> Option<String> {
	git_text(repo_path, &["rev-parse", "--verify", "refs/stash"]).ok().filter(|value| !value.is_empty())
}

fn stash_position(repo_path: &Path, oid: &str) -> Result<Option<usize>, String> {
	let list = git_text(repo_path, &["stash", "list", "--format=%H"])?;
	Ok(list.lines().position(|line| line.trim() == oid))
}

fn ensure_clean_worktree(repo_path: &Path) -> Result<(), String> {
	let status = git_text(repo_path, &["status", "--porcelain"])?;
	if !status.is_empty() {
		return Err("Working tree is not clean; refusing to apply or re-stash over unrelated changes.".to_string());
	}
	Ok(())
}

fn restore_exact_stash(repo_path: &Path, stash_oid: &str) -> Result<String, String> {
	ensure_clean_worktree(repo_path)?;
	let position = stash_position(repo_path, stash_oid)?
		.ok_or_else(|| "The stash created by this task is no longer present; refusing to pop a different stash.".to_string())?;
	let apply = git_output(repo_path, &["stash", "apply", "--index", stash_oid])?;
	if !apply.status.success() {
		return Err(String::from_utf8_lossy(&apply.stderr).trim().to_string());
	}
	let reference = format!("stash@{{{position}}}");
	let drop = git_output(repo_path, &["stash", "drop", &reference])?;
	if drop.status.success() {
		Ok("Restored and removed the exact stash entry created by this task.".to_string())
	} else {
		Ok(format!(
			"Restored the exact stash, but Git could not remove its stash-list entry; it remains as a recovery copy. {}",
			String::from_utf8_lossy(&drop.stderr).trim()
		))
	}
}

fn commit_tree_oid(repo_path: &Path, oid: &str) -> Result<String, String> {
	git_text(repo_path, &["rev-parse", &format!("{oid}^{{tree}}")])
}

fn index_tree_oid(repo_path: &Path) -> Result<String, String> {
	git_text(repo_path, &["write-tree"])
}

fn undo_cache_dir_for_repo(repo_path: &Path) -> PathBuf {
	// Keep clone undo moves on the same volume as the cloned directory so
	// Windows and Unix can rename atomically instead of failing with EXDEV.
	repo_path.parent().unwrap_or_else(|| Path::new(".")).join(".gitmanager-undo-cache")
}

fn parse_git_progress_percent(line: &str) -> Option<(u8, String)> {
	let pct_pos = line.find('%')?;
	let prefix = &line[..pct_pos];
	let digits: String = prefix
		.chars()
		.rev()
		.take_while(|c| c.is_ascii_digit())
		.collect::<String>()
		.chars()
		.rev()
		.collect();
	let value = digits.parse::<u8>().ok()?.min(100);
	Some((value, line.trim().to_string()))
}

fn update_task_progress(repo_key: &str, task_id: u64, progress: u8, phase: String, log_line: Option<String>) {
	let mgr = manager();
	let mut lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
	if let Some(queue) = lanes.get_mut(repo_key) {
		if let Some(task) = queue.iter_mut().find(|t| t.id == task_id) {
			task.progress = progress;
			task.phase = phase;
			if let Some(line) = log_line {
				if task.logs.len() >= 40 {
					task.logs.remove(0);
				}
				task.logs.push(line);
			}
		}
	}
}

fn spawn_lane_worker_if_idle(repo_key: String) {
	let mgr = Arc::clone(manager());
	if mgr.paused.load(Ordering::Relaxed) {
		return;
	}
	{
		let mut active = mgr.active_lane_keys.lock().unwrap_or_else(|p| p.into_inner());
		if *active.get(&repo_key).unwrap_or(&false) {
			return;
		}
		active.insert(repo_key.clone(), true);
	}

	std::thread::spawn(move || {
		loop {
			if mgr.paused.load(Ordering::Relaxed) {
				break;
			}
			let next_task = {
				let mut lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
				let Some(queue) = lanes.get_mut(&repo_key) else {
					break;
				};
				if let Some(first) = queue.first_mut() {
					first.status = "running".to_string();
					first.progress = first.progress.max(8);
					first.phase = "Starting…".to_string();
					Some(first.clone())
				} else {
					lanes.remove(&repo_key);
					break;
				}
			};

			let Some(task) = next_task else { break };
			let (ok, summary, metadata) = execute_single_task(&repo_key, &task);

			// Remove finished task from active lane and append to history.
			{
				let mut lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
				if let Some(queue) = lanes.get_mut(&repo_key) {
					queue.retain(|t| t.id != task.id);
					if queue.is_empty() {
						lanes.remove(&repo_key);
					}
				}
			}

			{
				let mut history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
				// Overwrite protection: any previously reverted task on the same repo_key
				// is now superseded and cannot go forward blindly.
				if ok {
					for entry in history.iter_mut() {
						if entry.repo_key == repo_key && entry.status == "reverted" {
							entry.overwritten = true;
						}
					}
				}
				history.insert(
					0,
					TaskHistoryEntry {
						id: task.id,
						repo_key: repo_key.clone(),
						repo_name: task.repo_name.clone(),
						disk_path: task.disk_path.clone(),
						kind: task.kind.clone(),
						title: task.title.clone(),
						finished_at: now_epoch(),
						reversible: ok && task.reversible && metadata.reversible,
						status: if ok { "applied".to_string() } else { "failed".to_string() },
						overwritten: false,
						revert_note: summary,
						before_ref: metadata.before_ref,
						after_ref: metadata.after_ref,
						before_index_snapshot: metadata.before_index_snapshot,
						after_index_snapshot: metadata.after_index_snapshot,
						before_index_existed: metadata.before_index_existed,
						stash_oid: metadata.stash_oid,
						undo_cache_path: None,
					},
				);
				if history.len() > 60 {
					history.truncate(60);
				}
			}
		}

		// Release the lane while holding the queue lock. If enqueue raced with
		// worker shutdown, either the enqueuer or this worker will restart it.
		let lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
		let has_pending = lanes.get(&repo_key).map(|queue| !queue.is_empty()).unwrap_or(false);
		let mut active = mgr.active_lane_keys.lock().unwrap_or_else(|p| p.into_inner());
		active.remove(&repo_key);
		drop(active);
		drop(lanes);
		if has_pending && !mgr.paused.load(Ordering::Relaxed) {
			spawn_lane_worker_if_idle(repo_key.clone());
		}
	});
}

fn execute_single_task(repo_key: &str, task: &QueuedTaskItem) -> (bool, String, TaskExecutionMetadata) {
	let target_path = PathBuf::from(&task.disk_path);
	let mut metadata = TaskExecutionMetadata {
		before_ref: if target_path.is_dir() { current_head_oid(&target_path) } else { None },
		..TaskExecutionMetadata::default()
	};

	match task.kind.as_str() {
		"clone" => {
			let raw_url = task.args_payload.clone_url.as_deref().unwrap_or("");
			let url = expand_clone_shorthand(raw_url);
			if url.is_empty() {
				return (false, "Missing clone URL.".to_string(), metadata);
			}
			if let Some(parent) = target_path.parent() {
				let _ = std::fs::create_dir_all(parent);
			}
			let mut child = match process_utils::hidden_command("git")
				.args(["clone", "--progress", &url, &task.disk_path])
				.stdout(Stdio::null())
				.stderr(Stdio::piped())
				.spawn()
			{
				Ok(child) => child,
				Err(error) => return (false, format!("Failed to start git clone: {error}"), metadata),
			};
			if let Some(stderr) = child.stderr.take() {
				for line in BufReader::new(stderr).lines().map_while(Result::ok) {
					if let Some((percent, phase)) = parse_git_progress_percent(&line) {
						update_task_progress(repo_key, task.id, percent, phase, Some(line));
					} else if !line.trim().is_empty() {
						update_task_progress(repo_key, task.id, 30, line.trim().to_string(), Some(line));
					}
				}
			}
			match child.wait() {
				Ok(status) if status.success() => {
					metadata.reversible = true;
					(true, "Cloned repository — revert moves folder to safe undo cache".to_string(), metadata)
				}
				Ok(status) => (false, format!("git clone exited with status {status}"), metadata),
				Err(error) => (false, format!("git clone error: {error}"), metadata),
			}
		}
		"stage" => {
			let (before_exists, before_snapshot) = match capture_index_snapshot(&target_path, task.id, "before") {
				Ok(snapshot) => snapshot,
				Err(error) => return (false, error, metadata),
			};
			let mut args = vec!["add".to_string()];
			if task.args_payload.paths.is_empty() {
				args.push("-A".to_string());
			} else {
				args.extend(task.args_payload.paths.clone());
			}
			let refs: Vec<&str> = args.iter().map(String::as_str).collect();
			let (ok, note) = run_simple_git(repo_key, task, &target_path, &refs, "Staged selected files — Undo restores the exact previous index");
			if !ok {
				return (false, note, metadata);
			}
			let (after_exists, after_snapshot) = match capture_index_snapshot(&target_path, task.id, "after") {
				Ok(snapshot) => snapshot,
				Err(error) => {
					return (true, format!("Staging completed, but this task is not undoable because its post-stage index snapshot could not be saved: {error}"), metadata);
				}
			};
			metadata.before_index_existed = before_exists;
			metadata.before_index_snapshot = before_snapshot;
			metadata.after_index_snapshot = after_snapshot;
			metadata.reversible = after_exists;
			(true, note, metadata)
		}
		"commit" => {
			let message = task.args_payload.commit_message.as_deref()
				.filter(|message| !message.trim().is_empty())
				.unwrap_or("Update from GitManager");
			let (ok, note) = run_simple_git(repo_key, task, &target_path, &["commit", "-m", message], "Committed — Undo soft-resets to the recorded parent HEAD");
			if ok {
				metadata.after_ref = current_head_oid(&target_path);
				metadata.reversible = metadata.before_ref.is_some() && metadata.after_ref.is_some();
			}
			(ok, note, metadata)
		}
		"stash_push" => {
			let before_stash = current_stash_oid(&target_path);
			let message = format!("GitManager TaskManager task {}", task.id);
			let (ok, note) = run_simple_git(repo_key, task, &target_path, &["stash", "push", "-m", &message], "Stashed changes");
			if !ok {
				return (false, note, metadata);
			}
			let after_stash = current_stash_oid(&target_path);
			if after_stash.is_some() && after_stash != before_stash {
				metadata.stash_oid = after_stash;
				metadata.reversible = true;
				(true, "Stashed changes — Undo restores this exact stash entry".to_string(), metadata)
			} else {
				(true, "Nothing to stash; no stash entry was created.".to_string(), metadata)
			}
		}
		"pull" => run_simple_git(repo_key, task, &target_path, &["pull", "--ff-only"], "Fast-forward pull completed").pipe_metadata(&target_path, metadata),
		"fetch" => run_simple_git(repo_key, task, &target_path, &["fetch", "--all", "--prune"], "Fetched remote refs").pipe_metadata(&target_path, metadata),
		"push" => run_simple_git(repo_key, task, &target_path, &["push"], "Remote push — non-reversible").pipe_metadata(&target_path, metadata),
		"stash_pop" => run_simple_git(repo_key, task, &target_path, &["stash", "pop"], "Popped stash to working tree").pipe_metadata(&target_path, metadata),
		other => (false, format!("Unsupported task kind '{other}'"), metadata),
	}
}

trait TaskResultMetadata {
	fn pipe_metadata(self, repo_path: &Path, metadata: TaskExecutionMetadata) -> (bool, String, TaskExecutionMetadata);
}

impl TaskResultMetadata for (bool, String) {
	fn pipe_metadata(self, repo_path: &Path, mut metadata: TaskExecutionMetadata) -> (bool, String, TaskExecutionMetadata) {
		if self.0 {
			metadata.after_ref = current_head_oid(repo_path);
		}
		(self.0, self.1, metadata)
	}
}

fn run_simple_git(
	repo_key: &str,
	task: &QueuedTaskItem,
	cwd: &Path,
	args: &[&str],
	ok_note: &str,
) -> (bool, String) {
	update_task_progress(repo_key, task.id, 35, format!("Running git {}…", args.first().copied().unwrap_or("")), None);
	let output = match git_output(cwd, args) {
		Ok(output) => output,
		Err(error) => return (false, error),
	};
	let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
	let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
	if output.status.success() {
		update_task_progress(repo_key, task.id, 100, "Done".to_string(), if stdout.is_empty() { None } else { Some(stdout) });
		(true, ok_note.to_string())
	} else {
		(false, if !stderr.is_empty() { stderr } else { stdout })
	}
}

/// Returns the live snapshot of all per-repo task queues and reversible history.
#[tauri::command]
pub async fn get_task_manager_snapshot() -> Result<TaskManagerSnapshot, String> {
	let mgr = manager();
	let paused = mgr.paused.load(Ordering::Relaxed);
	let lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
	let active_lanes = mgr.active_lane_keys.lock().unwrap_or_else(|p| p.into_inner()).len();
	let history = mgr.history.lock().unwrap_or_else(|p| p.into_inner()).clone();

	let mut groups = Vec::new();
	let mut total_running = 0usize;
	let mut total_queued = 0usize;

	for (repo_key, tasks) in lanes.iter() {
		if tasks.is_empty() {
			continue;
		}
		let running_count = tasks.iter().filter(|t| t.status == "running").count();
		let queued_count = tasks.iter().filter(|t| t.status == "queued").count();
		total_running += running_count;
		total_queued += queued_count;
		groups.push(RepoTaskGroup {
			repo_key: repo_key.clone(),
			repo_name: tasks[0].repo_name.clone(),
			disk_path: tasks[0].disk_path.clone(),
			running_count,
			queued_count,
			tasks: tasks.clone(),
		});
	}
	groups.sort_by(|a, b| a.repo_name.to_lowercase().cmp(&b.repo_name.to_lowercase()));

	Ok(TaskManagerSnapshot {
		paused,
		active_lanes,
		total_running,
		total_queued,
		groups,
		history,
	})
}

/// Queues a Git task into the target repository's serialized disk-path lane.
#[tauri::command]
pub async fn enqueue_repo_task(request: EnqueueTaskRequest) -> Result<u64, String> {
	let kind = request.kind.trim().to_lowercase();
	if !matches!(kind.as_str(), "clone" | "stage" | "commit" | "push" | "pull" | "fetch" | "stash_push" | "stash_pop") {
		return Err(format!("Unsupported task kind '{kind}'."));
	}
	if request.disk_path.trim().is_empty() {
		return Err("A repository disk path is required.".to_string());
	}
	if kind == "clone" && request.clone_url.as_deref().map(str::trim).unwrap_or("").is_empty() {
		return Err("A clone URL is required for clone tasks.".to_string());
	}
	let mgr = manager();
	let id = mgr.next_id.fetch_add(1, Ordering::Relaxed);
	let repo_key = normalize_repo_key(&request.disk_path);
	let reversible = matches!(kind.as_str(), "clone" | "commit" | "stage" | "stash_push");
	let title = request.title.unwrap_or_else(|| match kind.as_str() {
		"clone" => format!("Cloning {}", request.clone_url.as_deref().unwrap_or(&request.repo_name)),
		"pull" => format!("Pulling {}", request.repo_name),
		"push" => format!("Pushing {}", request.repo_name),
		"fetch" => format!("Fetching {}", request.repo_name),
		"stage" => format!("Staging changes in {}", request.repo_name),
		"commit" => format!("Committing {}", request.repo_name),
		"stash_push" => format!("Stashing changes in {}", request.repo_name),
		"stash_pop" => format!("Popping stash in {}", request.repo_name),
		_ => format!("Task on {}", request.repo_name),
	});
	let command_preview = match kind.as_str() {
		"clone" => format!("gitm clone {} {}", request.clone_url.as_deref().unwrap_or(""), request.disk_path),
		"pull" => "git pull --ff-only".to_string(),
		"push" => "git push".to_string(),
		"fetch" => "git fetch --all --prune".to_string(),
		"stage" => "git add -A".to_string(),
		"commit" => format!("git commit -m \"{}\"", request.commit_message.as_deref().unwrap_or("Update")),
		"stash_push" => "git stash push".to_string(),
		"stash_pop" => "git stash pop".to_string(),
		_ => kind.clone(),
	};

	let item = QueuedTaskItem {
		id,
		repo_key: repo_key.clone(),
		repo_name: request.repo_name,
		disk_path: request.disk_path,
		kind,
		title,
		command_preview,
		status: "queued".to_string(),
		progress: 0,
		phase: "Queued in repository lane".to_string(),
		reversible,
		logs: Vec::new(),
		args_payload: TaskPayload {
			clone_url: request.clone_url,
			commit_message: request.commit_message,
			branch: request.branch,
			paths: request.paths.unwrap_or_default(),
		},
	};

	{
		let mut lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
		lanes.entry(repo_key.clone()).or_default().push(item);
	}
	spawn_lane_worker_if_idle(repo_key);
	Ok(id)
}

/// Cancels a queued task by ID.
#[tauri::command]
pub async fn cancel_repo_task(task_id: u64) -> Result<(), String> {
	let mgr = manager();
	let mut lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
	lanes.retain(|_, queue| {
		queue.retain(|t| t.id != task_id || t.status == "running");
		!queue.is_empty()
	});
	Ok(())
}

/// Moves a queued task one slot earlier in its repository lane (never ahead of the running #1 task).
#[tauri::command]
pub async fn reorder_repo_task(repo_key: String, task_id: u64) -> Result<(), String> {
	let mgr = manager();
	let mut lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
	if let Some(queue) = lanes.get_mut(&repo_key) {
		if let Some(pos) = queue.iter().position(|t| t.id == task_id) {
			if pos > 1 && queue[pos].status == "queued" {
				queue.swap(pos - 1, pos);
			}
		}
	}
	Ok(())
}

/// Pauses or resumes dispatching new tasks across all lanes.
#[tauri::command]
pub async fn set_task_queue_paused(paused: bool) -> Result<bool, String> {
	let mgr = manager();
	mgr.paused.store(paused, Ordering::Relaxed);
	if !paused {
		let keys: Vec<String> = {
			let lanes = mgr.lanes.lock().unwrap_or_else(|p| p.into_inner());
			lanes.keys().cloned().collect()
		};
		for key in keys {
			spawn_lane_worker_if_idle(key);
		}
	}
	Ok(paused)
}

/// Reverts a completed task only when the exact Git state produced by that task is still present.
#[tauri::command]
pub async fn revert_history_task(task_id: u64) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let mgr = manager();
		let entry = {
			let history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
			history.iter().find(|history| history.id == task_id).cloned()
				.ok_or_else(|| "Task history entry not found.".to_string())?
		};
		if !entry.reversible || entry.status != "applied" {
			return Err("This task cannot be reverted.".to_string());
		}
		let _lane_guard = reserve_history_operation(&entry.repo_key)?;
		let disk_path = PathBuf::from(&entry.disk_path);
		let mut cache_path_saved: Option<String> = None;
		let note = match entry.kind.as_str() {
			"clone" => {
				if !disk_path.exists() {
					return Err("Cloned directory no longer exists on disk.".to_string());
				}
				let cache_root = undo_cache_dir_for_repo(&disk_path);
				std::fs::create_dir_all(&cache_root).map_err(|error| format!("Could not prepare undo cache: {error}"))?;
				let destination = unique_undo_cache_path(&cache_root, entry.id);
				if destination.exists() {
					return Err("A unique undo-cache destination already exists; refusing to overwrite it.".to_string());
				}
				std::fs::rename(&disk_path, &destination)
					.map_err(|error| format!("Could not move cloned repo to undo cache: {error}"))?;
				cache_path_saved = Some(destination.to_string_lossy().to_string());
				"Moved cloned repository to its undo cache.".to_string()
			}
			"commit" => {
				let Some(after) = entry.after_ref.as_deref() else {
					return Err("The commit OID created by this task was not recorded; refusing an unsafe reset.".to_string());
				};
				let Some(before) = entry.before_ref.as_deref() else {
					return Err("The parent HEAD for this commit was not recorded.".to_string());
				};
				if current_head_oid(&disk_path).as_deref() != Some(after) {
					return Err("HEAD has moved since this commit; refusing to reset newer commits.".to_string());
				}
				if index_tree_oid(&disk_path)? != commit_tree_oid(&disk_path, after)? {
					return Err("The index has changed since this commit; refusing to disturb newer staged work.".to_string());
				}
				let output = git_output(&disk_path, &["reset", "--soft", before])?;
				if !output.status.success() {
					return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
				}
				"Soft-reset the exact commit created by this task.".to_string()
			}
			"stage" => {
				restore_index_snapshot(
					&disk_path,
					entry.after_index_snapshot.as_deref(),
					true,
					entry.before_index_snapshot.as_deref(),
					entry.before_index_existed,
				)?;
				"Restored the exact index from before this staging task; working-tree files were left untouched.".to_string()
			}
			"stash_push" => {
				let Some(stash_oid) = entry.stash_oid.as_deref() else {
					return Err("The exact stash OID created by this task was not recorded.".to_string());
				};
				ensure_clean_worktree(&disk_path)?;
				let position = stash_position(&disk_path, stash_oid)?
					.ok_or_else(|| "The stash created by this task is no longer present; refusing to pop a different stash.".to_string())?;
				let apply = git_output(&disk_path, &["stash", "apply", "--index", stash_oid])?;
				if !apply.status.success() {
					return Err(String::from_utf8_lossy(&apply.stderr).trim().to_string());
				}
				let reference = format!("stash@{{{position}}}");
				let drop = git_output(&disk_path, &["stash", "drop", &reference])?;
				if drop.status.success() {
					"Restored and removed the exact stash entry created by this task.".to_string()
				} else {
					format!(
						"Restored the exact stash, but Git could not remove its stash-list entry; it remains as a recovery copy. {}",
						String::from_utf8_lossy(&drop.stderr).trim()
					)
				}
			}
			_ => return Err("Reverting this task kind is not supported.".to_string()),
		};
		let mut history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
		if let Some(item) = history.iter_mut().find(|item| item.id == task_id) {
			item.status = "reverted".to_string();
			item.undo_cache_path = cache_path_saved;
			item.revert_note = note.clone();
		}
		Ok(format!("Reverted: {} — {}", entry.title, note))
	})
	.await
	.map_err(|error| format!("Revert task failed: {error}"))?
}

/// Reapplies a reverted task only when the exact pre-task state is still present.
#[tauri::command]
pub async fn redo_history_task(task_id: u64) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let mgr = manager();
		let entry = {
			let history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
			history.iter().find(|history| history.id == task_id).cloned()
				.ok_or_else(|| "Task history entry not found.".to_string())?
		};
		if entry.status != "reverted" {
			return Err("Task is not in a reverted state.".to_string());
		}
		if entry.overwritten {
			return Err("Cannot go forward: a newer TaskManager operation has overwritten this repository state.".to_string());
		}
		let _lane_guard = reserve_history_operation(&entry.repo_key)?;
		let disk_path = PathBuf::from(&entry.disk_path);
		let mut replacement_stash_oid: Option<String> = None;
		let note = match entry.kind.as_str() {
			"clone" => {
				if disk_path.exists() {
					let mut history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
					if let Some(item) = history.iter_mut().find(|item| item.id == task_id) {
						item.overwritten = true;
					}
					return Err("Cannot restore clone: another folder already occupies that disk path.".to_string());
				}
				let Some(cache_path) = entry.undo_cache_path.as_deref() else {
					return Err("Undo cache for this clone is no longer available.".to_string());
				};
				std::fs::rename(cache_path, &disk_path)
					.map_err(|error| format!("Could not restore cloned repository: {error}"))?;
				"Restored the cloned repository from its undo cache.".to_string()
			}
			"commit" => {
				let Some(before) = entry.before_ref.as_deref() else {
					return Err("The original parent HEAD was not recorded.".to_string());
				};
				let Some(after) = entry.after_ref.as_deref() else {
					return Err("The commit OID created by this task was not recorded.".to_string());
				};
				if current_head_oid(&disk_path).as_deref() != Some(before) {
					return Err("HEAD no longer matches the state produced by Undo; refusing to move it.".to_string());
				}
				if index_tree_oid(&disk_path)? != commit_tree_oid(&disk_path, after)? {
					return Err("The staged index no longer matches the original commit; refusing to redo over newer staged work.".to_string());
				}
				let output = git_output(&disk_path, &["reset", "--soft", after])?;
				if !output.status.success() {
					return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
				}
				"Restored the exact commit created by this task.".to_string()
			}
			"stage" => {
				restore_index_snapshot(
					&disk_path,
					entry.before_index_snapshot.as_deref(),
					entry.before_index_existed,
					entry.after_index_snapshot.as_deref(),
					true,
				)?;
				"Restaged the exact index captured after the original task; working-tree files were left untouched.".to_string()
			}
			"stash_push" => {
				ensure_clean_worktree(&disk_path)?;
				let before_stash = current_stash_oid(&disk_path);
				let message = format!("GitManager TaskManager redo {}", entry.id);
				let output = git_output(&disk_path, &["stash", "push", "-m", &message])?;
				if !output.status.success() {
					return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
				}
				let after_stash = current_stash_oid(&disk_path);
				if after_stash.is_none() || after_stash == before_stash {
					return Err("Redo did not create a new stash entry; the working tree may contain no changes.".to_string());
				}
				replacement_stash_oid = after_stash;
				"Re-stashed the restored changes and recorded the new exact stash OID.".to_string()
			}
			_ => return Err("Redo forward for this task type is not supported.".to_string()),
		};
		let mut history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
		if let Some(item) = history.iter_mut().find(|item| item.id == task_id) {
			item.status = "applied".to_string();
			item.undo_cache_path = None;
			if replacement_stash_oid.is_some() {
				item.stash_oid = replacement_stash_oid;
			}
			item.revert_note = note.clone();
		}
		Ok(format!("Restored forward: {} — {}", entry.title, note))
	})
	.await
	.map_err(|error| format!("Redo task failed: {error}"))?
}

#[cfg(test)]
mod task_manager_tests {
	use super::*;
	use std::fs;
	use std::time::{SystemTime, UNIX_EPOCH};
	fn cleanup_snapshot(path: Option<&str>) {
		if let Some(path) = path {
			let snapshot = PathBuf::from(path);
			let _ = fs::remove_file(&snapshot);
			if let Some(parent) = snapshot.parent() {
				let _ = fs::remove_dir(parent);
			}
		}
	}


	#[cfg(not(windows))]
	#[test]
	fn repository_keys_preserve_case_on_case_sensitive_platforms() {
		assert_ne!(normalize_repo_key("/tmp/Repo"), normalize_repo_key("/tmp/repo"));
	}

	#[test]
	fn undo_cache_paths_do_not_reuse_task_ids() {
		let nonce = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap_or_default()
			.as_nanos();
		let root = std::env::temp_dir().join(format!("gitmanager-undo-cache-test-{nonce}"));
		fs::create_dir_all(&root).unwrap();

		let first = unique_undo_cache_path(&root, 1);
		fs::create_dir_all(&first).unwrap();
		fs::write(first.join("sentinel.txt"), "keep me").unwrap();
		let second = unique_undo_cache_path(&root, 1);

		assert_ne!(first, second);
		assert_eq!(fs::read_to_string(first.join("sentinel.txt")).unwrap(), "keep me");

		fs::remove_dir_all(root).unwrap();
	}
	fn init_test_repo() -> PathBuf {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let root = std::env::temp_dir().join(format!("gitmanager-task-undo-test-{nonce}"));
		fs::create_dir_all(&root).unwrap();
		assert!(git_output(&root, &["init", "-q"]).unwrap().status.success());
		assert!(git_output(&root, &["config", "user.name", "GitManager Tests"]).unwrap().status.success());
		assert!(git_output(&root, &["config", "user.email", "gitmanager-tests@example.invalid"]).unwrap().status.success());
		fs::write(root.join("tracked.txt"), "initial\\n").unwrap();
		assert!(git_output(&root, &["add", "tracked.txt"]).unwrap().status.success());
		assert!(git_output(&root, &["commit", "-m", "initial"]).unwrap().status.success());
		root
	}

	#[test]
	fn stage_undo_and_redo_restore_only_the_exact_index_snapshot() {
		let repo = init_test_repo();
		fs::write(repo.join("tracked.txt"), "modified\\n").unwrap();
		assert!(git_output(&repo, &["add", "tracked.txt"]).unwrap().status.success());
		fs::write(repo.join("second.txt"), "still present\\n").unwrap();
		let (before_exists, before) = capture_index_snapshot(&repo, 101, "before").unwrap();
		assert!(git_output(&repo, &["add", "second.txt"]).unwrap().status.success());
		let (after_exists, after) = capture_index_snapshot(&repo, 101, "after").unwrap();
		restore_index_snapshot(&repo, after.as_deref(), after_exists, before.as_deref(), before_exists).unwrap();
		assert_eq!(git_text(&repo, &["diff", "--cached", "--name-only"]).unwrap(), "tracked.txt");
		assert_eq!(fs::read_to_string(repo.join("second.txt")).unwrap(), "still present\\n");
		restore_index_snapshot(&repo, before.as_deref(), before_exists, after.as_deref(), after_exists).unwrap();
		assert_eq!(git_text(&repo, &["diff", "--cached", "--name-only"]).unwrap().lines().collect::<Vec<_>>(), vec!["second.txt", "tracked.txt"]);
		cleanup_snapshot(before.as_deref());
		cleanup_snapshot(after.as_deref());
		fs::remove_dir_all(repo).unwrap();
	}

	#[test]
	fn stage_undo_refuses_to_overwrite_newer_staged_work() {
		let repo = init_test_repo();
		fs::write(repo.join("tracked.txt"), "task change\\n").unwrap();
		let (before_exists, before) = capture_index_snapshot(&repo, 102, "before").unwrap();
		assert!(git_output(&repo, &["add", "tracked.txt"]).unwrap().status.success());
		let (after_exists, after) = capture_index_snapshot(&repo, 102, "after").unwrap();
		fs::write(repo.join("tracked.txt"), "external change\\n").unwrap();
		assert!(git_output(&repo, &["add", "tracked.txt"]).unwrap().status.success());
		let result = restore_index_snapshot(&repo, after.as_deref(), after_exists, before.as_deref(), before_exists);
		assert!(result.unwrap_err().contains("index has changed"));
		cleanup_snapshot(before.as_deref());
		cleanup_snapshot(after.as_deref());
		fs::remove_dir_all(repo).unwrap();
	}

	#[test]
	fn stash_undo_restores_the_recorded_oid_not_the_newer_top_stash() {
		let repo = init_test_repo();
		fs::write(repo.join("tracked.txt"), "task stash\\n").unwrap();
		assert!(git_output(&repo, &["stash", "push", "-m", "task stash"]).unwrap().status.success());
		let task_stash = current_stash_oid(&repo).unwrap();
		fs::write(repo.join("tracked.txt"), "newer unrelated stash\\n").unwrap();
		assert!(git_output(&repo, &["stash", "push", "-m", "newer stash"]).unwrap().status.success());
		let newer_stash = current_stash_oid(&repo).unwrap();
		assert_ne!(task_stash, newer_stash);
		let note = restore_exact_stash(&repo, &task_stash).unwrap();
		assert!(note.contains("exact stash"));
		assert_eq!(fs::read_to_string(repo.join("tracked.txt")).unwrap(), "task stash\\n");
		assert!(stash_position(&repo, &newer_stash).unwrap().is_some());
		assert!(stash_position(&repo, &task_stash).unwrap().is_none());
		fs::remove_dir_all(repo).unwrap();
	}

	fn init_test_repo() -> PathBuf {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let root = std::env::temp_dir().join(format!("gitmanager-task-undo-test-{nonce}"));
		fs::create_dir_all(&root).unwrap();
		assert!(git_output(&root, &["init", "-q"]).unwrap().status.success());
		assert!(git_output(&root, &["config", "user.name", "GitManager Tests"]).unwrap().status.success());
		assert!(git_output(&root, &["config", "user.email", "gitmanager-tests@example.invalid"]).unwrap().status.success());
		fs::write(root.join("tracked.txt"), "initial\\n").unwrap();
		assert!(git_output(&root, &["add", "tracked.txt"]).unwrap().status.success());
		assert!(git_output(&root, &["commit", "-m", "initial"]).unwrap().status.success());
		root
	}

	#[test]
	fn stage_undo_and_redo_restore_only_the_exact_index_snapshot() {
		let repo = init_test_repo();
		fs::write(repo.join("tracked.txt"), "modified\\n").unwrap();
		assert!(git_output(&repo, &["add", "tracked.txt"]).unwrap().status.success());
		fs::write(repo.join("second.txt"), "still present\\n").unwrap();
		let (before_exists, before) = capture_index_snapshot(&repo, 101, "before").unwrap();
		assert!(git_output(&repo, &["add", "second.txt"]).unwrap().status.success());
		let (after_exists, after) = capture_index_snapshot(&repo, 101, "after").unwrap();
		restore_index_snapshot(&repo, after.as_deref(), after_exists, before.as_deref(), before_exists).unwrap();
		assert_eq!(git_text(&repo, &["diff", "--cached", "--name-only"]).unwrap(), "tracked.txt");
		assert_eq!(fs::read_to_string(repo.join("second.txt")).unwrap(), "still present\\n");
		restore_index_snapshot(&repo, before.as_deref(), before_exists, after.as_deref(), after_exists).unwrap();
		assert_eq!(git_text(&repo, &["diff", "--cached", "--name-only"]).unwrap().lines().collect::<Vec<_>>(), vec!["second.txt", "tracked.txt"]);
		fs::remove_dir_all(repo).unwrap();
	}

	#[test]
	fn stage_undo_refuses_to_overwrite_newer_staged_work() {
		let repo = init_test_repo();
		fs::write(repo.join("tracked.txt"), "task change\\n").unwrap();
		let (before_exists, before) = capture_index_snapshot(&repo, 102, "before").unwrap();
		assert!(git_output(&repo, &["add", "tracked.txt"]).unwrap().status.success());
		let (after_exists, after) = capture_index_snapshot(&repo, 102, "after").unwrap();
		fs::write(repo.join("tracked.txt"), "external change\\n").unwrap();
		assert!(git_output(&repo, &["add", "tracked.txt"]).unwrap().status.success());
		let result = restore_index_snapshot(&repo, after.as_deref(), after_exists, before.as_deref(), before_exists);
		assert!(result.unwrap_err().contains("index has changed"));
		fs::remove_dir_all(repo).unwrap();
	}

	#[test]
	fn stash_undo_restores_the_recorded_oid_not_the_newer_top_stash() {
		let repo = init_test_repo();
		fs::write(repo.join("tracked.txt"), "task stash\\n").unwrap();
		assert!(git_output(&repo, &["stash", "push", "-m", "task stash"]).unwrap().status.success());
		let task_stash = current_stash_oid(&repo).unwrap();
		fs::write(repo.join("tracked.txt"), "newer unrelated stash\\n").unwrap();
		assert!(git_output(&repo, &["stash", "push", "-m", "newer stash"]).unwrap().status.success());
		let newer_stash = current_stash_oid(&repo).unwrap();
		assert_ne!(task_stash, newer_stash);
		let note = restore_exact_stash(&repo, &task_stash).unwrap();
		assert!(note.contains("exact stash"));
		assert_eq!(fs::read_to_string(repo.join("tracked.txt")).unwrap(), "task stash\\n");
		assert!(stash_position(&repo, &newer_stash).unwrap().is_some());
		assert!(stash_position(&repo, &task_stash).unwrap().is_none());
		fs::remove_dir_all(repo).unwrap();
	}

}
