# GitManager — Pure-Rust + Tauri v2 Native Git Explorer & Background Update Service

**GitManager** is a native cross-platform desktop Git repository manager, interactive **Git Commit Tree** visualizer, and **Background Repository Update Service** built in **100% Rust** using the **Rust `tauri` v2 crate** (with `withGlobalTauri: true` serving `ui/` directly — **zero Node.js, npm, or Vite required**) for **Windows 10 & 11**, **Linux**, and **macOS**.

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
