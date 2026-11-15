// Prevents additional console window on Windows 10 and 11 in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsString;

fn show_supervisor_error(message: &str) {
    #[cfg(windows)]
    {
        use std::iter;
        use std::os::windows::ffi::OsStrExt;

        let title: Vec<u16> = std::ffi::OsStr::new("GitManager could not start")
            .encode_wide()
            .chain(iter::once(0))
            .collect();
        let body: Vec<u16> = std::ffi::OsStr::new(message)
            .encode_wide()
            .chain(iter::once(0))
            .collect();

        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(window: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }

        unsafe {
            MessageBoxW(0, body.as_ptr(), title.as_ptr(), 0x0000_0010);
        }
    }
    #[cfg(not(windows))]
    eprintln!("{message}");
}

fn fail_closed(message: &str) -> ! {
    crash_helper::report_supervisor_launch_failure(message);
    show_supervisor_error(message);
    std::process::exit(2);
}

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<OsString>>();
    let is_supervised_child = crash_helper::is_supervised_child(&arguments);

    // The normal release executable is only a handoff. The crash helper then
    // owns the UI process; the independent background service is never its child.
    if !is_supervised_child {
        if let Ok(current_exe) = std::env::current_exe() {
            match crash_helper::launch_supervisor(&current_exe, &arguments) {
                Ok(true) => return,
                Ok(false) if cfg!(debug_assertions) => {
                    // Unbundled source checkouts remain runnable without sidecars.
                }
                Ok(false) => fail_closed(
                    "GitManager's crash_helper companion is missing. Reinstall GitManager or rebuild its sidecars.",
                ),
                Err(error) if cfg!(debug_assertions) => {
                    eprintln!("GitManager crash_helper could not start: {error}");
                }
                Err(error) => fail_closed(&format!("GitManager crash_helper could not start: {error}")),
            }
        } else if !cfg!(debug_assertions) {
            fail_closed("GitManager could not resolve its executable path to start crash_helper.");
        }
    }

    git_manager_lib::run();
    crash_helper::report_ui_shutdown_from_env();
}
