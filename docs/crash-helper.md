# GitManager crash_helper and background-service lifecycle

## Process boundaries

GitManager starts **two independent instances of the same `crash_helper` executable**:

- `crash_helper.exe --supervise` supervises the Tauri desktop UI, watches its UI heartbeat and owns the UI crash-containment boundary.
- `crash_helper.exe --supervise-service` supervises the required `gitmanager.service.exe` data engine, owns its separate supervisor lock and restarts it after an unexpected exit.

The service supervisor is started before the UI supervisor. It launches `gitmanager.service.exe` outside the UI's kill-on-close Job Object, so closing or recovering the desktop UI does not terminate the data engine. Both helper instances have the same executable name but different command-line modes and separate ownership locks.

Optional sign-in registration (Task Scheduler on Windows, a systemd user unit on Linux, or a LaunchAgent on macOS) can start the service before the desktop opens. It shares the service's per-user single-instance lock; the service supervisor detects an already-running instance instead of starting a duplicate.

**Expected Windows Task Manager entries:** two `crash_helper.exe` processes (UI supervisor and service supervisor) and one `gitmanager.service.exe`. A normal supervised desktop launch can also show two `gitmanager.exe` processes: the original launcher waiting for the UI supervisor, and the supervised Tauri UI child. The two helpers are intentional; they perform different jobs. Cloud Node keeps using its existing RBE Error Reporter; this feature does not duplicate it.

## UI health and recovery

The Tauri host starts a native runtime-monitor thread during setup. It reports the authenticated loopback heartbeat every three seconds, outside the WebView. The helper allows up to 90 seconds for first startup and treats 15 seconds without a heartbeat as an unresponsive native UI host. It requests a 120-second service suspension and gives the host another 15 seconds to recover before terminating the UI process tree. A later heartbeat resumes the service immediately. A clean UI exit does not suspend the service. If the host never returns, the service's own timer resumes background work automatically.

The monitor checks the repository-index and notification-history file generations once per second and checks the authenticated service endpoint every five seconds. It emits Tauri events only when those values change. React subscribes to those events; it no longer polls for index updates, notifications or service health. This heartbeat confirms the native Tauri host is responsive; it does not by itself prove that the WebView's JavaScript event loop is responsive.

The helper creates a separate process group for the UI on Linux and macOS and terminates that group on an emergency shutdown. On Windows, the original GitManager launcher remains alive while `crash_helper.exe` runs as its child and the Tauri UI runs as the helper's child. Neither child creates a new process group. The UI supervisor starts the UI suspended, assigns it to a Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resumes the primary thread. The Job Object—not process-group membership—is the authoritative crash-containment boundary: closing its handle on UI exit or timeout terminates only the UI and its descendants. The service supervisor and `gitmanager.service.exe` are never assigned to this job.

## Windows WebView2 compatibility

GitManager uses WebView2 to render its Windows UI. On systems where the WebView2 rendering or GPU path produces a blank window, launch the app with:

```powershell
& "C:\Path\To\GitManager.exe" --disable-webview-gpu
```

crash_helper consumes this switch and adds `--disable-gpu` to `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` for the supervised UI process. Existing browser arguments are preserved. GPU acceleration remains enabled by default; use this option as a troubleshooting fallback because software rendering can be slower.

## Service control

The required service exposes a per-user local IPC endpoint (protocol version 4): a Windows Named Pipe on Windows and a Unix Domain Socket on Linux/macOS. The socket identity is derived from the user's GitManager data directory; Unix sockets live in a private `0700` directory under the system temporary directory. The endpoint descriptor and random authentication token are stored in the per-user GitManager data directory; the descriptor is owner-readable/writable on Unix and inherits the current user's LocalAppData ACL on Windows. Requests and responses use a four-byte length prefix and are bounded to 8 MiB. The service accepts at most 32 concurrent authenticated requests, with per-repository read/write locks.

On Windows, the client uses Interprocess' Tokio named-pipe adapter, which performs overlapped I/O through kernel completion notifications instead of enabling legacy `PIPE_NOWAIT` mode. This prevents an empty nonblocking read from being mistaken for a completed response. Each status connection also captures the server PID directly from the named-pipe handle and checks it against the PID in the authenticated response. If an older service holds the single-instance lock but its lock file has no PID, the supervisor can use that kernel-reported PID; before recovery it verifies the full executable path against the packaged `gitmanager.service.exe` and pins the process handle to avoid PID-reuse races. The protocol supports status, timed suspend/resume, repository-index refresh and snapshot/generation reads, immediate remote-check requests, repository overview/history/commit/file/worktree and local-preference operations, archive capture/list/restore and cloud synchronization, GitHub CLI identity and GitHub/GitLab provider synchronization, notification history/read/dismiss/bulk-read, and account/application-preference load/save. Account-cache load/save messages carry only non-secret account metadata and provider repository records. A provider access token may be included in a single authenticated connect request; the service consumes it in memory for the HTTPS request and never writes it to account metadata/cache or service logs.

The `Status` response also exposes `remote_check_pending` and `remote_check_running`. The native Tauri monitor emits a service-health event only when readiness, service identity/protocol, or these task states change; the WebView does not poll for remote-check progress.

Suspension is cooperative. The service stops scheduling new remote checks, archive watcher work, and cloud sync. Already-running transactional operations are allowed to reach their safe stopping points. The service process is not OS-frozen, so its control channel and expiry timer remain alive. The service supervisor starts this service independently of the Tauri UI and waits for its authenticated readiness response. The UI then verifies that service through authenticated IPC before enabling the workspace.

The default suspension is 120 seconds and requests are clamped to a 15–600 second range.

## Platform integration

- Windows: Task Scheduler starts gitmanager.service.exe directly.
- Linux: the systemd user unit starts gitmanager.service directly and uses a bounded restart policy.
- macOS: the LaunchAgent starts gitmanager.service directly and uses launchd throttling.

Both executables are packaged as Tauri sidecars. The build script stages target-triple-suffixed sidecars before Tauri packaging.

Older protocol-v1, protocol-v2 and protocol-v3 workers are accepted only for a status handshake so the desktop can identify and replace them; no newer storage or Git commands are sent over the legacy TCP transport.
