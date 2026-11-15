# GitManager crash_helper and background-service lifecycle

## Process boundaries

- The desktop executable hands off to the bundled crash_helper sidecar.
- crash_helper starts and supervises only the Tauri UI process.
- git-manager-service is a separate executable launched directly by the OS service manager.
- The UI and helper never place the background service in their process tree or termination scope.
- Cloud Node keeps using its existing RBE Error Reporter; this feature does not duplicate it.

## UI health and recovery

The frontend reports an authenticated loopback heartbeat every three seconds after the GitManager hook has completed initialization. The helper allows up to 90 seconds for first startup and treats 15 seconds without a heartbeat as unresponsive. It requests a 120-second service suspension and gives the UI another 15 seconds to recover before terminating the UI process tree. A later heartbeat resumes the service immediately. A clean UI exit does not suspend the service. If the UI never returns, the service's own timer resumes background work automatically.

The helper creates a separate process group for the UI on Linux and macOS and terminates that group on an emergency shutdown. On Windows it uses the system task-tree termination command for the UI PID. The background service is launched independently by its OS service manager and is never part of that tree.

## Service control

The service exposes a loopback-only TCP endpoint on an ephemeral port. Its address and random token are stored in the per-user GitManager data directory. The endpoint file is owner-readable/writable on Unix and inherits the current user's LocalAppData ACL on Windows. Requests are bounded, authenticated, and limited to status, timed suspend, and resume.

Suspension is cooperative. The service stops scheduling new remote checks, archive watcher work, and cloud sync. Already-running transactional operations are allowed to reach their safe stopping points. The service process is not OS-frozen, so its control channel and expiry timer remain alive.

The default suspension is 120 seconds and requests are clamped to a 15–600 second range.

## Platform integration

- Windows: Task Scheduler starts git-manager-service.exe directly.
- Linux: the systemd user unit starts git-manager-service directly and uses a bounded restart policy.
- macOS: the LaunchAgent starts git-manager-service directly and uses launchd throttling.

Both executables are packaged as Tauri sidecars. The build script stages target-triple-suffixed sidecars before Tauri packaging.
