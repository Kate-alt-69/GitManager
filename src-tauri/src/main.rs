// Prevents additional console window on Windows 10 and 11 in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let argument_is = |expected: &str| arguments.iter().any(|argument| argument.to_string_lossy() == expected);

    // Preserve compatibility with installations that still have the older
    // scheduled-task command while new installs use git-manager-service.
    if argument_is("--background-service") {
        git_manager_lib::background_service::run();
    }

    if argument_is("--setup") {
        std::env::set_var("GITMANAGER_SETUP_MODE", "1");
    }

    // Normal desktop launches are handed to crash_helper. The helper then
    // starts this executable as a child and remains alive after UI exit.
    if !argument_is("--crash-helper-child") {
        if let Ok(current_exe) = std::env::current_exe() {
            match crash_helper::launch_supervisor(&current_exe, &arguments) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => eprintln!("GitManager crash_helper could not start: {error}"),
            }
        }
    }

    git_manager_lib::run();
    crash_helper::report_ui_shutdown_from_env();
}
