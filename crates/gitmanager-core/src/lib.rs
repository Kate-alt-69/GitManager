//! Shared GitManager repository discovery and metadata engine.
//!
//! These modules are compiled into the desktop application and the headless
//! service without depending on Tauri or a WebView runtime.

pub mod git_info;
pub mod scanner;
