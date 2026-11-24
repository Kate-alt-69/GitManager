use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};

use crate::process_utils;

/// Normalized Release record returned by `gh release list` or `glab release list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoReleaseEntry {
	pub tag_name: String,
	pub name: String,
	pub is_draft: bool,
	pub is_prerelease: bool,
	pub published_at: String,
	pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoReleasesResponse {
	pub provider: String,
	pub note: String,
	pub releases: Vec<RepoReleaseEntry>,
}

/// Normalized Issue record returned by `gh issue list` or `glab issue list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoIssueEntry {
	pub number: String,
	pub title: String,
	pub state: String,
	pub author: String,
	pub labels: Vec<String>,
	pub updated_at: String,
	pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoIssuesResponse {
	pub provider: String,
	pub note: String,
	pub issues: Vec<RepoIssueEntry>,
}

/// Normalized CI Variable or Secret entry (secret values are always masked).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiVariableEntry {
	pub name: String,
	pub kind: String,
	pub scope: String,
	pub updated_at: String,
	pub value_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiVariablesResponse {
	pub provider: String,
	pub note: String,
	pub items: Vec<CiVariableEntry>,
}

/// Branch protection status for the active branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchProtectionStatus {
	pub branch: String,
	pub protected: bool,
	pub summary: String,
}

fn detect_provider(repo_path: &Path) -> String {
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

fn slugify_issue_branch(issue_number: &str, title: &str) -> String {
	let mut slug = String::new();
	let mut last_dash = false;
	for ch in title.chars() {
		if ch.is_ascii_alphanumeric() {
			slug.push(ch.to_ascii_lowercase());
			last_dash = false;
		} else if !last_dash && !slug.is_empty() {
			slug.push('-');
			last_dash = true;
		}
		if slug.len() >= 36 {
			break;
		}
	}
	let cleaned = slug.trim_matches('-');
	let num = issue_number.trim().trim_start_matches('#');
	if cleaned.is_empty() {
		format!("issue/{num}")
	} else {
		format!("issue/{num}-{cleaned}")
	}
}

/// Lists releases for a repository using `gh` (GitHub) or `glab` (GitLab).
#[tauri::command]
pub async fn list_repo_releases(repo_path: String) -> Result<RepoReleasesResponse, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let provider = detect_provider(&root);
		if provider == "github" {
			let output = process_utils::hidden_command("gh")
				.args([
					"release",
					"list",
					"--limit",
					"20",
					"--json",
					"tagName,name,isDraft,isPrerelease,publishedAt",
				])
				.current_dir(&root)
				.output();
			let Ok(out) = output else {
				return Ok(RepoReleasesResponse {
					provider,
					note: "Install and sign in to `gh` CLI under Accounts to manage GitHub Releases.".to_string(),
					releases: Vec::new(),
				});
			};
			if !out.status.success() {
				let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
				return Ok(RepoReleasesResponse {
					provider,
					note: if stderr.is_empty() {
						"Could not load GitHub releases.".to_string()
					} else {
						stderr
					},
					releases: Vec::new(),
				});
			}
			let items: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
			let releases = items
				.into_iter()
				.map(|item| RepoReleaseEntry {
					tag_name: item.get("tagName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
					name: item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
					is_draft: item.get("isDraft").and_then(|v| v.as_bool()).unwrap_or(false),
					is_prerelease: item.get("isPrerelease").and_then(|v| v.as_bool()).unwrap_or(false),
					published_at: item.get("publishedAt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
					url: String::new(),
				})
				.collect();
			Ok(RepoReleasesResponse {
				provider,
				note: String::new(),
				releases,
			})
		} else {
			let output = process_utils::hidden_command("glab")
				.args(["release", "list"])
				.current_dir(&root)
				.output();
			let Ok(out) = output else {
				return Ok(RepoReleasesResponse {
					provider,
					note: "Install and sign in to `glab` CLI under Accounts to manage GitLab Releases.".to_string(),
					releases: Vec::new(),
				});
			};
			if !out.status.success() {
				let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
				return Ok(RepoReleasesResponse {
					provider,
					note: stderr,
					releases: Vec::new(),
				});
			}
			let mut releases = Vec::new();
			for line in String::from_utf8_lossy(&out.stdout).lines().skip(1) {
				let cols: Vec<&str> = line.split('\t').map(str::trim).collect();
				if cols.len() >= 2 && !cols[0].is_empty() {
					releases.push(RepoReleaseEntry {
						tag_name: cols[1].to_string(),
						name: cols[0].to_string(),
						is_draft: false,
						is_prerelease: false,
						published_at: cols.get(2).copied().unwrap_or("").to_string(),
						url: String::new(),
					});
				}
			}
			Ok(RepoReleasesResponse {
				provider,
				note: String::new(),
				releases,
			})
		}
	})
	.await
	.map_err(|err| format!("List releases task failed: {err}"))?
}

