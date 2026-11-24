use std::collections::HashMap;
use std::io::{BufRead, BufReader};
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
	let normalized = disk_path.trim().replace('\\\\', "/");
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
			let (ok, summary, before_ref) = execute_single_task(&repo_key, &task);

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
						reversible: ok && task.reversible,
						status: if ok { "applied".to_string() } else { "failed".to_string() },
						overwritten: false,
						revert_note: summary,
						before_ref,
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

fn execute_single_task(repo_key: &str, task: &QueuedTaskItem) -> (bool, String, Option<String>) {
	let target_path = PathBuf::from(&task.disk_path);
	let before_ref = if target_path.is_dir() {
		current_head_oid(&target_path)
	} else {
		None
	};

	match task.kind.as_str() {
		"clone" => {
			let raw_url = task.args_payload.clone_url.as_deref().unwrap_or("");
			let url = expand_clone_shorthand(raw_url);
			if url.is_empty() {
				return (false, "Missing clone URL.".to_string(), None);
			}
			if let Some(parent) = target_path.parent() {
				let _ = std::fs::create_dir_all(parent);
			}
			let mut child = match process_utils::hidden_command("git")
				.args(["clone", "--progress", &url, &task.disk_path])
				// Git's progress stream is written to stderr. Discard stdout so a
				// child cannot block forever on an unread stdout pipe.
				.stdout(Stdio::null())
				.stderr(Stdio::piped())
				.spawn()
			{
				Ok(c) => c,
				Err(err) => return (false, format!("Failed to start git clone: {err}"), None),
			};

			if let Some(stderr) = child.stderr.take() {
				let reader = BufReader::new(stderr);
				for line in reader.lines().map_while(Result::ok) {
					if let Some((pct, phase)) = parse_git_progress_percent(&line) {
						update_task_progress(repo_key, task.id, pct, phase, Some(line));
					} else if !line.trim().is_empty() {
						update_task_progress(repo_key, task.id, 30, line.trim().to_string(), Some(line));
					}
				}
			}
			match child.wait() {
				Ok(st) if st.success() => (
					true,
					"Cloned repository — revert moves folder to safe undo cache".to_string(),
					None,
				),
				Ok(st) => (false, format!("git clone exited with status {st}"), None),
				Err(err) => (false, format!("git clone error: {err}"), None),
			}
		}
		"pull" => run_simple_git(repo_key, task, &target_path, &["pull", "--ff-only"], "Fast-forward pull completed", before_ref),
		"fetch" => run_simple_git(repo_key, task, &target_path, &["fetch", "--all", "--prune"], "Fetched remote refs", before_ref),
		"push" => run_simple_git(repo_key, task, &target_path, &["push"], "Remote push — non-reversible", before_ref),
		"stage" => {
			let mut args = vec!["add".to_string()];
			if task.args_payload.paths.is_empty() {
				args.push("-A".to_string());
			} else {
				args.extend(task.args_payload.paths.clone());
			}
			let refs: Vec<&str> = args.iter().map(String::as_str).collect();
			run_simple_git(repo_key, task, &target_path, &refs, "Staged files — revert unstages", before_ref)
		}
		"commit" => {
			let msg = task
				.args_payload
				.commit_message
				.as_deref()
				.filter(|m| !m.trim().is_empty())
				.unwrap_or("Update from GitManager");
			run_simple_git(repo_key, task, &target_path, &["commit", "-m", msg], "Committed — revert soft-resets to parent HEAD", before_ref)
		}
		"stash_push" => run_simple_git(repo_key, task, &target_path, &["stash", "push", "-m", "GitManager TaskManager"], "Stashed changes — revert pops stash", before_ref),
		"stash_pop" => run_simple_git(repo_key, task, &target_path, &["stash", "pop"], "Popped stash to working tree", before_ref),
		other => (false, format!("Unsupported task kind '{other}'"), before_ref),
	}
}

