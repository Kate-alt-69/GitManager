# GitManager

**By Kastrick** · Native Git Workspace

![GitManager logo](frontend/static/image/git-managar-orange_default-black.png)

**GitManager** is a **free, open-source** cross-platform Rust desktop application for local Git repositories, remote provider accounts, commit history and background repository updates. It is released under the **Apache License 2.0**. No subscription, paid tier or account is required to use the app. It uses top-and-bottom tab navigation with no permanent sidebar. The Repositories page is the local-machine homepage; GitHub/GitLab account repositories live separately under Accounts. The current Tauri 2 shell is the portable host while the Windows-specific wl_ui hosting contract is prepared for its upcoming WinUI 3 / Win32 backends.

---

## 1. Smart Python Build & Dependency Script (`build.py`)

`build.py` checks your machine's dependencies before building or running:

- **Rust Toolchain Detection & Version Adaptation**:
  - Checks if `rustc` and `cargo` are installed in `PATH` or `~/.cargo/bin`.
  - Compares the installed version against the workspace baseline (`1.88.0`, required by the filesystem watcher). If a **different version is already installed** on your machine (newer, custom/nightly, or older), `build.py` **automatically uses the version you already have installed** (or picks the best already-installed `rustup` toolchain if multiple exist locally) instead of failing or forcing a toolchain re-download.
- **Pure-Rust `tauri-cli` (`cargo tauri`) Detection**:
  - Uses the Rust `cargo-tauri` crate binary if already installed, or builds/runs the Rust crate directly with `cargo build --release` / `cargo run` (pass `--install-tauri-cli` if you want `cargo install tauri-cli` run automatically).
- **OS Native Library Check**:
  - Checks WebView2/Win32 on Windows 10/11, `webkit2gtk-4.1` / `gtk+-3.0` / `openssl` via `pkg-config` on Linux, and Xcode Command Line Tools on macOS.

```bash
# Check installed Rust/Tauri/OS dependencies only
python3 build.py deps

# Run in development mode (uses your already-installed Rust toolchain)
python3 build.py dev

# Build optimized native release binaries (GitManager GUI with built-in headless background mode, plus CLI/setup sidecars)
python3 build.py build

# Prepare the isolated CPython runtime used by Windows installers
python3 build.py prepare-runtime
```

---

## 2. Background Repository Update Service (User-Level OS Service + Loading Spinner)

Implemented in `src-tauri/src/background_service.rs` and `src-tauri/src/watcher_service.rs`. The installed GitManager executable enters this worker with `--background-service`; the standalone `git-manager-service` Cargo target is only a thin development wrapper:

- **OS Service Registration & Background Worker**:
  - **Windows 10 & 11**: Registers a current-user Task Scheduler task that launches the installed GitManager executable with `--background-service` at logon and starts it immediately. It no longer uses the experimental LocalSystem `GitManagerUpdateSvc`.
  - **Linux**: Registers and starts `~/.config/systemd/user/gitmanager-updater.service` via `systemctl --user`, launching the same installed executable in headless mode.
  - **macOS**: Registers and loads `~/Library/LaunchAgents/com.gitmanager.updater.plist` via `launchctl`, launching the same installed executable in headless mode.
- **Automatic Update Notification Prompt**:
  - Periodically polls remote `origin` heads (`libgit2` fetch / `ls-remote`) across all indexed repositories.
  - Whenever a repository has new upstream commits, it triggers both an OS desktop notification and an in-app interactive banner asking:
    > **"Hay you have `<X-REPO_NAME>` update do you want to update it?"**
- **Clear Animated Loading Spinner**:
  - Displays the GitManager loading mark: the orange ring rotates around the stationary center PNG while scanning remotes or when clicking **"Yes, Update Now"** to pull/fast-forward the repository.

---

## 3. Core GitManager Features

1. **Machine-Wide `.git` Scanner & Folder Indexer (`src-tauri/src/scanner.rs`)**
   - Recursively scans your machine for `.git` directories and indexes repositories by **Parent Folder**, **`.git` Directory Location**, or **Remote Provider**.
2. **Full Topological Git Commit Tree (`src-tauri/src/git_info.rs` & `ui/app.js`)**
   - Walks `refs/heads/*`, `refs/remotes/*`, and tags in topological + chronological order and renders an interactive multi-lane SVG commit graph.
