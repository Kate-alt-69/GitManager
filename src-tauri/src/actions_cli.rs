use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::process_utils;

/// Live installation & authentication status for an official provider CLI (`gh` or `glab`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCliStatus {
	pub provider: String,
	pub binary: String,
	pub installed: bool,
	pub version: String,
	pub authenticated: bool,
	pub account_summary: String,
}

/// Metadata parsed from a local CI workflow or pipeline definition file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInputField {
	pub name: String,
	pub description: String,
	pub default_value: String,
	pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoWorkflowFile {
	pub provider: String,
	pub name: String,
	pub file_name: String,
	pub relative_path: String,
	pub supports_dispatch: bool,
	pub inputs: Vec<WorkflowInputField>,
}

/// Normalized CI run entry returned from either GitHub Actions (`gh run list`)
/// or GitLab CI (`glab ci list`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunEntry {
	pub id: String,
	pub provider: String,
	pub workflow_name: String,
	pub title: String,
	pub status: String,
	pub conclusion: String,
	pub branch: String,
	pub sha: String,
	pub created_at: String,
	pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunsResponse {
	pub provider: String,
	pub cli_available: bool,
	pub note: String,
	pub runs: Vec<WorkflowRunEntry>,
}

fn probe_cli_version(binary: &str) -> Option<String> {
	let output = process_utils::hidden_command(binary)
		.arg("--version")
		.output()
		.ok()?;
	if !output.status.success() {
		return None;
	}
	let first_line = String::from_utf8_lossy(&output.stdout)
		.lines()
		.next()
		.unwrap_or("")
		.trim()
		.to_string();
	Some(first_line)
}

