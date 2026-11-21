// Dedicated per-user background process. OS service managers launch this
// executable directly; it remains independent from the Tauri desktop UI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    gitmanager_service::run();
}