3. **Repository Inspector & Remote Avatar Resolution**
   - Clicking any repository displays its **Git Name**, **Owner Name**, **Owner Email**, and **Repository Icon** (from `github.com/<owner>.png`, `gitlab.com/<owner>.png`, or fallback SVG badge).
4. **GitHub & GitLab Account Integration (`src-tauri/src/remote_providers.rs`)**
   - Connect specific **GitHub** or **GitLab** accounts to view and filter remote repositories.
5. **Smart Remote & Local Availability Filter**
   - Filter by **Remote** (available) and **Local** (no remote configured), with automatic fallback that **groups both into 1 unified view** whenever either side has `0` repositories.


---

## 4. Preview-first Developer Cleaner (tools/gitmanager_cleaner.py)

GitManager includes a cross-platform Python helper based on the safe cache/project cleanup model from the DevToolchain-Clean PowerShell script.

The Developer Cleaner button offers three scopes:

- **Selected local repository** — detects generated outputs for Node.js, Rust, Python, .NET, Gradle, Maven, CMake, Dart/Flutter, and Zig projects inside the selected worktree. Git-tracked paths are skipped by default. node_modules, project package caches, and validated Python virtual environments are opt-in.
- **Current-user developer caches** — checks known npm/pnpm/Yarn/Bun, Cargo/Rustup, pip/uv/Poetry, Go, Gradle, NuGet, and related cache locations. It does not scan entire drives or remove installed toolchains. Curated Windows update/temp/diagnostic folders are separately opt-in; GPU, shader, font, and thumbnail caches are not targeted.
- **Remote repository** — clones the selected remote into a temporary directory, previews recognized generated/dependency paths, creates a commit with the message "gitManager | Cleaned repo of junk", and pushes a new gitmanager/clean/<timestamp> branch. It never pushes directly to the default branch.

The UI always previews candidates and estimated size first. Applying cleanup requires the exact confirmation YES. Remote cleanup additionally requires a successful push; if permissions or credentials do not allow the push, the app reports the error instead of changing the remote default branch.

### CLI examples

    # Preview a local repository (no deletion)
    python tools/gitmanager_cleaner.py plan --scope repository --path .

    # Preview and include project dependencies such as node_modules
    python tools/gitmanager_cleaner.py plan --scope repository --path . --include-dependencies

    # Apply repository cleanup after reviewing the plan
    python tools/gitmanager_cleaner.py apply --scope repository --path . --confirm YES

    # Preview known developer caches under the current user profile
    python tools/gitmanager_cleaner.py plan --scope system

    # Apply current-user cache cleanup
    python tools/gitmanager_cleaner.py apply --scope system --confirm YES

    # Preview a remote repository
    python tools/gitmanager_cleaner.py remote-plan --url https://github.com/owner/repository.git

    # Push remote cleanup as a separate branch after explicit confirmation
    python tools/gitmanager_cleaner.py remote-clean --url https://github.com/owner/repository.git --confirm YES --push

Add --include-virtualenvs to include validated Python environments. --include-windows-junk is available only on Windows for the curated system-junk targets.

### Per-repository exclusions and ignore rules

A repository can add a .gitmanager-cleanignore file at its root with one relative path or glob pattern per line to exclude cleanup candidates for that repository. Existing .gitignore rules are reported in the preview, and remote cleanup appends a clearly marked GitManager-managed block to .gitignore only when it actually removes tracked generated outputs. Existing user-authored ignore rules are preserved.

The Windows setup installer includes a pinned, checksum-verified CPython 3.14.10 embeddable runtime as an app-local resource. The NSIS post-install hook runs after the installer has extracted the application files and verifies that the private interpreter starts successfully. GitManager launches that bundled runtime without opening a console window; it does not install Python machine-wide or modify PATH. Development builds can prepare the same runtime with `python build.py prepare-runtime`. Linux and macOS use an installed Python 3.10+ interpreter for cleaner operations. Python remains isolated to the Developer Cleaner; the desktop app itself is Rust/Tauri.


---

## 5. Workspace Setup & Repository Dashboard

