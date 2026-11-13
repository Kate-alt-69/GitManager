//! Shared repository archive implementation lives in the standalone
//! `gitmanager-archive` crate so the desktop app and the `gitm` CLI use
//! exactly the same snapshot and restore rules.

pub use gitmanager_archive::*;
