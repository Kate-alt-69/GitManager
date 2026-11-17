//! Shared GitManager repository discovery, metadata and notification storage.
//!
//! This crate is Tauri-free and contains only native Git/filesystem-facing
//! modules. Both the desktop library and service host consume it; the current
//! service runtime entry point still reuses the desktop library for its
//! higher-level archive and platform-integration modules.

pub mod git_info;
pub mod scanner;
pub mod notification_center;
