# GitManager Core Service packaging

GitManager's `gitmanager.service` executable is the **required independent per-user data engine**, not a machine-wide Windows Service. The desktop app starts it on demand and waits for an authenticated readiness handshake before using the repository index. It remains outside the Tauri UI/crash-helper process tree and uses the signed-in user's permissions.

## Why this distinction matters

The worker calls `load_background_preferences()`, which resolves GitManager's configuration through `dirs::data_local_dir()`. It reads that user's preferences, scans that user's configured repository roots, performs per-user archive work and sends desktop notifications.

The OS logon-registration option also runs it as the signed-in user:

| Platform | Registration mechanism | Execution identity |
| --- | --- | --- |
| Windows | Task Scheduler logon task (`/SC ONLOGON`, limited run level) | Current interactive user |
| Linux desktop packages | systemd **user** unit | Current user |
| macOS | launchd LaunchAgent | Current user |

The desktop app always starts the service when the UI opens. Separate OS logon registration remains opt-in, allowing the service to run before the desktop UI is opened. Do not convert it into a machine-wide privileged service.

## Upgrade migration from the legacy worker

The first launch of the required core service checks its reported service generation. A generation-less response is treated as the PR #9 legacy worker, not as a ready core service. Before starting the replacement, GitManager removes the old per-user startup registration and stops the old executable:

- Windows Task Scheduler task: `GitManager Background Service`; process: `git-manager-service.exe`
- Linux systemd user unit: `gitmanager-updater.service`
- macOS LaunchAgent: `com.gitmanager.updater`

The Windows installer also closes the old `git-manager.exe` and `git-manager-service.exe` processes before replacing files. This prevents an old worker from retaining the shared service lock or continuing duplicate repository watches and notifications after an upgrade.

## Repository workspace ownership

All Git worktree operations now run in the required service through authenticated protocol-v3 IPC: repository status, diffs, fetch, stage/unstage, commit preview/create, safe path deletion, pull preview/preparation/execution, and branch list/switch. The shared gitmanager-core crate owns the implementation and keeps the existing path validation and recovery checks. The Tauri process forwards user intents and displays results; it does not open the working repository for these operations.

The service control listener has a hard cap of eight concurrent requests. This keeps status/health requests responsive during a longer history or Git operation without allowing unbounded request threads. Each repository uses a read/write lock: read-only history, file and status requests may run concurrently, while stage/unstage, commit, fetch, pull, delete and branch mutations take an exclusive lock. Archive capture takes a shared repository read lock plus a separate per-repository archive lock, so it cannot overlap a Git write but does not block ordinary reads.

## Repository read ownership

The required service performs Git repository overview reads, paginated commit-history traversal and individual commit-detail calculations. The Tauri command layer forwards those requests over the authenticated protocol-v4 local socket (Windows Named Pipe or Unix Domain Socket) and deserializes the result for the UI. Commit-history requests are bounded to 100 results maximum (the UI requests 50 at a time); branch and text filters execute in the service process.

## Preferences and repository inventory

The service owns the global preferences file at the existing per-user config path and validates its persisted bounds. The desktop performs a one-time migration from the former Tauri app-config location before service startup; normal preference loads and saves use authenticated IPC. Writes are serialized and use a recoverable temporary/previous file.

Repository discovery, indexing, the authoritative index snapshot and index generation all belong to the service. The UI requests a grouped snapshot and generation through IPC. The native Tauri monitor reads only the tiny monotonic `repo/metadata/index.generation` sidecar to emit change events; it does not deserialize the repository index. The service publishes the generation after each successful index replacement, and the desktop compares the integer to detect changes even on filesystems with coarse timestamp precision. Manual Check remotes requests are queued to the service loop and use the same notification/OS delivery path as scheduled checks.

## Account provider synchronization

GitHub CLI identity detection and GitHub/GitLab repository synchronization run in `gitmanager.service`. The Tauri process forwards the provider request over authenticated IPC and displays the returned account/repository summary. Personal access tokens are request-only values: the service uses them in memory for the HTTPS request, marks authorization headers sensitive, and does not store them in account metadata or logs. The persisted account record contains only non-secret provider identity and repository cache data. GitHub CLI credentials remain managed by the user's `gh` installation.