fn probe_cli_auth_summary(binary: &str) -> (bool, String) {
	let output = match process_utils::hidden_command(binary)
		.args(["auth", "status"])
		.output()
	{
		Ok(out) => out,
		Err(err) => return (false, format!("Could not probe {binary}: {err}")),
	};
	let combined = format!(
		"{}\n{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
	let summary = combined
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.take(3)
		.collect::<Vec<_>>()
		.join(" · ");
	(output.status.success(), summary)
}

/// Checks whether `gh` (GitHub CLI) and `glab` (GitLab CLI) are installed and signed in.
#[tauri::command]
pub async fn get_cli_auth_status() -> Result<Vec<ProviderCliStatus>, String> {
	tokio::task::spawn_blocking(|| {
		let mut list = Vec::new();
		for (provider, binary) in [("github", "gh"), ("gitlab", "glab")] {
			if let Some(version) = probe_cli_version(binary) {
				let (authenticated, account_summary) = probe_cli_auth_summary(binary);
				list.push(ProviderCliStatus {
					provider: provider.to_string(),
					binary: binary.to_string(),
					installed: true,
					version,
					authenticated,
					account_summary,
				});
			} else {
				list.push(ProviderCliStatus {
					provider: provider.to_string(),
					binary: binary.to_string(),
					installed: false,
					version: String::new(),
					authenticated: false,
					account_summary: format!("{binary} CLI is not installed on PATH."),
				});
			}
		}
		Ok(list)
	})
	.await
	.map_err(|err| format!("CLI auth status probe failed: {err}"))?
}

/// Authenticates `gh` or `glab` by piping a Personal Access Token over stdin into the OS keyring.
#[tauri::command]
pub async fn cli_auth_login_token(
	provider: String,
	hostname: Option<String>,
	token: String,
) -> Result<String, String> {
	let trimmed_token = token.trim().to_string();
	if trimmed_token.is_empty() {
		return Err("Please enter a Personal Access Token.".to_string());
	}
	tokio::task::spawn_blocking(move || {
		let provider_norm = provider.trim().to_lowercase();
		let (binary, default_host, token_flag) = match provider_norm.as_str() {
			"github" => ("gh", "github.com", "--with-token"),
			"gitlab" => ("glab", "gitlab.com", "--stdin"),
			other => return Err(format!("Unsupported CLI provider '{other}'. Use github or gitlab.")),
		};
		let host = hostname
			.as_deref()
			.map(str::trim)
			.filter(|h| !h.is_empty())
			.unwrap_or(default_host);

		let mut child = process_utils::hidden_command(binary)
			.args(["auth", "login", "--hostname", host, token_flag])
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.map_err(|err| format!("Could not launch `{binary}` CLI (is it installed?): {err}"))?;

		if let Some(mut stdin) = child.stdin.take() {
			stdin
				.write_all(format!("{trimmed_token}\n").as_bytes())
				.map_err(|err| format!("Failed writing token to `{binary}` stdin: {err}"))?;
		}

		let output = child
			.wait_with_output()
			.map_err(|err| format!("Failed waiting for `{binary} auth login`: {err}"))?;
		if !output.status.success() {
			let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
			let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
			let detail = if !stderr.is_empty() { stderr } else { stdout };
			return Err(format!("`{binary} auth login` failed: {detail}"));
		}
		Ok(format!("Authenticated `{binary}` for {host} (stored in OS keyring)."))
	})
	.await
	.map_err(|err| format!("CLI token login task failed: {err}"))?
}

/// Launches the official interactive browser/OAuth login flow for `gh` or `glab`.
#[tauri::command]
pub async fn cli_auth_start_web_login(
	provider: String,
	hostname: Option<String>,
) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let provider_norm = provider.trim().to_lowercase();
		let (binary, default_host) = match provider_norm.as_str() {
			"github" => ("gh", "github.com"),
			"gitlab" => ("glab", "gitlab.com"),
			other => return Err(format!("Unsupported CLI provider '{other}'. Use github or gitlab.")),
		};
		let host = hostname
			.as_deref()
			.map(str::trim)
			.filter(|h| !h.is_empty())
			.unwrap_or(default_host);

		if probe_cli_version(binary).is_none() {
			return Err(format!(
				"`{binary}` CLI is not installed on PATH. Install it first or use a Personal Access Token."
			));
		}

		#[cfg(windows)]
		{
			let cmd_line = if binary == "gh" {
				format!("gh auth login --hostname {host} --web --git-protocol https")
			} else {
				format!("glab auth login --hostname {host} --web")
			};
			std::process::Command::new("cmd.exe")
				.args(["/C", "start", "GitManager CLI Login", "cmd.exe", "/K", &cmd_line])
				.spawn()
				.map_err(|err| format!("Could not launch interactive terminal for `{binary}`: {err}"))?;
			return Ok(format!(
				"Opened browser login terminal for `{binary}` ({host}). Finish the browser prompt, then click Refresh Status."
			));
		}

		#[cfg(not(windows))]
		{
			let mut cmd = std::process::Command::new(binary);
			if binary == "gh" {
				cmd.args(["auth", "login", "--hostname", host, "--web", "--git-protocol", "https"]);
			} else {
				cmd.args(["auth", "login", "--hostname", host, "--web"]);
			}
			cmd.spawn()
				.map_err(|err| format!("Could not start `{binary} auth login --web`: {err}"))?;
			Ok(format!(
				"Started `{binary}` web OAuth flow for {host}. Complete browser authorization, then click Refresh Status."
			))
		}
	})
	.await
	.map_err(|err| format!("CLI web login task failed: {err}"))?
}