/// Auto-generates markdown release notes from local commits since the latest Git tag.
#[tauri::command]
pub async fn generate_release_notes(repo_path: String) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let last_tag = process_utils::hidden_command("git")
			.args(["describe", "--tags", "--abbrev=0"])
			.current_dir(&root)
			.output()
			.ok()
			.filter(|o| o.status.success())
			.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
			.filter(|s| !s.is_empty());

		let range = match &last_tag {
			Some(tag) => format!("{tag}..HEAD"),
			None => "HEAD".to_string(),
		};

		let output = process_utils::hidden_command("git")
			.args(["log", &range, "--pretty=format:- %s (%h) — %an", "-n", "40"])
			.current_dir(&root)
			.output()
			.map_err(|err| format!("Failed generating changelog from git log: {err}"))?;

		let body = String::from_utf8_lossy(&output.stdout).trim().to_string();
		let heading = match last_tag {
			Some(tag) => format!("## Changes since `{tag}`\n\n"),
			None => "## Highlights\n\n".to_string(),
		};
		if body.is_empty() {
			Ok(format!("{heading}- Maintenance release"))
		} else {
			Ok(format!("{heading}{body}"))
		}
	})
	.await
	.map_err(|err| format!("Generate release notes task failed: {err}"))?
}

/// Creates a GitHub or GitLab release via `gh release create` or `glab release create`.
#[tauri::command]
pub async fn create_repo_release(
	repo_path: String,
	tag_name: String,
	title: String,
	notes: String,
	is_draft: bool,
	is_prerelease: bool,
	asset_path: Option<String>,
) -> Result<String, String> {
	let tag = tag_name.trim().to_string();
	if tag.is_empty() {
		return Err("Release tag name is required (e.g. v1.2.0).".to_string());
	}
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let provider = detect_provider(&root);
		let release_title = if title.trim().is_empty() {
			tag.clone()
		} else {
			title.trim().to_string()
		};

		if provider == "github" {
			let mut cmd = process_utils::hidden_command("gh");
			cmd.args(["release", "create", &tag, "--title", &release_title, "--notes", &notes]);
			if is_draft {
				cmd.arg("--draft");
			}
			if is_prerelease {
				cmd.arg("--prerelease");
			}
			if let Some(ref asset) = asset_path {
				if !asset.trim().is_empty() {
					cmd.arg(asset.trim());
				}
			}
			let out = cmd
				.current_dir(&root)
				.output()
				.map_err(|err| format!("Failed executing `gh release create`: {err}"))?;
			if !out.status.success() {
				let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
				return Err(format!("GitHub release creation failed: {stderr}"));
			}
			Ok(format!("Created GitHub Release {tag}."))
		} else {
			let mut cmd = process_utils::hidden_command("glab");
			cmd.args(["release", "create", &tag, "--name", &release_title, "--notes", &notes]);
			let out = cmd
				.current_dir(&root)
				.output()
				.map_err(|err| format!("Failed executing `glab release create`: {err}"))?;
			if !out.status.success() {
				let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
				return Err(format!("GitLab release creation failed: {stderr}"));
			}
			Ok(format!("Created GitLab Release {tag}."))
		}
	})
	.await
	.map_err(|err| format!("Create release task failed: {err}"))?
}