## Repository-local preferences

Per-repository settings are stored in the repository's `.git/info/gitmanager-settings.json` file. Reads, legacy fallback from the previous global preferences entry, validation, backup recovery and writes all run in `gitmanager.service` through authenticated IPC. The Tauri host no longer opens the working tree with libgit2 for settings operations and does not own the repository settings file.

## Archive operations

The required service owns archive capture, archive listing, restore, Cloud Node configuration and synchronization. Tauri forwards the existing command payloads over authenticated IPC and receives serialized results; it no longer links the archive engine directly or runs repository archive work in the desktop process. Archive capture/list operations share a per-repository read lock, so overview/history/file reads can proceed concurrently while commits, fetches, pulls, restores and branch changes wait for a consistent snapshot. Multiple archive operations for the same repository are serialized by a dedicated archive lock, and the archive engine's own per-repository file lock remains the cross-process storage guard. Long-running archive commands receive an extended IPC response timeout so large restore points do not get abandoned by the UI adapter.

## Notification lifecycle

The required service is the only process that publishes and mutates persistent notification history. The Tauri UI requests the history, mark-read, bulk mark-read and dismiss operations through authenticated protocol-v4 IPC. The notification file is capped at 200 records and 2 MiB; deterministic repository-update IDs prevent duplicate records across service restarts. A separate monotonic generation sidecar lets the native event monitor detect rapid history changes without reading or deserializing the notification file.

The In-app notifications and Operating system notifications preferences are independent. The service stores a repository-update notification once, then sends the same persisted title/body record to the OS only when OS notifications are enabled and that record was newly created. If persistence fails, the service logs the error and retries on the next remote-check cycle instead of emitting an OS toast without a matching in-app record. Disabling in-app notifications hides the in-app panel but does not implicitly disable OS notifications.

The top-right Notifications popover supports All/Unread filters, per-item open/read/dismiss actions, and bulk mark-read. The Task Center is a compact top-right popover for required-service health, the service-reported repository-indexing state, queued/running remote checks and bounded service-log output. The indexing row reflects the service's own scan lifecycle, including first-run index creation and automatic rebuilds, rather than only the UI's local scan button state. Notifications remains a sibling popover; neither surface is a full navigation page.

## Native runtime monitoring and idle behavior

The Tauri host owns one lightweight native monitor thread. It watches the repository-index and notification-history generation stamps and emits Tauri events when those files change. It checks the authenticated core-service endpoint every five seconds and reports the native host heartbeat every three seconds to crash_helper. The React/WebView frontend subscribes to these events rather than polling on intervals.

The monitor does not scan repository roots, enumerate Git history or query provider APIs. Those operations remain service/feature work and are started only when requested. The commit-history view loads 50 results per request through the shared Git history API; opening a repository overview loads branch metadata and HEAD without walking the history.

## Persistent data ownership

Before publishing its authenticated IPC-ready endpoint, the service creates the canonical per-user data layout:

```text
data/GitManager/
├── repo/
│   ├── metadata/       # Persisted local repository inventory
│   └── remote/
│       ├── metadata/   # Remote identities and synchronization metadata
│       └── git_data/   # Cached remote Git data
├── git_data/           # Local Git-derived indexes and cache data
├── account/
│   ├── metadata/       # Non-secret account identities
│   └── git_data/       # Per-account cached repository lists
├── notifications/      # Bounded in-app notification history
└── .cache/             # Disposable derived data
```

On Windows this is under `%LOCALAPPDATA%\\kastrick's_software\\data\\GitManager`; macOS and Linux use the current user's local application-data directory. The desktop and service share the same path resolver, but the service initializes the layout before it reports ready.

The repository index is an atomic, versioned snapshot at `repo/metadata/index.json`. Existing working copies and their `.git` directories remain the source of truth; GitManager does not duplicate those repositories into its data directory.

Account identity metadata and cached provider repository lists are stored separately under `account/metadata` and `account/git_data`. The Tauri coordinator sends the non-secret account summary and repository cache to `gitmanager.service` over authenticated protocol-v3 IPC; the service owns persistent reads and writes. Provider API requests remain in the coordinator for this iteration. Personal access tokens are not written to either location or included in account-cache IPC. GitHub CLI credentials remain managed by `gh`. On a later launch, cached identities are shown as cached until the user reconnects.