fn parse_workflow_metadata(content: &str, fallback_name: &str) -> (String, bool, Vec<WorkflowInputField>) {
	let mut display_name = fallback_name.to_string();
	let mut supports_dispatch = false;
	let mut inputs: Vec<WorkflowInputField> = Vec::new();

	let lines: Vec<&str> = content.lines().collect();
	let mut in_dispatch = false;
	let mut in_inputs = false;
	let mut dispatch_indent: usize = 0;
	let mut inputs_indent: usize = 0;
	let mut current_input: Option<WorkflowInputField> = None;
	let mut current_input_indent: usize = 0;

	for raw_line in &lines {
		let trimmed = raw_line.trim();
		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}
		let indent = raw_line.len() - raw_line.trim_start_matches([' ', '\t']).len();

		if indent == 0 {
			if let Some(rest) = trimmed.strip_prefix("name:") {
				let cleaned = rest.trim().trim_matches(['"', '\'']);
				if !cleaned.is_empty() {
					display_name = cleaned.to_string();
				}
			}
		}

		if trimmed.starts_with("workflow_dispatch") {
			supports_dispatch = true;
			in_dispatch = true;
			dispatch_indent = indent;
			in_inputs = false;
			continue;
		}

		if in_dispatch && indent <= dispatch_indent && !trimmed.starts_with("workflow_dispatch") {
			if let Some(item) = current_input.take() {
				inputs.push(item);
			}
			in_dispatch = false;
			in_inputs = false;
		}

		if in_dispatch {
			if trimmed.starts_with("inputs:") && !in_inputs {
				in_inputs = true;
				inputs_indent = indent;
				continue;
			}
			if in_inputs {
				if indent <= inputs_indent {
					if let Some(item) = current_input.take() {
						inputs.push(item);
					}
					in_inputs = false;
					continue;
				}
				if trimmed.ends_with(':') && (current_input.is_none() || indent <= current_input_indent) {
					if let Some(item) = current_input.take() {
						inputs.push(item);
					}
					let field_name = trimmed.trim_end_matches(':').trim().to_string();
					current_input_indent = indent;
					current_input = Some(WorkflowInputField {
						name: field_name,
						description: String::new(),
						default_value: String::new(),
						required: false,
					});
				} else if let Some(ref mut item) = current_input {
					if let Some(desc) = trimmed.strip_prefix("description:") {
						item.description = desc.trim().trim_matches(['"', '\'']).to_string();
					} else if let Some(def) = trimmed.strip_prefix("default:") {
						item.default_value = def.trim().trim_matches(['"', '\'']).to_string();
					} else if let Some(req) = trimmed.strip_prefix("required:") {
						item.required = req.trim().eq_ignore_ascii_case("true");
					}
				}
			}
		}
	}
	if let Some(item) = current_input.take() {
		inputs.push(item);
	}

	(display_name, supports_dispatch, inputs)
}

/// Scans a local repository for GitHub Actions, GitLab CI, and Forgejo/Gitea workflow YAML files.
#[tauri::command]
pub async fn list_repo_workflows(repo_path: String) -> Result<Vec<RepoWorkflowFile>, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let mut workflows = Vec::new();

		for (provider, rel_dir) in [
			("github", ".github/workflows"),
			("forgejo", ".forgejo/workflows"),
			("gitea", ".gitea/workflows"),
		] {
			let dir = root.join(rel_dir);
			if let Ok(entries) = std::fs::read_dir(&dir) {
				for entry in entries.flatten() {
					let path = entry.path();
					let ext = path
						.extension()
						.and_then(|e| e.to_str())
						.unwrap_or("")
						.to_lowercase();
					if ext != "yml" && ext != "yaml" {
						continue;
					}
					let file_name = entry.file_name().to_string_lossy().to_string();
					let relative_path = format!("{rel_dir}/{file_name}");
					let content = std::fs::read_to_string(&path).unwrap_or_default();
					let (name, supports_dispatch, inputs) =
						parse_workflow_metadata(&content, &file_name);
					workflows.push(RepoWorkflowFile {
						provider: provider.to_string(),
						name,
						file_name,
						relative_path,
						supports_dispatch,
						inputs,
					});
				}
			}
		}

		let gitlab_ci = root.join(".gitlab-ci.yml");
		if gitlab_ci.is_file() {
			workflows.push(RepoWorkflowFile {
				provider: "gitlab".to_string(),
				name: "GitLab CI/CD Pipeline".to_string(),
				file_name: ".gitlab-ci.yml".to_string(),
				relative_path: ".gitlab-ci.yml".to_string(),
				supports_dispatch: true,
				inputs: Vec::new(),
			});
		}

		workflows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
		Ok(workflows)
	})
	.await
	.map_err(|err| format!("Failed listing repository workflows: {err}"))?
}

fn detect_repo_provider_from_git_config(repo_path: &Path) -> String {
	let output = process_utils::hidden_command("git")
		.args(["remote", "get-url", "origin"])
		.current_dir(repo_path)
		.output();
	if let Ok(out) = output {
		if out.status.success() {
			let url = String::from_utf8_lossy(&out.stdout).to_lowercase();
			if url.contains("gitlab") {
				return "gitlab".to_string();
			}
			if url.contains("github") {
				return "github".to_string();
			}
		}
	}
	if repo_path.join(".gitlab-ci.yml").is_file() {
		"gitlab".to_string()
	} else {
		"github".to_string()
	}
}

