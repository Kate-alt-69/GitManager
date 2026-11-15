// Standalone entry point for development and explicit Cargo invocations.
// Production OS integration launches the already-installed GitManager binary
// with --background-service, so installers do not need a second executable.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    git_manager_lib::background_service::run();
}
