//! Compatibility exports for the shared account-provider data types.
//!
//! Provider API and GitHub CLI operations run exclusively in gitmanager.service.

pub use gitmanager_core::account_types::{
    ConnectedAccountSummary,
    GitHubIdentity,
    ProviderAccountRequest,
};
