# GitManager — Native Git Workspace

**GitManager** is a **free, open-source** cross-platform Rust desktop application for local Git repositories, remote provider accounts, commit history and background repository updates. It is published by **Kastrick** (Copyright © 2026) under the **Apache License 2.0**. No subscription, paid tier or account is required to use the app. It uses top-and-bottom tab navigation with no permanent sidebar. The Repositories page is the local-machine homepage; GitHub/GitLab account repositories live separately under Accounts. The current Tauri 2 shell is the portable host while the Windows-specific wl_ui hosting contract is prepared for its upcoming WinUI 3 / Win32 backends.

---

## 1. Smart Python Build & Dependency Script (`build.py`)

`build.py` checks your machine's dependencies before building or running:

- **Rust Toolchain Detection & Version Adaptation**:
  - Checks if `rustc` and `cargo` are installed in `PATH` or `~/.cargo/bin`.
  - Compares the installed version against the baseline (`1.77.2`). If a **different version is already installed** on your machine (newer, custom/nightly, or older), `build.py` **automatically uses the version you already have installed** (or picks the best already-installed `rustup` toolchain if multiple exist locally) instead of failing or forcing a toolchain re-download.
- **Pure-Rust `tauri-cli` (`cargo tauri`) Detection**:
  - Uses the Rust `cargo-tauri` crate binary if already installed, or builds/runs the Rust crate directly with `cargo build --release` / `cargo run` (pass `--install-tauri-cli` if you want `cargo install tauri-cli` run automatically).
- **OS Native Library Check**:
  - Checks WebView2/Win32 on Windows 10/11, `webkit2gtk-4.1` / `gtk+-3.0` / `openssl` via `pkg-config` on Linux, and Xcode Command Line Tools on macOS.

```bash
# Check installed Rust/Tauri/OS dependencies only
python3 build.py deps

# Run in development mode (uses your already-installed Rust toolchain)
python3 build.py dev

# Build optimized native release binaries (both `git-manager` GUI and `git-manager-service` daemon)
python3 build.py build

# Prepare the isolated CPython runtime used by Windows installers
python3 build.py prepare-runtime
```

---

## 2. Background Repository Update Service (`svchost` / OS Service + Loading Spinner)

Implemented in `src-tauri/src/watcher_service.rs` and `src-tauri/src/bin/git_manager_service.rs`:

- **OS Service Registration & Background Worker**:
  - **Windows 10 & 11**: Registers `GitManagerUpdateSvc` via `sc.exe` (Windows Service / `svchost` background service model) or spawns `git-manager-service.exe` (`--svchost-worker`) in the background.
  - **Linux**: Registers and starts `~/.config/systemd/user/gitmanager-updater.service` via `systemctl --user`.
  - **macOS**: Registers and loads `~/Library/LaunchAgents/com.gitmanager.updater.plist` via `launchctl`.
- **Automatic Update Notification Prompt**:
  - Periodically polls remote `origin` heads (`libgit2` fetch / `ls-remote`) across all indexed repositories.
  - Whenever a repository has new upstream commits, it triggers both an OS desktop notification and an in-app interactive banner asking:
    > **"Hay you have `<X-REPO_NAME>` update do you want to update it?"**
- **Clear Animated Loading Spinner**:
  - Displays a crisp rotating SVG loading icon (`#service-spinner-icon` and `.loading-spinner.active-spin`) while scanning remotes or when clicking **"Yes, Update Now"** to pull/fast-forward the repository.

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

## 7. Windows production installer and signing

On Windows, `python build.py build` prepares the isolated CPython runtime, builds GitManager and its background service, and creates:

    target/release/bundle/nsis/GitManager-Setup.exe

The setup installer copies the private CPython runtime after application files have been extracted. It lives under the current user's GitManager data directory and is used only by the bundled Developer Cleaner. It does not install Python machine-wide or modify PATH.

### Sign the production installer from GitManager

1. Import a valid Authenticode **Code Signing** certificate with its private key into the current Windows user's `Cert:\CurrentUser\My` certificate store.
2. Install the Windows SDK SignTool component.
3. Open **Settings → Production signing**, refresh certificates, select the certificate and choose `GitManager-Setup.exe`.
4. Click **Sign installer**. GitManager uses SHA-256, a trusted timestamp and then verifies the signature.

GitManager does not export or store the certificate's private key. A self-signed certificate creates a cryptographic signature but does not establish public trust on other computers. For public downloads, use a certificate chaining to a Windows-trusted code-signing authority; SmartScreen reputation can still take time to build. GitManager itself remains free; code-signing certificates are a separate publisher cost.

For tagged Windows CI releases, configure repository secrets `WINDOWS_SIGNING_CERTIFICATE` (base64 PFX) and `WINDOWS_SIGNING_PASSWORD`, plus the repository variable `WINDOWS_SIGNING_THUMBPRINT`. The workflow imports the certificate only on its ephemeral runner, signs the application, service and setup package, verifies the signature, then removes the temporary PFX and certificate. Never commit PFX files, passwords or private keys.


## 6. Per-user application storage and installation

GitManager keeps the application install directory separate from user configuration and data.

On Windows, the NSIS setup installer is current-user only and installs the application under:

    %LOCALAPPDATA%\kastrick's_software\GitManager\

Configuration and data live beside the application under the same Kastrick software root:

    %LOCALAPPDATA%\kastrick's_software\config\GitManager\
    %LOCALAPPDATA%\kastrick's_software\data\GitManager\

On Linux and macOS, the same folder layout is rooted in the platform's user-local data directory. GitManager migrates the existing preferences file from its previous Tauri config directory on first launch and leaves the source copy untouched.

The setup finish screen offers **Continue using GitManager** (launch the app) and a separate desktop-shortcut option. Uninstalling the application does not target the config/data siblings.

Release builds use size-focused Cargo settings (size optimization, LTO, one codegen unit and symbol stripping), and Tokio is built with only the runtime/time features used by this application. This is an optimization baseline; final installer sizes must still be measured from actual Windows/Linux/macOS release artifacts.


## 5. Repository Archives & Native `gitm` CLI

GitManager vendors the RBE Cloud Node storage engine under
`src-tauri/vendor/rbe` and adapts it for complete repository restore points.
Archive data is stored as content-addressed file blobs plus immutable manifests,
so unchanged content is shared across snapshots. Local and Remote Archives are
separate; each retains up to five complete restore points per repository/type.
Secret-bearing `.env`/credential/private-key paths are excluded by default.

The standalone Rust `gitm` CLI provides backup, history, restore, and an
arrow/WASD keyboard TUI without requiring Node.js or the desktop WebView.

See [docs/archive-storage.md](docs/archive-storage.md).