## Windows MSI and MSIX

The Windows desktop and Store package payloads include `gitmanager.service.exe` alongside the other application binaries. Packaging the executable does not mean registering it with the Windows Service Control Manager.

**Do not add this worker to MPDEV's top-level `services[]` definition.** MPDEV's Windows-service definition defaults to `LocalSystem`; its other built-in accounts are `LocalService` and `NetworkService`. None is the interactive user's profile. The MSI is deliberately per-user, and installing an always-on privileged service whose executable lives in that user's install tree would create an unsafe privilege boundary.

Keep the current per-user Task Scheduler registration for MSI as an optional sign-in action. Validate that registration, shutdown, uninstall and upgrade behavior on a clean Windows machine.

For the Microsoft Store MSIX, do not add a packaged Windows service merely to make the worker start at logon. Packaged services require restricted MSIX capabilities; `LocalSystem` additionally needs the `localSystemServices` capability. Microsoft advises that these capabilities are generally not approved for Store apps. Test the current per-user activation path with Store-identity MSIX, including an update between package versions. A scheduled task that points directly into a versioned WindowsApps directory may need re-registration after an MSIX update.

A future genuine machine-wide Windows service would need a separate design: a privileged service/broker plus a per-user agent for repository access, preferences and notifications. It must not reuse this per-user worker under `LocalSystem`.

## Linux packages and Flatpak

For native DEB/RPM packages, ship a systemd **user** unit with the distro package and expose optional sign-in registration through GitManager's settings. The unit targets the installed `gitmanager.service` binary and runs as the user. It must not be a system-wide root service.

Flatpak is a separate case. `/app/bin/gitmanager.service` exists inside the application's sandbox, not as a host executable path. A host systemd user manager cannot be given that path as a normal host `ExecStart`. A Flatpak-specific user unit would need to invoke the installed app through the host's Flatpak command, for example `flatpak run --command=gitmanager.service com.gitmanager.desktop`, and GitManager would need a narrowly justified way to request user-systemd control from inside the sandbox. Flatpak filters D-Bus access by default, so this permission and the resulting Flathub review must be tested rather than assumed.

The desktop launches `gitmanager.service` within the Flatpak sandbox while it is open. Detached sign-in startup remains dependent on the host systemd/Flatpak integration and must not be advertised as production-ready until the host invocation and minimal sandbox permissions are verified.

## macOS

Keep the existing per-user LaunchAgent model. It already matches the worker's access to the current user's home directory and preferences.

## Microsoft Store signing and identity

Partner Center supplies the Store app identity values; it does **not** issue a public code-signing PFX certificate for this workflow.

For Store MSIX submission:
- Set `MSIX_PUBLISHER` and `MSIX_IDENTITY_NAME` to the exact identity values shown in Partner Center.
- The Store accepts the MSIX without a CA-trusted developer certificate and signs it with Microsoft's certificate after certification.
- A locally generated test certificate is only for local installation testing; it is not a public distribution certificate.

For direct-download MSIX distribution outside the Store, use a certificate trusted by the target Windows devices. For MSI distribution, Authenticode-sign the installer and the included PE binaries for public release. The separate Microsoft Store MSI/EXE submission path also requires a CA-trusted signature and a silent installer; GitManager's planned Store distribution path is MSIX.

## Acceptance checks

- MSI carries the required service and installs it only for the current user; the desktop starts it on demand, while sign-in registration can install the current-user Task Scheduler task. A complete disable/uninstall lifecycle must remove only that named task without touching unrelated tasks.
- Store MSIX carries the required service but does not declare MPDEV `services[]` or restricted packaged-service capabilities.
- Store MSIX can be installed locally with the Partner Center identity and test certificate, and can be submitted without a CA-issued PFX; Microsoft signs it after certification.
- Native DEB/RPM sign-in user-unit lifecycle is validated on supported distributions.
- Flatpak worker registration is not considered complete until the host systemd invocation and minimal sandbox permissions have been verified.
- macOS LaunchAgent registration and clean removal are tested.