/// Lists issues for a repository via `gh issue list` or `glab issue list`.
#[tauri::command]
pub async fn list_repo_issues(
	repo_path: String,
	state: Option<String>,
) -> Result<RepoIssuesResponse, String> {
	let wanted_state = state.unwrap_or_else(|| "open".to_string());
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		if !root.is_dir() {
			return Err(format!("Repository directory not found: {repo_path}"));
		}
		let provider = detect_provider(&root);
		if provider == "github" {
			let output = process_utils::hidden_command("gh")
				.args([
					"issue",
					"list",
					"--state",
					wanted_state.as_str(),
					"--limit",
					"30",
					"--json",
					"number,title,state,author,labels,updatedAt,url",
				])
				.current_dir(&root)
				.output();
			let Ok(out) = output else {
				return Ok(RepoIssuesResponse {
					provider,
					note: "Install and sign in to `gh` CLI under Accounts to view GitHub Issues.".to_string(),
					issues: Vec::new(),
				});
			};
			if !out.status.success() {
				return Ok(RepoIssuesResponse {
					provider,
					note: String::from_utf8_lossy(&out.stderr).trim().to_string(),
					issues: Vec::new(),
				});
			}
			let raw: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
			let issues = raw
				.into_iter()
				.map(|item| {
					let labels = item
						.get("labels")
						.and_then(|v| v.as_array())
						.map(|arr| {
							arr.iter()
								.filter_map(|l| l.get("name").and_then(|n| n.as_str()).map(str::to_owned))
								.collect()
						})
						.unwrap_or_default();
					RepoIssueEntry {
						number: item.get("number").map(|v| v.to_string()).unwrap_or_default(),
						title: item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
						state: item.get("state").and_then(|v| v.as_str()).unwrap_or("OPEN").to_string(),
						author: item
							.get("author")
							.and_then(|a| a.get("login"))
							.and_then(|v| v.as_str())
							.unwrap_or("")
							.to_string(),
						labels,
						updated_at: item.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
						url: item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
					}
				})
				.collect();
			Ok(RepoIssuesResponse {
				provider,
				note: String::new(),
				issues,
			})
		} else {
			let mut cmd = process_utils::hidden_command("glab");
			cmd.args(["issue", "list", "--output", "json", "--per-page", "30"]);
			if wanted_state == "closed" {
				cmd.arg("--closed");
			} else if wanted_state == "all" {
				cmd.arg("--all");
			}
			let Ok(out) = cmd.current_dir(&root).output() else {
				return Ok(RepoIssuesResponse {
					provider,
					note: "Install and sign in to `glab` CLI under Accounts to view GitLab Issues.".to_string(),
					issues: Vec::new(),
				});
			};
			if !out.status.success() {
				return Ok(RepoIssuesResponse {
					provider,
					note: String::from_utf8_lossy(&out.stderr).trim().to_string(),
					issues: Vec::new(),
				});
			}
			let raw: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
			let issues = raw
				.into_iter()
				.map(|item| {
					let labels = item
						.get("labels")
						.and_then(|v| v.as_array())
						.map(|arr| {
							arr.iter()
								.filter_map(|l| l.as_str().map(str::to_owned))
								.collect()
						})
						.unwrap_or_default();
					RepoIssueEntry {
						number: item.get("iid").map(|v| v.to_string()).unwrap_or_default(),
						title: item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
						state: item.get("state").and_then(|v| v.as_str()).unwrap_or("opened").to_string(),
						author: item
							.get("author")
							.and_then(|a| a.get("username"))
							.and_then(|v| v.as_str())
							.unwrap_or("")
							.to_string(),
						labels,
						updated_at: item.get("updated_at").and_then(|v| v.as_str()).unwrap_or("").to_string(),
						url: item.get("web_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
					}
				})
				.collect();
			Ok(RepoIssuesResponse {
				provider,
				note: String::new(),
				issues,
			})
		}
	})
	.await
	.map_err(|err| format!("List issues task failed: {err}"))?
}