/// Lists recent remote CI runs for a repository using `gh` (GitHub) or `glab` (GitLab).
#[tauri::command]
pub async fn list_workflow_runs(
	repo_path: String,
	limit: Option<usize>,
) -> Result<WorkflowRunsResponse, String> {
	let max_runs = limit.unwrap_or(15).clamp(1, 50);
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let provider = detect_repo_provider_from_git_config(&root);
		let binary = if provider == "gitlab" { "glab" } else { "gh" };

		if probe_cli_version(binary).is_none() {
			return Ok(WorkflowRunsResponse {
				provider,
				cli_available: false,
				note: format!(
					"`{binary}` CLI is not installed on PATH. Install `{binary}` and sign in under Accounts to view and trigger remote runs."
				),
				runs: Vec::new(),
			});
		}

		if provider == "github" {
			let output = process_utils::hidden_command("gh")
				.args([
					"run",
					"list",
					"--limit",
					&max_runs.to_string(),
					"--json",
					"databaseId,displayTitle,workflowName,status,conclusion,headBranch,headSha,createdAt,url",
				])
				.current_dir(&root)
				.output()
				.map_err(|err| format!("Failed executing `gh run list`: {err}"))?;

			if !output.status.success() {
				let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
				return Ok(WorkflowRunsResponse {
					provider,
					cli_available: true,
					note: if stderr.is_empty() {
						"Could not load GitHub Actions runs (check `gh auth status` in Accounts).".to_string()
					} else {
						stderr
					},
					runs: Vec::new(),
				});
			}

			let raw_items: Vec<serde_json::Value> =
				serde_json::from_slice(&output.stdout).unwrap_or_default();
			let runs = raw_items
				.into_iter()
				.map(|item| WorkflowRunEntry {
					id: item
						.get("databaseId")
						.map(|v| v.to_string().trim_matches('"').to_string())
						.unwrap_or_default(),
					provider: "github".to_string(),
					workflow_name: item
						.get("workflowName")
						.and_then(|v| v.as_str())
						.unwrap_or("Workflow")
						.to_string(),
					title: item
						.get("displayTitle")
						.and_then(|v| v.as_str())
						.unwrap_or("")
						.to_string(),
					status: item
						.get("status")
						.and_then(|v| v.as_str())
						.unwrap_or("unknown")
						.to_string(),
					conclusion: item
						.get("conclusion")
						.and_then(|v| v.as_str())
						.unwrap_or("")
						.to_string(),
					branch: item
						.get("headBranch")
						.and_then(|v| v.as_str())
						.unwrap_or("")
						.to_string(),
					sha: item
						.get("headSha")
						.and_then(|v| v.as_str())
						.map(|s| s.chars().take(7).collect())
						.unwrap_or_default(),
					created_at: item
						.get("createdAt")
						.and_then(|v| v.as_str())
						.unwrap_or("")
						.to_string(),
					url: item
						.get("url")
						.and_then(|v| v.as_str())
						.unwrap_or("")
						.to_string(),
				})
				.collect();

			Ok(WorkflowRunsResponse {
				provider,
				cli_available: true,
				note: String::new(),
				runs,
			})
		} else {
			let output = process_utils::hidden_command("glab")
				.args(["ci", "list", "--output", "json", "--per-page", &max_runs.to_string()])
				.current_dir(&root)
				.output()
				.map_err(|err| format!("Failed executing `glab ci list`: {err}"))?;

			if !output.status.success() {
				let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
				return Ok(WorkflowRunsResponse {
					provider,
					cli_available: true,
					note: if stderr.is_empty() {
						"Could not load GitLab CI pipelines (check `glab auth status` in Accounts).".to_string()
					} else {
						stderr
					},
					runs: Vec::new(),
				});
			}

			let raw_items: Vec<serde_json::Value> =
				serde_json::from_slice(&output.stdout).unwrap_or_default();
			let runs = raw_items
				.into_iter()
				.map(|item| {
					let status = item
						.get("status")
						.and_then(|v| v.as_str())
						.unwrap_or("unknown")
						.to_string();
					WorkflowRunEntry {
						id: item
							.get("id")
							.map(|v| v.to_string().trim_matches('"').to_string())
							.unwrap_or_default(),
						provider: "gitlab".to_string(),
						workflow_name: "GitLab CI Pipeline".to_string(),
						title: format!(
							"Pipeline #{}",
							item.get("iid")
								.or_else(|| item.get("id"))
								.map(|v| v.to_string())
								.unwrap_or_default()
						),
						status: status.clone(),
						conclusion: status,
						branch: item
							.get("ref")
							.and_then(|v| v.as_str())
							.unwrap_or("")
							.to_string(),
						sha: item
							.get("sha")
							.and_then(|v| v.as_str())
							.map(|s| s.chars().take(7).collect())
							.unwrap_or_default(),
						created_at: item
							.get("created_at")
							.and_then(|v| v.as_str())
							.unwrap_or("")
							.to_string(),
						url: item
							.get("web_url")
							.and_then(|v| v.as_str())
							.unwrap_or("")
							.to_string(),
					}
				})
				.collect();

			Ok(WorkflowRunsResponse {
				provider,
				cli_available: true,
				note: String::new(),
				runs,
			})
		}
	})
	.await
	.map_err(|err| format!("Workflow runs task failed: {err}"))?
}

