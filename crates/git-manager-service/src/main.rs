// Dedicated background service executable. Supports both Windows SCM
// (`services.msc`) and per-user OS service managers / crash_helper launches,
// remaining independent from the Tauri desktop UI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
	if gitmanager_service::try_run_as_windows_service() {
		return;
	}
	gitmanager_service::run();
}
