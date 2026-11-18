# GitManager crash_helper and background-service lifecycle

## Process boundaries

- The desktop executable stays alive as the process-tree root while its bundled crash_helper sidecar supervises the Tauri UI.
- crash_helper starts and supervises only the Tauri UI process.
- gitmanager.service is a separate executable launched directly by the OS service manager.
- The UI and helper never place the background service in their process tree or termination scope.
- Cloud Node keeps using its existing RBE Error Reporter; this feature does not duplicate it.

## UI health and recovery

The Tauri host starts a native runtime-monitor thread during setup. It reports the authenticated loopback heartbeat every three seconds, outside the WebView. The helper allows up to 90 seconds for first startup and treats 15 seconds without a heartbeat as an unresponsive native UI host. It requests a 120-second service suspension and gives the host another 15 seconds to recover before terminating the UI process tree. A later heartbeat resumes the service immediately. A clean UI exit does not suspend the service. If the host never returns, the service's own timer resumes background work automatically.

The monitor checks the repository-index and notification-history file generations once per second and checks the authenticated service endpoint every five seconds. It emits Tauri events only when those values change. React subscribes to those events; it no longer polls for index updates, notifications or service health. This heartbeat confirms the native Tauri host is responsive; it does not by itself prove that the WebView's JavaScript event loop is responsive.

The helper creates a separate process group for the UI on Linux and macOS and terminates that group on an emergency shutdown. On Windows, the original GitManager launcher remains alive while `crash_helper.exe` runs as its child and the Tauri UI runs as the helper's child. Neither child creates a new process group. The helper starts the UI suspended, assigns it to a Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resumes the primary thread. The Job Object—not process-group membership—is the authoritative crash-containment boundary: closing its handle on UI exit or timeout terminates only the UI and its descendants. The independent background service is never assigned to this job. Task Manager should now have a real process ancestry to display, but the exact grouping presentation still needs a Windows runtime check.

## Windows WebView2 compatibility

GitManager uses WebView2 to render its Windows UI. On systems where the WebView2 rendering or GPU path produces a blank window, launch the app with:

```powershell
& "C:\Path\To\GitManager.exe" --disable-webview-gpu
```

crash_helper consumes this switch and adds `--disable-gpu` to `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` for the supervised UI process. Existing browser arguments are preserved. GPU acceleration remains enabled by default; use this option as a troubleshooting fallback because software rendering can be slower.

## Service control

The required service exposes a loopback-only TCP endpoint on an ephemeral port (protocol version 2). Its address and random token are stored in the per-user GitManager data directory. The endpoint file is owner-readable/writable on Unix and inherits the current user's LocalAppData ACL on Windows. Authenticated requests and responses are bounded to 8 MiB. The protocol supports status, timed suspend/resume, repository-index refresh and snapshot/generation reads, immediate remote-check requests, repository overview/history/commit/file/worktree and local-preference operations, notification history/read/dismiss/bulk-read, and account/application-preference load/save. Account messages carry only non-secret account metadata and provider repository records; access tokens are not sent to the service.

The `Status` response also exposes `remote_check_pending` and `remote_check_running`. The native Tauri monitor emits a service-health event only when readiness, service identity/protocol, or these task states change; the WebView does not poll for remote-check progress.

Suspension is cooperative. The service stops scheduling new remote checks, archive watcher work, and cloud sync. Already-running transactional operations are allowed to reach their safe stopping points. The service process is not OS-frozen, so its control channel and expiry timer remain alive. The Tauri UI starts this service on demand and waits for its authenticated readiness response before enabling the workspace.

The default suspension is 120 seconds and requests are clamped to a 15–600 second range.

## Platform integration

- Windows: Task Scheduler starts gitmanager.service.exe directly.
- Linux: the systemd user unit starts gitmanager.service directly and uses a bounded restart policy.
- macOS: the LaunchAgent starts gitmanager.service directly and uses launchd throttling.

Both executables are packaged as Tauri sidecars. The build script stages target-triple-suffixed sidecars before Tauri packaging.