/// Triggers a remote GitHub Actions workflow (`gh workflow run`) or GitLab pipeline (`glab ci run`).
#[tauri::command]
pub async fn trigger_workflow_run(
	repo_path: String,
	workflow_file: String,
	branch: Option<String>,
	inputs: Option<HashMap<String, String>>,
) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let provider = detect_repo_provider_from_git_config(&root);
		let fields = inputs.unwrap_or_default();

		if provider == "github" {
			let mut cmd = process_utils::hidden_command("gh");
			cmd.args(["workflow", "run", workflow_file.trim()]);
			if let Some(ref br) = branch {
				if !br.trim().is_empty() {
					cmd.args(["--ref", br.trim()]);
				}
			}
			for (key, value) in &fields {
				if !key.trim().is_empty() {
					cmd.args(["-f", &format!("{}={}", key.trim(), value)]);
				}
			}
			let output = cmd
				.current_dir(&root)
				.output()
				.map_err(|err| format!("Failed executing `gh workflow run`: {err}"))?;
			if !output.status.success() {
				let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
				return Err(format!("Could not trigger GitHub workflow: {stderr}"));
			}
			Ok(format!("Triggered GitHub workflow '{workflow_file}'."))
		} else {
			let mut cmd = process_utils::hidden_command("glab");
			cmd.args(["ci", "run"]);
			if let Some(ref br) = branch {
				if !br.trim().is_empty() {
					cmd.args(["--branch", br.trim()]);
				}
			}
			for (key, value) in &fields {
				if !key.trim().is_empty() {
					cmd.args(["--variables", &format!("{}:{}", key.trim(), value)]);
				}
			}
			let output = cmd
				.current_dir(&root)
				.output()
				.map_err(|err| format!("Failed executing `glab ci run`: {err}"))?;
			if !output.status.success() {
				let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
				return Err(format!("Could not trigger GitLab pipeline: {stderr}"));
			}
			Ok("Triggered GitLab CI pipeline.".to_string())
		}
	})
	.await
	.map_err(|err| format!("Trigger workflow task failed: {err}"))?
}

