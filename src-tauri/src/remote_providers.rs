use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};

use crate::git_info::{BranchInfo, CommitNode, GitRepositoryInfo, RemoteEndpoint};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAccountRequest {
    pub provider: String,         // "github" | "gitlab"
    pub username_or_host: String, // e.g. "octocat" or "https://gitlab.com"
    pub access_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedAccountSummary {
    pub provider: String,
    pub account_login: String,
    pub display_name: String,
    pub email: String,
    pub avatar_url: String,
    pub profile_url: String,
    pub repositories: Vec<GitRepositoryInfo>,
}

#[derive(Debug, Deserialize)]
struct GithubUserResponse {
    login: String,
    name: Option<String>,
    email: Option<String>,
    avatar_url: String,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubOwner {
    login: String,
    avatar_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRepoItem {
    id: u64,
    name: String,
    full_name: String,
    html_url: String,
    clone_url: String,
    default_branch: Option<String>,
    owner: GithubOwner,
    updated_at: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitlabUserResponse {
    id: u64,
    username: String,
    name: String,
    public_email: Option<String>,
    email: Option<String>,
    avatar_url: Option<String>,
    web_url: String,
}

#[derive(Debug, Deserialize)]
struct GitlabNamespace {
    path: String,
    name: String,
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitlabProjectItem {
    id: u64,
    name: String,
    path_with_namespace: String,
    web_url: String,
    http_url_to_repo: String,
    default_branch: Option<String>,
    avatar_url: Option<String>,
    last_activity_at: Option<String>,
    description: Option<String>,
    namespace: Option<GitlabNamespace>,
}

/// Connects to a GitHub or GitLab account (via personal access token or public username)
/// and fetches its remote repositories along with owner, email, and icon metadata.
pub async fn fetch_remote_account_repositories(
    req: ProviderAccountRequest,
) -> Result<ConnectedAccountSummary, String> {
    let provider = req.provider.trim().to_lowercase();
    match provider.as_str() {
        "github" => fetch_github_account(req).await,
        "gitlab" => fetch_gitlab_account(req).await,
        other => Err(format!("Unsupported provider '{}'. Use 'github' or 'gitlab'.", other)),
    }
}

async fn fetch_github_account(req: ProviderAccountRequest) -> Result<ConnectedAccountSummary, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("GitManager-Tauri2/1.0"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );

    let has_token = req
        .access_token
        .as_ref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);

    if let Some(ref token) = req.access_token {
        if !token.trim().is_empty() {
            let auth_val = format!("Bearer {}", token.trim());
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&auth_val).map_err(|e| e.to_string())?,
            );
        }
    }

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| e.to_string())?;

    let user_url = if has_token && req.username_or_host.trim().is_empty() {
        "https://api.github.com/user".to_string()
    } else {
        format!(
            "https://api.github.com/users/{}",
            req.username_or_host.trim()
        )
    };

    let user_resp = client
        .get(&user_url)
        .send()
        .await
        .map_err(|e| format!("GitHub request failed: {}", e))?;

    if !user_resp.status().is_success() {
        return Err(format!(
            "GitHub API returned status {} when fetching profile",
            user_resp.status()
        ));
    }

    let gh_user: GithubUserResponse = user_resp.json().await.map_err(|e| e.to_string())?;

    let repos_url = if has_token && req.username_or_host.trim().is_empty() {
        "https://api.github.com/user/repos?per_page=100&sort=updated".to_string()
    } else {
        format!(
            "https://api.github.com/users/{}/repos?per_page=100&sort=updated",
            gh_user.login
        )
    };

    let repos_resp = client
        .get(&repos_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch GitHub repos: {}", e))?;

    let gh_repos: Vec<GithubRepoItem> = repos_resp.json().await.map_err(|e| e.to_string())?;

    let owner_email = gh_user
        .email
        .clone()
        .unwrap_or_else(|| format!("{}@users.noreply.github.com", gh_user.login));
    let display_name = gh_user.name.clone().unwrap_or_else(|| gh_user.login.clone());

    let mapped_repos = gh_repos
        .into_iter()
        .map(|item| {
            let default_branch = item.default_branch.unwrap_or_else(|| "main".to_string());
            let head_oid = format!("gh-{}-head", item.id);
            let date_iso = item
                .updated_at
                .unwrap_or_else(|| "2026-01-01T00:00:00Z".to_string());

            GitRepositoryInfo {
                id: format!("github::{}", item.full_name),
                name: item.name.clone(),
                path: format!("github.com/{}", item.full_name),
                git_dir: format!("remote://github.com/{}.git", item.full_name),
                parent_folder: format!("GitHub / @{}", item.owner.login),
                owner_name: display_name.clone(),
                owner_email: owner_email.clone(),
                current_branch: default_branch.clone(),
                has_remote: true,
                primary_provider: "github".to_string(),
                primary_remote_url: Some(item.clone_url.clone()),
                web_url: Some(item.html_url.clone()),
                icon_url: item.owner.avatar_url.clone(),
                remotes: vec![RemoteEndpoint {
                    name: "origin".to_string(),
                    url: item.clone_url,
                    provider: "github".to_string(),
                    host: "github.com".to_string(),
                    owner: Some(item.owner.login),
                    repo_slug: Some(item.name),
                    web_url: Some(item.html_url),
                    icon_url: item.owner.avatar_url,
                }],
                branches: vec![BranchInfo {
                    name: default_branch.clone(),
                    is_head: true,
                    is_remote: true,
                    target_oid: head_oid.clone(),
                    upstream: Some(format!("origin/{}", default_branch)),
                }],
                commits: vec![CommitNode {
                    oid: head_oid,
                    short_oid: "remote".to_string(),
                    message: item
                        .description
                        .clone()
                        .unwrap_or_else(|| "Remote repository indexed via GitHub API".to_string()),
                    summary: item
                        .description
                        .unwrap_or_else(|| "Remote repository head on GitHub".to_string()),
                    author_name: display_name.clone(),
                    author_email: owner_email.clone(),
                    timestamp: 0,
                    date_iso,
                    parents: vec![],
                    branches: vec![default_branch],
                    tags: vec!["remote".to_string()],
                    is_head: true,
                    lane: 0,
                }],
                dirty_files_count: 0,
                total_commits_loaded: 1,
                source_kind: "remote_account".to_string(),
            }
        })
        .collect();

    Ok(ConnectedAccountSummary {
        provider: "github".to_string(),
        account_login: gh_user.login,
        display_name,
        email: owner_email,
        avatar_url: gh_user.avatar_url,
        profile_url: gh_user.html_url,
        repositories: mapped_repos,
    })
}

async fn fetch_gitlab_account(req: ProviderAccountRequest) -> Result<ConnectedAccountSummary, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("GitManager-Tauri2/1.0"));

    let has_token = req
        .access_token
        .as_ref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);

    if let Some(ref token) = req.access_token {
        if !token.trim().is_empty() {
            let auth_val = format!("Bearer {}", token.trim());
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&auth_val).map_err(|e| e.to_string())?,
            );
        }
    }

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| e.to_string())?;

    let gl_user: GitlabUserResponse = if has_token && req.username_or_host.trim().is_empty() {
        let resp = client
            .get("https://gitlab.com/api/v4/user")
            .send()
            .await
            .map_err(|e| format!("GitLab API error: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("GitLab API returned {}", resp.status()));
        }
        resp.json().await.map_err(|e| e.to_string())?
    } else {
        let search_url = format!(
            "https://gitlab.com/api/v4/users?username={}",
            req.username_or_host.trim()
        );
        let resp = client
            .get(&search_url)
            .send()
            .await
            .map_err(|e| format!("GitLab API error: {}", e))?;
        let mut users: Vec<GitlabUserResponse> = resp.json().await.map_err(|e| e.to_string())?;
        users
            .pop()
            .ok_or_else(|| format!("GitLab user '{}' not found", req.username_or_host))?
    };

    let projects_url = format!(
        "https://gitlab.com/api/v4/users/{}/projects?per_page=100&order_by=updated_at",
        gl_user.id
    );
    let proj_resp = client
        .get(&projects_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch GitLab projects: {}", e))?;
    let gl_projects: Vec<GitlabProjectItem> = proj_resp.json().await.map_err(|e| e.to_string())?;

    let owner_email = gl_user
        .email
        .or(gl_user.public_email)
        .unwrap_or_else(|| format!("{}@users.noreply.gitlab.com", gl_user.username));
    let avatar_url = gl_user
        .avatar_url
        .unwrap_or_else(|| format!("https://gitlab.com/{}.png?size=128", gl_user.username));

    let mapped_repos = gl_projects
        .into_iter()
        .map(|proj| {
            let default_branch = proj.default_branch.unwrap_or_else(|| "main".to_string());
            let head_oid = format!("gl-{}-head", proj.id);
            let icon = proj
                .avatar_url
                .or_else(|| proj.namespace.as_ref().and_then(|n| n.avatar_url.clone()))
                .unwrap_or_else(|| avatar_url.clone());

            GitRepositoryInfo {
                id: format!("gitlab::{}", proj.path_with_namespace),
                name: proj.name.clone(),
                path: format!("gitlab.com/{}", proj.path_with_namespace),
                git_dir: format!("remote://gitlab.com/{}.git", proj.path_with_namespace),
                parent_folder: format!("GitLab / @{}", gl_user.username),
                owner_name: gl_user.name.clone(),
                owner_email: owner_email.clone(),
                current_branch: default_branch.clone(),
                has_remote: true,
                primary_provider: "gitlab".to_string(),
                primary_remote_url: Some(proj.http_url_to_repo.clone()),
                web_url: Some(proj.web_url.clone()),
                icon_url: icon.clone(),
                remotes: vec![RemoteEndpoint {
                    name: "origin".to_string(),
                    url: proj.http_url_to_repo,
                    provider: "gitlab".to_string(),
                    host: "gitlab.com".to_string(),
                    owner: Some(gl_user.username.clone()),
                    repo_slug: Some(proj.name),
                    web_url: Some(proj.web_url),
                    icon_url: icon,
                }],
                branches: vec![BranchInfo {
                    name: default_branch.clone(),
                    is_head: true,
                    is_remote: true,
                    target_oid: head_oid.clone(),
                    upstream: Some(format!("origin/{}", default_branch)),
                }],
                commits: vec![CommitNode {
                    oid: head_oid,
                    short_oid: "remote".to_string(),
                    message: proj
                        .description
                        .clone()
                        .unwrap_or_else(|| "Remote project indexed via GitLab API".to_string()),
                    summary: proj
                        .description
                        .unwrap_or_else(|| "Remote project head on GitLab".to_string()),
                    author_name: gl_user.name.clone(),
                    author_email: owner_email.clone(),
                    timestamp: 0,
                    date_iso: proj.last_activity_at.unwrap_or_default(),
                    parents: vec![],
                    branches: vec![default_branch],
                    tags: vec!["remote".to_string()],
                    is_head: true,
                    lane: 0,
                }],
                dirty_files_count: 0,
                total_commits_loaded: 1,
                source_kind: "remote_account".to_string(),
            }
        })
        .collect();

    Ok(ConnectedAccountSummary {
        provider: "gitlab".to_string(),
        account_login: gl_user.username,
        display_name: gl_user.name,
        email: owner_email,
        avatar_url,
        profile_url: gl_user.web_url,
        repositories: mapped_repos,
    })
}