On first launch, GitManager asks which directories to index. Add one or more folders such as `D:/Project_Customs`, or choose a drive root such as `D:/` when a broader scan is intended. GitManager persists these preferences in its per-user application configuration directory; it does not automatically scan every system drive.

- **Configurable roots and depth** — add/remove scan folders, restore recommended development folders, and choose a scan depth from 2 to 16 (8 by default).
- **Dependency repositories stay indexed** — Git repositories found inside known dependency/build folders are classified separately. The scanner limits how deeply it descends into those folders so it can find nested package repositories without crawling every file in a dependency tree.
- **Projects / Dependencies / All** — the main workspace defaults to Projects. Switch to Dependencies or All to inspect the indexed package/vendor repositories.
- **Repository cards** — show the remote owner/repository name when available, latest local commit summary, current branch, and CI status. Public GitHub commit status and GitLab pipeline APIs are queried for recent local commit SHAs; statuses can show unavailable when a project is private, has no public status, or the provider cannot be queried.
- **Persistent setup** — change scan folders and discovery options later from Scan Setup in the toolbar.

CI lookups are read-only and use public provider endpoints; they do not trigger or rerun pipelines.

Per-repository Auto Update and unattended Keep Remote Clean / Commit Clean automation are not enabled by this dashboard stage. Those policies need their own explicit settings and safe execution flow before they can perform background pulls or remote writes.


### Per-repository Auto Update

Local repositories expose an **Auto Update** preference in their inspector. When enabled, the in-app watcher and standalone OS service may fast-forward that repository automatically only when the working tree is clean and the local branch has no commits missing from the remote. GitManager uses fast-forward-only update semantics; dirty or divergent repositories remain manual notifications. The background watcher uses the scan roots saved during first-run setup.

Keep Remote Clean and Commit Clean remain separate follow-up features; they are not presented as active automation controls in this build.


### Per-repository Commit Clean

The repository inspector can install **Commit Clean** as a Git `pre-commit` hook for a locally indexed repository. On commit, it detects tracked generated/build/dependency paths and removes those paths from the Git index for that commit. It does **not** delete the files from the working tree. Exact paths are added to that repository's local `.git/info/exclude`, preserving the shared `.gitignore` and excluding those generated paths from future `git add .` operations.

The hook requires Python 3.10 or newer. GitManager installs only into the repository's default `.git/hooks` directory, refuses custom `core.hooksPath` configurations, and will not overwrite an existing custom `pre-commit` hook. Disable Commit Clean in the same repository inspector to remove the GitManager-managed hook.


---

## 6. Windows production installer and signing

