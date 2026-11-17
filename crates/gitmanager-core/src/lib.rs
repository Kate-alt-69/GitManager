//! Shared GitManager repository discovery and metadata engine.
//!
//! These modules are compiled into the desktop application and the headless
//! service without depending on Tauri or a WebView runtime.

#[path = "../../../src-tauri/src/git_info.rs"]
pub mod git_info;

#[path = "../../../src-tauri/src/scanner.rs"]
pub mod scanner;
