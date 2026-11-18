//! Data-transfer types for provider account synchronization.
//!
//! Provider access tokens are request-only values. They must never be included
//! in account metadata or written to persistent storage.

use crate::git_info::GitRepositoryInfo;
use serde::{Deserialize, Serialize};

/// Ephemeral provider sync request. The access token is consumed in memory by
/// the core service and is deliberately excluded from persisted account data.
#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderAccountRequest {
    pub provider: String,
    pub username_or_host: String,
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
    #[serde(default)]
    pub session_connected: bool,
    pub repositories: Vec<GitRepositoryInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIdentity {
    pub login: String,
    pub display_name: String,
    pub organizations: Vec<String>,
}