/// Fetches logs for a specific GitHub Actions run or GitLab CI pipeline.
#[tauri::command]
pub async fn get_workflow_run_logs(repo_path: String, run_id: String) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let provider = detect_repo_provider_from_git_config(&root);
		let output = if provider == "github" {
			process_utils::hidden_command("gh")
				.args(["run", "view", run_id.trim(), "--log"])
				.current_dir(&root)
				.output()
				.map_err(|err| format!("Failed executing `gh run view`: {err}"))?
		} else {
			process_utils::hidden_command("glab")
				.args(["ci", "get", "-p", run_id.trim()])
				.current_dir(&root)
				.output()
				.map_err(|err| format!("Failed executing `glab ci get`: {err}"))?
		};

		if !output.status.success() {
			let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
			return Err(if stderr.is_empty() {
				format!("Could not fetch logs for run #{run_id}.")
			} else {
				stderr
			});
		}

		let text = String::from_utf8_lossy(&output.stdout).to_string();
		// Cap output at 250 KiB so huge build logs never freeze the WebView.
		if text.len() > 256_000 {
			let tail = &text[text.len() - 256_000..];
			Ok(format!("... (log truncated to last 250 KiB) ...\n{tail}"))
		} else {
			Ok(text)
		}
	})
	.await
	.map_err(|err| format!("Log fetch task failed: {err}"))?
}

fn validate_relative_repo_path(relative_path: &str) -> Result<PathBuf, String> {
	let trimmed = relative_path.trim().replace('\\', "/");
	if trimmed.is_empty() {
		return Err("File path cannot be empty.".to_string());
	}
	let candidate = PathBuf::from(&trimmed);
	if candidate.is_absolute() {
		return Err("File path must be relative to the repository root.".to_string());
	}
	for component in candidate.components() {
		if matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
			return Err("Path traversal outside the repository root is not allowed.".to_string());
		}
	}
	Ok(candidate)
}

/// Safely saves text content to a file inside a repository for the in-app Workbench Editor.
#[tauri::command]
pub async fn write_repository_file(
	repo_path: String,
	relative_path: String,
	content: String,
) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let rel = validate_relative_repo_path(&relative_path)?;
		let target = root.join(&rel);
		if let Some(parent) = target.parent() {
			std::fs::create_dir_all(parent)
				.map_err(|err| format!("Could not create parent folders for {}: {err}", rel.display()))?;
		}
		std::fs::write(&target, content.as_bytes())
			.map_err(|err| format!("Could not write {}: {err}", rel.display()))?;
		Ok(format!("Saved {}", rel.display()))
	})
	.await
	.map_err(|err| format!("File save task failed: {err}"))?
}

/// Exports a plain-text troubleshooting diagnostics report to the user's Downloads (or home) folder.
#[tauri::command]
pub async fn export_troubleshoot_diagnostics(app_handle: AppHandle) -> Result<String, String> {
	let version = app_handle.package_info().version.to_string();
	tokio::task::spawn_blocking(move || {
		let ts = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|d| d.as_secs())
			.unwrap_or(0);
		let out_dir = dirs::download_dir()
			.or_else(dirs::home_dir)
			.unwrap_or_else(std::env::temp_dir);
		let report_path = out_dir.join(format!("gitmanager-diagnostics-{ts}.txt"));

		let gh_ver = probe_cli_version("gh").unwrap_or_else(|| "not installed".to_string());
		let glab_ver = probe_cli_version("glab").unwrap_or_else(|| "not installed".to_string());
		let git_ver = probe_cli_version("git").unwrap_or_else(|| "not installed".to_string());
		let gitm_ver = probe_cli_version("gitm").unwrap_or_else(|| "not on PATH".to_string());

		let mut report = String::new();
		report.push_str("GitManager Troubleshooting Diagnostics Report\n");
		report.push_str("=============================================\n");
		report.push_str(&format!("Timestamp (UTC epoch): {ts}\n"));
		report.push_str(&format!("App Version: {version}\n"));
		report.push_str(&format!("OS / Arch: {} / {}\n", std::env::consts::OS, std::env::consts::ARCH));
		report.push_str(&format!("git:  {git_ver}\n"));
		report.push_str(&format!("gitm: {gitm_ver}\n"));
		report.push_str(&format!("gh:   {gh_ver}\n"));
		report.push_str(&format!("glab: {glab_ver}\n"));

		std::fs::write(&report_path, report.as_bytes())
			.map_err(|err| format!("Could not write diagnostics report: {err}"))?;
		Ok(report_path.to_string_lossy().to_string())
	})
	.await
	.map_err(|err| format!("Diagnostics export task failed: {err}"))?
}