fn run_simple_git(
	repo_key: &str,
	task: &QueuedTaskItem,
	cwd: &Path,
	args: &[&str],
	ok_note: &str,
	before_ref: Option<String>,
) -> (bool, String, Option<String>) {
	update_task_progress(repo_key, task.id, 35, format!("Running git {}…", args.first().copied().unwrap_or("")), None);
	let out = match process_utils::hidden_command("git")
		.args(args)
		.current_dir(cwd)
		.output()
	{
		Ok(o) => o,
		Err(err) => return (false, format!("Could not execute git: {err}"), before_ref),
	};
	let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
	let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
	if out.status.success() {
		update_task_progress(repo_key, task.id, 100, "Done".to_string(), if stdout.is_empty() { None } else { Some(stdout) });
		(true, ok_note.to_string(), before_ref)
	} else {
		let detail = if !stderr.is_empty() { stderr } else { stdout };
		(false, detail, before_ref)
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

/// Safely reverts a reversible completed task (e.g. moves cloned repo to undo cache or soft-resets commit).
#[tauri::command]
pub async fn revert_history_task(task_id: u64) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let mgr = manager();
		let entry = {
			let history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
			history
				.iter()
				.find(|h| h.id == task_id)
				.cloned()
				.ok_or_else(|| "Task history entry not found.".to_string())?
		};
		if !entry.reversible || entry.status != "applied" {
			return Err("This task cannot be reverted.".to_string());
		}

		let disk_path = PathBuf::from(&entry.disk_path);
		let mut cache_path_saved: Option<String> = None;

		match entry.kind.as_str() {
			"clone" => {
				if !disk_path.exists() {
					return Err("Cloned directory no longer exists on disk.".to_string());
				}
				let cache_root = undo_cache_dir();
				std::fs::create_dir_all(&cache_root)
					.map_err(|err| format!("Could not prepare undo cache: {err}"))?;
				let dest = unique_undo_cache_path(&cache_root, entry.id);
				if dest.exists() {
					return Err("A unique undo-cache destination already exists; refusing to overwrite it.".to_string());
				}
				std::fs::rename(&disk_path, &dest)
					.map_err(|err| format!("Could not move cloned repo to undo cache: {err}"))?;
				cache_path_saved = Some(dest.to_string_lossy().to_string());
			}
			"commit" => {
				let Some(ref before) = entry.before_ref else {
					return Err("No parent commit recorded for soft reset.".to_string());
				};
				let out = process_utils::hidden_command("git")
					.args(["reset", "--soft", before])
					.current_dir(&disk_path)
					.output()
					.map_err(|err| format!("Could not soft-reset commit: {err}"))?;
				if !out.status.success() {
					return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
				}
			}
			"stage" => {
				let out = process_utils::hidden_command("git")
					.args(["reset"])
					.current_dir(&disk_path)
					.output()
					.map_err(|err| format!("Could not unstage files: {err}"))?;
				if !out.status.success() {
					return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
				}
			}
			"stash_push" => {
				let out = process_utils::hidden_command("git")
					.args(["stash", "pop"])
					.current_dir(&disk_path)
					.output()
					.map_err(|err| format!("Could not pop stashed changes: {err}"))?;
				if !out.status.success() {
					return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
				}
			}
			_ => return Err("Reverting this task kind is not supported.".to_string()),
		}

		let mut history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
		if let Some(h) = history.iter_mut().find(|h| h.id == task_id) {
			h.status = "reverted".to_string();
			h.undo_cache_path = cache_path_saved;
		}
		Ok(format!("Reverted: {}", entry.title))
	})
	.await
	.map_err(|err| format!("Revert task failed: {err}"))?
}

/// Restores a previously reverted task forward only if no newer task has overwritten its disk/Git state.
#[tauri::command]
pub async fn redo_history_task(task_id: u64) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let mgr = manager();
		let entry = {
			let history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
			history
				.iter()
				.find(|h| h.id == task_id)
				.cloned()
				.ok_or_else(|| "Task history entry not found.".to_string())?
		};
		if entry.status != "reverted" {
			return Err("Task is not in a reverted state.".to_string());
		}
		if entry.overwritten {
			return Err("Cannot go forward: a newer task has overwritten this repository state.".to_string());
		}

		let disk_path = PathBuf::from(&entry.disk_path);
		if entry.kind == "clone" {
			if disk_path.exists() {
				let mut history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
				if let Some(h) = history.iter_mut().find(|h| h.id == task_id) {
					h.overwritten = true;
				}
				return Err("Cannot restore clone: another folder already occupies that disk path.".to_string());
			}
			let Some(ref cache_path) = entry.undo_cache_path else {
				return Err("Undo cache for this clone is no longer available.".to_string());
			};
			std::fs::rename(cache_path, &disk_path)
				.map_err(|err| format!("Could not restore cloned repository: {err}"))?;
		} else if entry.kind == "stash_push" {
			let out = process_utils::hidden_command("git")
				.args(["stash", "push", "-m", "GitManager TaskManager Redo"])
				.current_dir(&disk_path)
				.output()
				.map_err(|err| format!("Could not re-stash changes: {err}"))?;
			if !out.status.success() {
				return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
			}
		} else {
			return Err("Redo forward for this task type requires re-running the action.".to_string());
		}

		let mut history = mgr.history.lock().unwrap_or_else(|p| p.into_inner());
		if let Some(h) = history.iter_mut().find(|h| h.id == task_id) {
			h.status = "applied".to_string();
			h.undo_cache_path = None;
		}
		Ok(format!("Restored forward: {}", entry.title))
	})
	.await
	.map_err(|err| format!("Redo task failed: {err}"))?
}

#[cfg(test)]
mod task_manager_tests {
	use super::{normalize_repo_key, unique_undo_cache_path};
	use std::fs;
	use std::time::{SystemTime, UNIX_EPOCH};

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
}
