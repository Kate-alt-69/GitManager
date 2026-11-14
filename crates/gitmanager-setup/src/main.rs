#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let app_path = match resolve_application_path() {
        Ok(path) => path,
        Err(error) => {
            show_error(&error);
            return ExitCode::from(2);
        }
    };

    match Command::new(&app_path).arg("--setup").spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            show_error(&format!(
                "GitManager Setup could not start the application at:\n{}\n\n{}",
                app_path.display(),
                error
            ));
            ExitCode::from(1)
        }
    }
}

fn resolve_application_path() -> Result<PathBuf, String> {
    // Normal first-run handoff: the Tauri app supplies its own exact executable path.
    if let Some(argument) = std::env::args_os().nth(1) {
        let supplied = PathBuf::from(argument);
        if supplied.is_file() {
            return Ok(supplied);
        }
        return Err(format!(
            "The GitManager application path supplied to Setup does not exist:\n{}",
            supplied.display()
        ));
    }

    // Direct launch from Explorer/Finder: look for the main app beside this helper.
    let helper = std::env::current_exe()
        .map_err(|error| format!("Could not locate GitManager Setup: {error}"))?;
    let helper_dir = helper
        .parent()
        .ok_or_else(|| "GitManager Setup has no parent directory.".to_string())?;

    let candidates = application_candidates(helper_dir);
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "GitManager Setup could not find the GitManager application next to itself.\n\n         Place the Setup helper beside the installed GitManager application, or start GitManager          normally so it can launch Setup with the correct application path.\n\n         Setup location: {}",
        helper.display()
    ))
}

fn application_candidates(helper_dir: &Path) -> Vec<PathBuf> {
    let mut directories = vec![helper_dir.to_path_buf()];
    if let Some(parent) = helper_dir.parent() {
        if parent != helper_dir {
            directories.push(parent.to_path_buf());
        }
    }

    #[cfg(windows)]
    let executable_names = ["git-manager.exe", "GitManager.exe"];
    #[cfg(target_os = "macos")]
    let executable_names = [
        "git-manager",
        "GitManager",
        "git-manager.app/Contents/MacOS/git-manager",
        "GitManager.app/Contents/MacOS/git-manager",
    ];
    #[cfg(all(unix, not(target_os = "macos")))]
    let executable_names = ["git-manager", "GitManager"];

    directories
        .into_iter()
        .flat_map(|directory| {
            executable_names
                .iter()
                .map(move |name| directory.join(name))
        })
        .collect()
}

fn show_error(message: &str) {
    #[cfg(windows)]
    show_windows_error(message);

    #[cfg(not(windows))]
    eprintln!("GitManager Setup: {message}");
}

#[cfg(windows)]
fn show_windows_error(message: &str) {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;

    let title: Vec<u16> = std::ffi::OsStr::new("GitManager Setup")
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let body: Vec<u16> = std::ffi::OsStr::new(message)
        .encode_wide()
        .chain(iter::once(0))
        .collect();

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            window: isize,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    // A native message box keeps release-mode GUI launches diagnosable even
    // when there is no console attached to the helper process.
    unsafe {
        MessageBoxW(0, body.as_ptr(), title.as_ptr(), 0x0000_0010);
    }
}
