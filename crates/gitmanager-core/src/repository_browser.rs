//! Compatibility exports for the shared repository workspace file-browser API.
//!
//! The implementation and path-safety checks live in repository_workspace so
//! the required service has one authoritative implementation for local and
//! remote file listing and preview operations.

pub use crate::models::RepositoryFileEntry;
pub use crate::repository_workspace::{list_repository_files, read_repository_file};