On Windows, `py build.py build` prepares the architecture-matched private CPython runtime, builds GitManager, its background service, the `gitm` CLI and the first-run companion. It produces the Tauri NSIS setup executable and WiX MSI, plus an additional comparison MSI from Master Packager Dev (MPDEV):

    target/release/bundle/nsis/GitManager-Setup.exe
    target/release/bundle/msi/*.msi
    target/windows-installers/*.msi

The Tauri WiX MSI and MPDEV MSI are separate packaging outputs so their install, repair and uninstall behavior can be compared. The Microsoft Store MSIX is not part of this ordinary desktop build; use `py build.py --store=msix` for the dedicated Store package stream.

The build also creates a separate first-run helper alongside the native binaries:

    target/release/GitManager-Setup.exe

That helper is bundled into the installed app as a Tauri sidecar. It is a tiny, dependency-free Rust launcher; it does not replace or duplicate Tauri. On first launch, the main Tauri app hands its executable path to the helper. The helper relaunches that same Tauri app with `--setup`, so the actual workspace wizard is still rendered by GitManager's Tauri UI/runtime. The helper is built for every supported OS: `GitManager-Setup.exe` on Windows and `GitManager-Setup` on Linux/macOS. The same first-run flow asks for the main repository folder, supports additional scan roots, and then continues into GitManager. If the sidecar is unavailable in a development build, the in-process wizard remains as a fallback.

The Windows installer copies the private CPython runtime after application files have been extracted. It lives under the current user's GitManager data directory and is used only by the bundled Developer Cleaner. It does not install Python machine-wide or modify PATH.

### Sign the production installer from GitManager

1. Import a valid Authenticode **Code Signing** certificate with its private key into the current Windows user's `Cert:\CurrentUser\My` certificate store.
2. Install the Windows SDK SignTool component.
3. Open **Settings → Production signing**, refresh certificates, select the certificate and choose `GitManager-Setup.exe`.
4. Click **Sign installer**. GitManager uses SHA-256, a trusted timestamp and then verifies the signature.

GitManager does not export or store the certificate's private key. A self-signed certificate creates a cryptographic signature but does not establish public trust on other computers. For public downloads, use a certificate chaining to a Windows-trusted code-signing authority; SmartScreen reputation can still take time to build. GitManager itself remains free; code-signing certificates are a separate publisher cost.

For tagged Windows CI releases, configure repository secrets `WINDOWS_SIGNING_CERTIFICATE` (base64 PFX) and `WINDOWS_SIGNING_PASSWORD`, plus the repository variable `WINDOWS_SIGNING_THUMBPRINT`. The workflow imports the certificate only on its ephemeral runner, signs the application, service and setup package, verifies the signature, then removes the temporary PFX and certificate. Never commit PFX files, passwords or private keys.


## 7. Per-user application storage and installation

GitManager keeps the application install directory separate from user configuration and data.

On Windows, the NSIS setup installer is current-user only and installs the application under:

    %LOCALAPPDATA%\kastrick's_software\GitManager\

Configuration and data live beside the application under the same Kastrick software root:

    %LOCALAPPDATA%\kastrick's_software\config\GitManager\
    %LOCALAPPDATA%\kastrick's_software\data\GitManager\

On Linux and macOS, the same folder layout is rooted in the platform's user-local data directory. GitManager migrates the existing preferences file from its previous Tauri config directory on first launch and leaves the source copy untouched.

The setup finish screen offers **Continue using GitManager** (launch the app) and a separate desktop-shortcut option. Uninstalling the application does not target the config/data siblings.

Release builds use size-focused Cargo settings (size optimization, thin LTO, eight codegen units and symbol stripping) to balance binary size and parallel build time. The build script reports separate target-specific dependency graphs for `git-manager`, `gitm-cli` and `gitmanager-setup`, plus the deduplicated union. It counts normal and build dependencies for each requested target, not every workspace member. The under-500 target applies to the desktop app graph, not the whole workspace. Cargo's `541/542`-style progress indicator is a build-progress total, not a count of distinct dependency package names; use the `GitManager app graph` report when comparing against the target. The Tauri signing overlay is created only when a valid Windows signing thumbprint is configured; otherwise local and CI builds remain intentionally unsigned without invoking SignTool.

GitManager's desktop application does **not** embed the standalone RBE runtime. Its archive engine links only the Cloud Node storage/provider APIs needed for local and remote archive operations. A separate NAS/remote-machine bootstrap ZIP provisions the full RBE runtime when installed on that machine.

## 7. Portable RBE node deployment

Every normal `py build.py` / `python build.py build` run also creates a small bootstrap ZIP under `target/deployment/`:

```text
GitManager-Cloud-Node-Setup.zip
```

To build only this package without a Rust toolchain or desktop installer, run:

```powershell
py build.py prepare-cloud-node
```

The ZIP contains `setup.py`, `manifest.json`, starter `settings.json` and `setting.node.cn.json`, the RBE `api/` route files, and the Apache-2.0 license/notice files. **It deliberately contains no RBE executables.**

On the destination machine, extract the ZIP and run `py setup.py` on Windows or `python3 setup.py` on Linux/macOS. Python 3.9+ and network access are required there. The script detects the host OS/architecture, queries the official RBE latest stable release, downloads only the matching platform archive, verifies the GitHub Release SHA-256 digest and the corresponding `SHA256SUMS` entry, and safely installs the complete runtime:

- `backend(.exe)` — RBE API backend
- `cloud_node(.exe)` — Cloud Node storage and peer/provider client
- `dep/container(.exe)` and `dep/service(.exe)` — RBE runtime dependencies
- `build-info.json` — release/build provenance
- `api/` — the included `.route` files copied beside the backend, including `/api/health` and `/api/status`

The script preserves existing configuration, keys, storage and API routes; it does not delete user data or overwrite an existing settings file. It creates a protected Cloud Node private key and separate `node-registration.json` with the public node identity. The generated start script sets the key/config environment variables and starts `backend` with the selected `settings.json`. Before it offers background-service registration, setup can initialize or confirm RBE's administrator account using RBE's hidden terminal prompt; GitManager never reads or stores that password. Unattended installs can use RBE's `RBE_ADMIN_PASSWORD` or `RBE_ADMIN_PASSWORD_FILE` environment input. With `--skip-admin-init`, or without a terminal and without either environment input, it leaves the service stopped until the operator runs `backend --settings settings.json --init-admin`. After validating the installed Cloud Node config and completing admin initialization, setup offers to register the RBE backend as a per-user Windows Task Scheduler task, Linux systemd unit, or macOS LaunchAgent. Use `--no-service` to skip that step and launch it manually. The launcher writes backend output to `logs/backend.log`; Linux headless systems may need user lingering enabled for startup before login.

The starter backend API binds to `0.0.0.0:8080` by default. After startup, the native `/health` endpoint and bundled `/api/health` and `/api/status` routes provide basic status information. In **Settings → Cloud → Cloud Node setup**, GitManager can accept a private LAN IP/port, save it in application preferences, and check all three endpoints. This is a reachability/status check, not peer pairing or authentication. The check only accepts private, loopback or link-local IP addresses and does not follow redirects. The installer does not open firewall ports or configure TLS; restrict the service with the host firewall and configure HTTPS before exposing it beyond a trusted LAN.

**Current peer-server limitation:** the current public RBE release wires the authenticated Cloud Node receiver into the backend's temporary maintenance/boot-recovery listener. It does not yet provide a permanent LAN peer listener with an interactive pairing endpoint. The RBE peer URL validator also requires HTTPS for non-loopback hosts. Installing the complete backend/runtime therefore provides the normal API/status service and the required runtime layout, but a persistent GitManager-to-NAS pairing/sync session still needs a permanent receiver and pairing flow.

GitManager's repository archive engine now writes its own content-addressed blobs under `gitm.archives/` and snapshot manifests/indexes under `gitm.metadata/`. `gitm.recovery/` is reserved for future recovery metadata. Existing format-version-1 `gitmanager/` index and blob paths remain readable while new captures move to format version 2.

Automatic archive watching is shared by the desktop app and the installed GitManager executable running with `--background-service`, through a per-user OS file lease, so only one watcher handles repository events at a time. If the desktop exits, the background worker takes over on its next preference refresh. The native installers already contain the executable used by the OS service; no unbundled BGW binary is required. The worker also retries configured cloud archive synchronization every five minutes, using the vendored Cloud Node provider-sync API in-process for Supabase/S3 uploads; it does not need to spawn a second `cloud_node.exe` for that. A permanent authenticated LAN peer listener/pairing flow is still required before GitManager can sync directly to the NAS by IP.

For CI, the lightweight ZIP is built once by the `cloud-node-deployment` job and uploaded as the `gitmanager-cloud-node-deployment` artifact. The package is not duplicated inside Windows, Linux or macOS GitManager installers. See [the deployment guide](tools/cloud-node-deployment/README.md), [RBE Cloud Node documentation](https://github.com/Kastrick8/RBE/blob/main/doc/cloud-node.md), [RBE API route documentation](https://github.com/Kastrick8/RBE/blob/main/api/README.md), and [the official RBE binary release](https://github.com/Kastrick8/RBE/releases/tag/rbe-bin-v0.1.0).

## 8. Repository Archives & Native `gitm` CLI

GitManager vendors the RBE Cloud Node storage engine under
`src-tauri/vendor/rbe` and adapts it for complete repository restore points.
Archive data is stored as content-addressed file blobs plus immutable manifests,
so unchanged content is shared across snapshots. Local and Remote Archives are
separate; each retains up to five complete restore points per repository/type.
Secret-bearing `.env`/credential/private-key paths are excluded by default. Remote Archives are refused when those paths exist in reachable Git history. Archive identity follows the sanitized Git remote where available, so cloud restore points survive checkout-path changes.

The standalone Rust `gitm` CLI provides local repository scanning,
inspection, status, commit history, branches, remotes, origin update checks,
fast-forward-only pulls, backup, restore, Cloud Node synchronization, and the
preview-first Developer Cleaner, plus an adaptive keyboard/mouse TUI without
requiring Node.js or the desktop WebView. Cloud Node providers can be configured
from **Scan Setup → Cloud object storage** or with
`gitm cloud configure --file provider.json`. The desktop archive panel displays
provider status and offers an explicit Sync Now action. Provider secrets stay
in OS environment variables. Google Drive/OneDrive desktop-sync folders can
also be selected in Scan Setup as local archive destinations.

### Native `gitm` command reference

Run `gitm --help` for the top-level list or `gitm help COMMAND` for focused help.

| Command | Purpose |
| --- | --- |
| `gitm` / `gitm tui` | Open the adaptive terminal dashboard. |
| `gitm scan [ROOT...]` | Find local Git repositories under selected roots. |
| `gitm inspect [PATH]` | Show repository metadata, refs, remotes, and worktree state. |
| `gitm status [PATH]` | Show changed and untracked files. |
| `gitm log [PATH] [--limit N]` | Read recent commits. |
| `gitm branches [PATH]` / `gitm remotes [PATH]` | Inspect branches and credential-redacted remote URLs. |
| `gitm update check [PATH]` | Compare the current branch with `origin`. |
| `gitm update pull [PATH]` | Fast-forward a clean branch only; refuses dirty or diverged worktrees. |
| `gitm backup [PATH] --kind local` | Save a Local Archive. |
| `gitm backup [PATH] --kind remote` | Save a Remote Archive including reachable Git history. |
| `gitm backups [PATH]` | List retained restore points. |
| `gitm restore [PATH] --id ID --kind KIND --yes` | Restore a selected point after verification. |
| `gitm cloud status|sync|configure|disable` | Inspect or explicitly synchronize Cloud Node storage. |
| `gitm cleaner preview|apply|commit-clean|hook` | Use the same preview-first developer cleaner as the desktop. |

See [docs/archive-storage.md](docs/archive-storage.md).

## Next.js Engine frontend

GitManager keeps its native Rust/Tauri backend. Its frontend is compiled as a static Next.js export using the Generation 3 source from [Kastrick8/nextjs-engine](https://github.com/Kastrick8/nextjs-engine/tree/main-3).

The build does **not** install the published npm package. Before a frontend build, `tools/nextjs_engine_source.py` queries the Engine repository's `main-3` branch and selects the newest reachable commit whose Engine runtime CI, cross-browser CI, CodeQL, and code-quality checks have all completed successfully. That source archive is downloaded into `.cache/nextjs-engine/`, then built with the Engine compiler and Next.js. The resolved SHA and required CI evidence are written to `workspace/out/engine-source.json` for build provenance; no Engine commit is hard-coded in GitManager.

A build requires Node.js 20.9 or newer (Node.js 22 is used in CI), npm, Python, Rust, and the Tauri CLI. Node.js and npm are build-time tools only; they are not bundled into the installed GitManager app. An internet connection and a reachable green Engine CI candidate are required. If the resolver cannot verify a green source commit, the frontend build stops rather than using unverified source.

Use `python build.py frontend-build` to build the static frontend alone, or `python build.py frontend-dev` to run the Next.js development server. The normal `python build.py build` and `python build.py dev` workflows invoke the same steps automatically through Tauri's hooks.

GitManager now uses a React App Router interface styled around GitHub's dark workspace language. The application shell, repository dashboard and detail view, Accounts, Settings, archive controls, cloud/NAS controls, first-run setup and Developer Cleaner are React components. Layout primitives such as `EngineBox` and `EngineGrid` render under the Generation 3 `EngineProvider`; the Engine source remains the authority for its runtime and component implementations.

The Engine's own `app/` routes are development and integration fixtures. They remain untouched in `.cache/nextjs-engine/source/app/`. GitManager overlays its tracked `frontend/app/` pages only into the separate `.cache/nextjs-engine/workspace/app/` build copy. The workspace keeps the rest of the Engine repository root—including `src/`, `scripts/`, `tests/`, `examples/`, `docs/`, root configuration and the original package manifest—so Engine tooling and development tests are not lost.

The React UI calls the existing Tauri commands directly; the Rust backend, background service, Git scanner, archive engine, provider integrations and cleaner remain native. The previous `ui/` files are retained as legacy references during the migration but are not the frontend bundled by Tauri.

