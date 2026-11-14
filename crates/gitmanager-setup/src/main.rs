#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let Some(app_path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("GitManager Setup could not locate the installed GitManager application.");
        return ExitCode::from(2);
    };

    if !app_path.is_file() {
        eprintln!("GitManager Setup could not find the application at: {}", app_path.display());
        return ExitCode::from(2);
    }

    match Command::new(&app_path).arg("--setup").spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("GitManager Setup could not start GitManager: {error}");
            ExitCode::from(1)
        }
    }
}
