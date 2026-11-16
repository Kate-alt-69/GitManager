#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(crash_helper::run_helper_from_args());
}
