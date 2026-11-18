//! Compatibility exports for the shared account-provider data types.
//!
//! Provider API and GitHub CLI operations run exclusively in gitmanager.service.

pub use gitmanager_core::account_types::{
    ConnectedAccountSummary,
    GitHubIdentity,
    ProviderAccountRequest,
};


/// Authenticate with a provider, fetch its repository list and persist only
/// non-secret account metadata/cache. Access tokens remain transient request
/// data and are never written to the account store.
pub fn connect_account(
    request: ProviderAccountRequest,
) -> Result<ConnectedAccountSummary, String> {
    let use_github_cli = request.provider.eq_ignore_ascii_case("github")
        && request.username_or_host.trim().is_empty()
        && request.access_token.as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true);

    let summary = if use_github_cli {
        fetch_github_cli_account()?
    } else {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("Could not initialize the account provider runtime: {error}"))?;
        runtime.block_on(fetch_remote_account_repositories(request))?
    };

    let metadata = gitmanager_core::account_store::AccountMetadata {
        provider: summary.provider.clone(),
        account_login: summary.account_login.clone(),
        display_name: summary.display_name.clone(),
        email: summary.email.clone(),
        avatar_url: summary.avatar_url.clone(),
        profile_url: summary.profile_url.clone(),
        updated_at_unix_ms: 0,
    };
    gitmanager_core::account_store::save_account(&metadata, &summary.repositories)?;
    Ok(summary)
}