/// Creates a new GitHub or GitLab issue.
#[tauri::command]
pub async fn create_repo_issue(
	repo_path: String,
	title: String,
	body: String,
	labels: Option<String>,
) -> Result<String, String> {
	let trimmed_title = title.trim().to_string();
	if trimmed_title.is_empty() {
		return Err("Issue title cannot be empty.".to_string());
	}
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		let provider = detect_provider(&root);
		let mut cmd = if provider == "github" {
			let mut c = process_utils::hidden_command("gh");
			c.args(["issue", "create", "--title", &trimmed_title, "--body", &body]);
			if let Some(ref lbl) = labels {
				if !lbl.trim().is_empty() {
					c.args(["--label", lbl.trim()]);
				}
			}
			c
		} else {
			let mut c = process_utils::hidden_command("glab");
			c.args(["issue", "create", "--title", &trimmed_title, "--description", &body]);
			if let Some(ref lbl) = labels {
				if !lbl.trim().is_empty() {
					c.args(["--label", lbl.trim()]);
				}
			}
			c
		};
		let out = cmd
			.current_dir(&root)
			.output()
			.map_err(|err| format!("Failed creating issue: {err}"))?;
		if !out.status.success() {
			return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
		}
		Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
	})
	.await
	.map_err(|err| format!("Create issue task failed: {err}"))?
}

/// Closes or reopens an issue via `gh issue` / `glab issue`.
#[tauri::command]
pub async fn set_repo_issue_state(
	repo_path: String,
	issue_number: String,
	close: bool,
) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		let provider = detect_provider(&root);
		let verb = if close { "close" } else { "reopen" };
		let binary = if provider == "github" { "gh" } else { "glab" };
		let out = process_utils::hidden_command(binary)
			.args(["issue", verb, issue_number.trim()])
			.current_dir(&root)
			.output()
			.map_err(|err| format!("Failed executing `{binary} issue {verb}`: {err}"))?;
		if !out.status.success() {
			return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
		}
		Ok(format!("Issue #{issue_number} {verb}d."))
	})
	.await
	.map_err(|err| format!("Update issue state task failed: {err}"))?
}

/// Creates and checks out a local Git branch named from an issue number and title.
#[tauri::command]
pub async fn create_branch_from_issue(
	repo_path: String,
	issue_number: String,
	issue_title: String,
) -> Result<String, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		let branch_name = slugify_issue_branch(&issue_number, &issue_title);
		let out = process_utils::hidden_command("git")
			.args(["checkout", "-b", &branch_name])
			.current_dir(&root)
			.output()
			.map_err(|err| format!("Could not create branch '{branch_name}': {err}"))?;
		if !out.status.success() {
			let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
			return Err(format!("Could not create branch '{branch_name}': {stderr}"));
		}
		Ok(branch_name)
	})
	.await
	.map_err(|err| format!("Create branch task failed: {err}"))?
}

/// Lists CI secrets and variables (secret values remain masked).
#[tauri::command]
pub async fn list_ci_variables(repo_path: String) -> Result<CiVariablesResponse, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		let provider = detect_provider(&root);
		let mut items = Vec::new();
		if provider == "github" {
			let sec_out = process_utils::hidden_command("gh")
				.args(["secret", "list", "--json", "name,updatedAt"])
				.current_dir(&root)
				.output();
			if let Ok(out) = sec_out {
				if out.status.success() {
					let raw: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
					for row in raw {
						items.push(CiVariableEntry {
							name: row.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
							kind: "secret".to_string(),
							scope: "repository".to_string(),
							updated_at: row.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
							value_preview: "••••••••".to_string(),
						});
					}
				}
			}
			let var_out = process_utils::hidden_command("gh")
				.args(["variable", "list", "--json", "name,value,updatedAt"])
				.current_dir(&root)
				.output();
			if let Ok(out) = var_out {
				if out.status.success() {
					let raw: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
					for row in raw {
						items.push(CiVariableEntry {
							name: row.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
							kind: "variable".to_string(),
							scope: "repository".to_string(),
							updated_at: row.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
							value_preview: row.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string(),
						});
					}
				}
			}
			Ok(CiVariablesResponse {
				provider,
				note: if items.is_empty() {
					"No repository secrets or variables found (or `gh` lacks repo admin scope).".to_string()
				} else {
					String::new()
				},
				items,
			})
		} else {
			let out = process_utils::hidden_command("glab")
				.args(["variable", "list", "--output", "json"])
				.current_dir(&root)
				.output();
			let Ok(out) = out else {
				return Ok(CiVariablesResponse {
					provider,
					note: "Install and sign in to `glab` CLI to view GitLab CI variables.".to_string(),
					items,
				});
			};
			if !out.status.success() {
				return Ok(CiVariablesResponse {
					provider,
					note: String::from_utf8_lossy(&out.stderr).trim().to_string(),
					items,
				});
			}
			let raw: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
			for row in raw {
				let masked = row.get("masked").and_then(|v| v.as_bool()).unwrap_or(false);
				items.push(CiVariableEntry {
					name: row.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string(),
					kind: if masked { "secret".to_string() } else { "variable".to_string() },
					scope: row
						.get("environment_scope")
						.and_then(|v| v.as_str())
						.unwrap_or("*")
						.to_string(),
					updated_at: String::new(),
					value_preview: if masked {
						"••••••••".to_string()
					} else {
						row.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string()
					},
				});
			}
			Ok(CiVariablesResponse {
				provider,
				note: String::new(),
				items,
			})
		}
	})
	.await
	.map_err(|err| format!("List CI variables task failed: {err}"))?
}

