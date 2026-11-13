# GitManager platform UI boundary

Status: prepared integration boundary; the wl_ui host API is not wired into this repository yet.

## Intended target selection

- **Windows builds:** use WinLook's wl_ui library. Its Windows-side host will select the WinUI 3 or Win32 implementation supported by the target OS/build. GitManager must not hard-code the UI backend that belongs to wl_ui.
- **Linux and macOS builds:** continue using the existing Tauri 2 desktop host. These targets must not resolve or compile Windows-only wl_ui dependencies.

## Integration boundary

Keep GitManager's repository scanner, Git operations, account providers, preferences, update service and cleanup commands in the Rust application backend. The selected UI host should consume those same application operations through a narrow adapter; no repository logic should move into a page or UI framework module.

When wl_ui publishes its application-host API, wire it only behind a Windows target-specific dependency and adapter. Do not call the current standalone wl_ui::run() from GitManager: that entry point starts WinLook's own Manager shell and would replace GitManager's UI.

Until that API lands, the Tauri shell remains the buildable host on every CI target. This is deliberate staging, not a claim that the Windows-native wl_ui frontend is already active.

## Required checks when the API arrives

- Windows 10 and 11: compile the wl_ui Windows host and verify its documented WinUI 3 / Win32 backend selection.
- Non-Windows: build without fetching or compiling Windows-only wl_ui/Windows Reactor crates.
- Run the same scanner, preferences, account and background-service tests across host variants.
- Verify no duplicate application windows or duplicated Tauri command runtimes are started by the native UI host.