/// Safely sets a CI secret or variable by piping its value over stdin (never in CLI args).
#[tauri::command]
pub async fn set_ci_variable(
	repo_path: String,
	name: String,
	value: String,
	is_secret: bool,
) -> Result<String, String> {
	let key = name.trim().to_string();
	if key.is_empty() {
		return Err("Variable/Secret name cannot be empty.".to_string());
	}
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		let provider = detect_provider(&root);
		let mut cmd = if provider == "github" {
			let mut c = process_utils::hidden_command("gh");
			if is_secret {
				c.args(["secret", "set", &key]);
			} else {
				c.args(["variable", "set", &key]);
			}
			c
		} else {
			let mut c = process_utils::hidden_command("glab");
			c.args(["variable", "set", &key]);
			if is_secret {
				c.arg("--masked");
			}
			c
		};

		let mut child = cmd
			.current_dir(&root)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.map_err(|err| format!("Could not launch CLI to set {key}: {err}"))?;

		if let Some(mut stdin) = child.stdin.take() {
			stdin
				.write_all(value.as_bytes())
				.map_err(|err| format!("Failed writing value for {key}: {err}"))?;
		}
		let out = child
			.wait_with_output()
			.map_err(|err| format!("Failed saving {key}: {err}"))?;
		if !out.status.success() {
			return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
		}
		Ok(format!(
			"Saved {} `{key}`.",
			if is_secret { "secret" } else { "variable" }
		))
	})
	.await
	.map_err(|err| format!("Set CI variable task failed: {err}"))?
}

/// Checks whether the active branch has remote branch protection rules enabled.
#[tauri::command]
pub async fn get_branch_protection_status(
	repo_path: String,
	branch: String,
) -> Result<BranchProtectionStatus, String> {
	tokio::task::spawn_blocking(move || {
		let root = PathBuf::from(&repo_path);
		let provider = detect_provider(&root);
		if provider == "github" {
			let endpoint = format!("repos/{{owner}}/{{repo}}/branches/{}/protection", branch.trim());
			let out = process_utils::hidden_command("gh")
				.args(["api", &endpoint])
				.current_dir(&root)
				.output();
			if let Ok(output) = out {
				if output.status.success() {
					return Ok(BranchProtectionStatus {
						branch,
						protected: true,
						summary: "Protected branch (GitHub rules enforced)".to_string(),
					});
				}
			}
		} else {
			let endpoint = format!("projects/:id/protected_branches/{}", branch.trim());
			let out = process_utils::hidden_command("glab")
				.args(["api", &endpoint])
				.current_dir(&root)
				.output();
			if let Ok(output) = out {
				if output.status.success() {
					return Ok(BranchProtectionStatus {
						branch,
						protected: true,
						summary: "Protected branch (GitLab rules enforced)".to_string(),
					});
				}
			}
		}
		Ok(BranchProtectionStatus {
			branch,
			protected: false,
			summary: String::new(),
		})
	})
	.await
	.map_err(|err| format!("Branch protection check failed: {err}"))?
}
