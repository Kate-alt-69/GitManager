use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use crossterm::execute;
use crossterm::queue;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{
    self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use git2::{BranchType, Repository, Sort, StatusOptions};
use gitmanager_archive::{
    archive_cloud_storage_info, capture_repository, configure_archive_cloud_storage,
    default_archive_root, disable_archive_cloud_storage, list_repository_archives,
    restore_repository_archive, sync_archive_cloud_storage, ArchiveSnapshot,
};

const MENU: [&str; 6] = [
    "Create Local Archive",
    "Create Remote Archive",
    "Browse Restore Points",
    "Show Archive Location",
    "Cloud Storage: Status & Sync",
    "Exit",
];

fn terminal_width() -> usize {
    let (width, _) = terminal::size().unwrap_or((80, 24));
    usize::from(width.clamp(48, 104))
}

fn two_column_menu(width: usize) -> bool {
    width >= 88
}

fn move_menu_vertical(selected: usize, direction: isize, width: usize) -> usize {
    if two_column_menu(width) {
        let column = selected / 3;
        let row = selected % 3;
        column * 3 + (row as isize + direction).rem_euclid(3) as usize
    } else {
        (selected as isize + direction).rem_euclid(MENU.len() as isize) as usize
    }
}

fn move_menu_horizontal(selected: usize, right: bool, width: usize) -> usize {
    if !two_column_menu(width) {
        return selected;
    }
    let row = selected % 3;
    let column = selected / 3;
    (if right { 1 - column } else { 0 }) * 3 + row
}

fn main() {
    if let Err(error) = run() {
        eprintln!("gitm: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        let cwd = env::current_dir().map_err(|error| error.to_string())?;
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            return run_tui(cwd);
        }
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "--help" | "-h" | "help" => {
            print_help();
            Ok(())
        }
        "--version" | "-V" | "version" => {
            println!("gitm {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "backup" | "snapshot" => command_backup(&args[1..]),
        "backups" | "list" | "history" => command_list(&args[1..]),
        "restore" => command_restore(&args[1..]),
        "status" => command_status(&args[1..]),
        "log" | "history-commits" => command_log(&args[1..]),
        "branch" | "branches" => command_branches(&args[1..]),
        "remote" | "remotes" => command_remotes(&args[1..]),
        "cloud" => command_cloud(&args[1..]),
        "tui" => {
            let path = option_value(&args[1..], "--repo")
                .map(PathBuf::from)
                .unwrap_or(env::current_dir().map_err(|error| error.to_string())?);
            run_tui(path)
        }
        other => Err(format!("Unknown command '{other}'. Run 'gitm --help'.")),
    }
}

fn command_backup(args: &[String]) -> Result<(), String> {
    let repo = option_value(args, "--repo")
        .map(str::to_owned)
        .or_else(|| positional(args, 0).map(str::to_owned))
        .unwrap_or(env::current_dir().map_err(|error| error.to_string())?.to_string_lossy().to_string());
    let kind = option_value(args, "--kind").unwrap_or("local");
    let archive_root = option_value(args, "--archive-root");
    let snapshot = capture_repository(&repo, archive_root, kind)?;
    print_snapshot(&snapshot);
    Ok(())
}

fn command_list(args: &[String]) -> Result<(), String> {
    let repo = option_value(args, "--repo")
        .map(str::to_owned)
        .or_else(|| positional(args, 0).map(str::to_owned))
        .unwrap_or(env::current_dir().map_err(|error| error.to_string())?.to_string_lossy().to_string());
    let kind = option_value(args, "--kind");
    let archive_root = option_value(args, "--archive-root");
    let snapshots = list_repository_archives(&repo, archive_root, kind)?;
    if snapshots.is_empty() {
        println!("No GitManager restore points found for {repo}");
        return Ok(());
    }
    println!("GitManager restore points · {repo}");
    for snapshot in snapshots {
        println!(
            "{}  {:<6}  {}  {} files  {}",
            snapshot.id,
            snapshot.kind.as_str(),
            snapshot.branch,
            snapshot.file_count,
            human_bytes(snapshot.stored_bytes)
        );
    }
    Ok(())
}

fn command_restore(args: &[String]) -> Result<(), String> {
    let repo = option_value(args, "--repo")
        .map(str::to_owned)
        .or_else(|| positional(args, 0).map(str::to_owned))
        .unwrap_or(env::current_dir().map_err(|error| error.to_string())?.to_string_lossy().to_string());
    let snapshot_id = option_value(args, "--id")
        .or_else(|| positional(args, 1))
        .ok_or_else(|| "restore requires --id <snapshot-id>.".to_string())?;
    let kind = option_value(args, "--kind").unwrap_or("local");
    let destination = option_value(args, "--to").unwrap_or(&repo);
    let replace = args.iter().any(|value| value == "--yes" || value == "-y");
    if !replace {
        println!("Restore will be previewed and the destination will not be overwritten.");
        println!("Review the restore point with: gitm backups --repo \"{repo}\" --kind {kind}");
        println!("Run again with --yes to confirm replacement of the destination.");
        return Ok(());
    }
    let archive_root = option_value(args, "--archive-root");
    let result = restore_repository_archive(&repo, destination, archive_root, kind, snapshot_id, true)?;
    println!("Restored {} archive to {}", result.snapshot.id, result.destination);
    if let Some(path) = result.preserved_previous_path {
        println!("Previous destination preserved at {}", path);
    }
    Ok(())
}

fn repository_path_arg(args: &[String]) -> Result<PathBuf, String> {
    let value = option_value(args, "--repo")
        .or_else(|| positional(args, 0))
        .map(PathBuf::from)
        .unwrap_or(env::current_dir().map_err(|error| error.to_string())?);
    Ok(value)
}

fn open_repository(args: &[String]) -> Result<(PathBuf, Repository), String> {
    let path = repository_path_arg(args)?;
    let canonical = path.canonicalize()
        .map_err(|error| format!("Could not resolve repository path {}: {error}", path.display()))?;
    let repository = Repository::open(&canonical)
        .map_err(|error| format!("{} is not a readable Git repository: {error}", canonical.display()))?;
    Ok((canonical, repository))
}

fn command_status(args: &[String]) -> Result<(), String> {
    let (path, repository) = open_repository(args)?;
    let branch = repository.head()
        .ok()
        .and_then(|head| head.shorthand().map(str::to_owned))
        .unwrap_or_else(|| "(detached or unborn HEAD)".to_string());
    let head = repository.head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string())
        .unwrap_or_else(|| "(no commit yet)".to_string());

    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = repository.statuses(Some(&mut options))
        .map_err(|error| format!("Could not read worktree status: {error}"))?;

    println!("Repository: {}", path.display());
    println!("Branch:     {branch}");
    println!("HEAD:       {}", &head[..head.len().min(12)]);
    println!("Worktree:   {} changed path(s)", statuses.len());
    if statuses.is_empty() {
        println!("Working tree clean.");
    } else {
        for entry in statuses.iter() {
            let state = format!("{:?}", entry.status());
            let file = entry.path().unwrap_or("<non-UTF-8 path>");
            println!("{state:<28} {file}");
        }
    }
    Ok(())
}

fn command_log(args: &[String]) -> Result<(), String> {
    let (path, repository) = open_repository(args)?;
    let limit = option_value(args, "--limit")
        .map(|value| value.parse::<usize>().map_err(|_| "--limit must be an integer from 1 to 500.".to_string()))
        .transpose()?
        .unwrap_or(20);
    if !(1..=500).contains(&limit) {
        return Err("--limit must be between 1 and 500.".to_string());
    }
    let mut walk = repository.revwalk()
        .map_err(|error| format!("Could not create commit history walk: {error}"))?;
    walk.push_head().map_err(|error| format!("Repository has no readable HEAD: {error}"))?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .map_err(|error| format!("Could not sort commit history: {error}"))?;
    println!("Recent commits · {} · {}", path.display(), limit);
    let mut count = 0usize;
    for oid in walk.take(limit) {
        let oid = oid.map_err(|error| format!("Could not read commit reference: {error}"))?;
        let commit = repository.find_commit(oid)
            .map_err(|error| format!("Could not read commit {oid}: {error}"))?;
        let author = commit.author();
        let name = author.name().unwrap_or("Unknown author");
        let email = author.email().unwrap_or("no email");
        let summary = commit.summary().unwrap_or("(no commit message)");
        let timestamp = commit.time().seconds();
        println!("{}  {} <{}>  [{}]", &oid.to_string()[..12], name, email, timestamp);
        println!("    {}", summary.lines().next().unwrap_or(""));
        count += 1;
    }
    if count == 0 {
        println!("No commits yet.");
    }
    Ok(())
}

fn command_branches(args: &[String]) -> Result<(), String> {
    let (path, repository) = open_repository(args)?;
    let mut branches = repository.branches(None)
        .map_err(|error| format!("Could not list repository branches: {error}"))?
        .map(|entry| {
            let (branch, kind) = entry.map_err(|error| error.to_string())?;
            let name = branch.name()
                .map_err(|error| error.to_string())?
                .unwrap_or("<non-UTF-8 branch>")
                .to_string();
            let current = branch.is_head();
            Ok::<_, String>((name, kind, current))
        })
        .collect::<Result<Vec<_>, _>>()?;
    branches.sort_by(|left, right| left.0.cmp(&right.0));
    println!("Branches · {}", path.display());
    if branches.is_empty() {
        println!("No branches found.");
    }
    for (name, kind, current) in branches {
        let kind = match kind {
            BranchType::Local => "local",
            BranchType::Remote => "remote",
        };
        println!("{} {:<7} {}", if current { "*" } else { " " }, kind, name);
    }
    Ok(())
}

fn safe_remote_display(value: &str) -> String {
    if let Ok(mut url) = url::Url::parse(value) {
        if url.scheme() != "ssh" {
            let _ = url.set_username("");
        }
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        return url.to_string();
    }
    if !value.contains("://") {
        if let Some((user, destination)) = value.split_once('@') {
            if destination.contains(':') && !destination.starts_with('/') {
                let user = if user.is_empty() || user.contains(':') { "git" } else { user };
                return format!("{user}@{destination}");
            }
        }
    }
    if value.contains("://") {
        "<remote URL omitted: unable to safely redact credentials>".to_string()
    } else {
        value.to_string()
    }
}

fn command_remotes(args: &[String]) -> Result<(), String> {
    let (path, repository) = open_repository(args)?;
    let remotes = repository.remotes()
        .map_err(|error| format!("Could not list repository remotes: {error}"))?;
    println!("Remotes · {}", path.display());
    if remotes.is_empty() {
        println!("No remotes configured.");
    }
    for name in remotes.iter().flatten() {
        let remote = repository.find_remote(name)
            .map_err(|error| format!("Could not inspect remote {name}: {error}"))?;
        println!("{name}");
        if let Some(url) = remote.url() {
            println!("  fetch: {}", safe_remote_display(url));
        }
        if let Some(url) = remote.pushurl() {
            println!("  push:  {}", safe_remote_display(url));
        }
    }
    Ok(())
}

fn command_cloud(args: &[String]) -> Result<(), String> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    let archive_root = option_value(args, "--archive-root");
    match action {
        "status" => {
            let root = archive_root.map(PathBuf::from).unwrap_or(default_archive_root()?);
            let info = archive_cloud_storage_info(&root)?;
            print_cloud_info(&info);
            Ok(())
        },
        "sync" => {
            let result = sync_archive_cloud_storage(archive_root)?;
            println!("Cloud archive synchronization: {result}");
            Ok(())
        }
        "configure" | "config" => {
            let config_path = option_value(args, "--file")
                .or_else(|| positional(args, 1))
                .ok_or_else(|| "cloud configure requires --file <provider.json>.".to_string())?;
            let contents = std::fs::read_to_string(config_path)
                .map_err(|error| format!("Could not read provider config {config_path}: {error}"))?;
            let info = configure_archive_cloud_storage(archive_root, &contents)?;
            print_cloud_info(&info);
            println!("Configuration saved. Run gitm cloud sync to test/publish the archive store.");
            Ok(())
        }
        "disable" => {
            let info = disable_archive_cloud_storage(archive_root)?;
            print_cloud_info(&info);
            Ok(())
        }
        "--help" | "-h" | "help" => {
            print_cloud_help();
            Ok(())
        }
        other => Err(format!("Unknown cloud action '{other}'. Run 'gitm cloud --help'.")),
    }
}

fn cloud_status_summary(root: &Path) -> Result<String, String> {
    let info = archive_cloud_storage_info(root)?;
    if !info.configured {
        return Ok(format!("Cloud storage not configured · {}", info.note));
    }
    let provider = info.provider.as_deref().unwrap_or("unknown");
    let sync = sync_archive_cloud_storage(Some(&root.to_string_lossy()))?;
    Ok(format!("{provider} cloud storage · {sync}"))
}

fn print_cloud_info(info: &gitmanager_archive::ArchiveCloudStorageInfo) {
    if !info.configured {
        println!("Cloud archive storage: not configured");
        println!("{}", info.note);
        return;
    }
    println!("Cloud archive storage: configured");
    println!("Provider: {}", info.provider.as_deref().unwrap_or("unknown"));
    println!("Target: {}", info.target.as_deref().unwrap_or("unknown"));
    println!("Namespace: {}", info.namespace.as_deref().unwrap_or("unknown"));
    println!("Bucket: {}", info.bucket.as_deref().unwrap_or("unknown"));
    if info.auth_environment_variables.is_empty() {
        println!("Authentication: no environment variables required");
    } else {
        println!("Required credential environment variables:");
        for name in &info.auth_environment_variables {
            println!("  {name}");
        }
    }
    println!("{}", info.note);
}

fn print_cloud_help() {
    println!(
        "gitm cloud — configure and synchronize archive object storage\n\
         Usage:\n\
           gitm cloud status [--archive-root PATH]\n\
           gitm cloud sync [--archive-root PATH]\n\
           gitm cloud configure --file provider.json [--archive-root PATH]\n\
           gitm cloud disable [--archive-root PATH]\n\
         \n\
         Provider configuration supports Cloud Node providers (S3, Supabase,\n\
         Azure Blob, Google Cloud Storage, and generic HTTPS). The JSON stores\n\
         environment-variable names only; put secret values in your OS environment.\n\
         Google Cloud Storage is not Google Drive. A Drive Desktop-synced folder\n\
         can be selected as the local archive path.\n"
    );
}

fn option_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.iter().position(|value| value == key)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
        .or_else(|| args.iter().find_map(|value| value.strip_prefix(&format!("{key}="))))
}

fn positional(args: &[String], index: usize) -> Option<&str> {
    let mut found = 0usize;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let value = &args[cursor];
        if value.starts_with('-') {
            if !value.contains('=') && cursor + 1 < args.len()
                && !args[cursor + 1].starts_with('-')
            {
                cursor += 2;
            } else {
                cursor += 1;
            }
            continue;
        }
        if found == index {
            return Some(value.as_str());
        }
        found += 1;
        cursor += 1;
    }
    None
}

fn print_help() {
    println!(
        "GitManager · gitm — native repository and archive toolkit\n\
         Usage:\n\
           gitm                         Open the interactive TUI\n\
           gitm backup [PATH] --kind local|remote [--archive-root PATH]\n\
           gitm backups [PATH] [--kind local|remote] [--archive-root PATH]\n\
           gitm restore [PATH] --id ID [--kind local|remote] [--to PATH] [--archive-root PATH] --yes\n\
           gitm status [PATH]\n\
           gitm log [PATH] [--limit N]\n\
           gitm branches [PATH]\n\
           gitm remotes [PATH]\n\
           gitm cloud status|sync|configure|disable [options]\n\
           gitm tui [--repo PATH]\n\
           gitm --help | --version\n\
         \n\
         Local Archive stores working files without .git. Remote Archive stores\n\
         the Git repository metadata and working files. Secrets are excluded by\n\
         default. Restore requires --yes before replacing an existing destination.\n"
    );
}

fn run_tui(repository: PathBuf) -> Result<(), String> {
    let repository = repository.canonicalize()
        .map_err(|error| format!("Could not resolve repository directory: {error}"))?;
    let mut terminal = TerminalSession::enter()?;
    let mut screen = Screen::Menu;
    let mut selected = 0usize;
    let mut snapshots = Vec::<ArchiveSnapshot>::new();
    let mut status = "Ready · choose an archive action".to_string();
    let mut pending_restore: Option<ArchiveSnapshot> = None;

    draw(&mut terminal, &repository, screen, selected, &snapshots, &status, pending_restore.as_ref())?;
    loop {
        if !event::poll(Duration::from_millis(250)).map_err(|error| error.to_string())? {
            continue;
        }
        let incoming = event::read().map_err(|error| error.to_string())?;
        let key = match incoming {
            Event::Mouse(mouse) => {
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    let width = terminal_width();
                    let row = usize::from(mouse.row);
                    match screen {
                        Screen::Menu => {
                            if two_column_menu(width) && (7..10).contains(&row) {
                                let item_row = row - 7;
                                let right_column = if usize::from(mouse.column) >= width / 2 { 1 } else { 0 };
                                selected = item_row + right_column * 3;
                                status = format!("Selected {} · press Enter", MENU[selected]);
                            } else if !two_column_menu(width) && (7..13).contains(&row) {
                                selected = row - 7;
                                status = format!("Selected {} · press Enter", MENU[selected]);
                            }
                        }
                        Screen::Snapshots => {
                            let item_row = row.saturating_sub(7);
                            if row >= 7 && item_row < snapshots.len() {
                                selected = item_row;
                                status = format!("Selected restore point {} · press Enter or R", snapshots[selected].id);
                            }
                        }
                        Screen::ConfirmRestore => {}
                    }
                }
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    draw(&mut terminal, &repository, screen, selected, &snapshots, &status, pending_restore.as_ref())?;
                }
                continue;
            }
            Event::Key(key) => key,
            Event::Resize(_, _) => {
                draw(&mut terminal, &repository, screen, selected, &snapshots, &status, pending_restore.as_ref())?;
                continue;
            }
            _ => continue,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match screen {
            Screen::Menu => match key.code {
                KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('W') => {
                    selected = move_menu_vertical(selected, -1, terminal_width());
                }
                KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') => {
                    selected = move_menu_vertical(selected, 1, terminal_width());
                }
                KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('A') => {
                    selected = move_menu_horizontal(selected, false, terminal_width());
                }
                KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('D') => {
                    selected = move_menu_horizontal(selected, true, terminal_width());
                }
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break,
                KeyCode::Enter => match selected {
                    0 | 1 => {
                        let kind = if selected == 0 { "local" } else { "remote" };
                        status = format!("Capturing {kind} archive…");
                        draw(&mut terminal, &repository, screen, selected, &snapshots, &status, None)?;
                        match capture_repository(&repository.to_string_lossy(), None, kind) {
                            Ok(snapshot) => status = format!(
                                "Saved {} · {} files · {}",
                                snapshot.id, snapshot.file_count, human_bytes(snapshot.stored_bytes)
                            ),
                            Err(error) => status = format!("Capture failed: {error}"),
                        }
                    }
                    2 => {
                        snapshots = list_repository_archives(&repository.to_string_lossy(), None, None)?;
                        screen = Screen::Snapshots;
                        selected = 0;
                        status = if snapshots.is_empty() {
                            "No restore points yet. Create an archive first.".to_string()
                        } else {
                            format!("{} restore point(s) · Enter to inspect, R to restore", snapshots.len())
                        };
                    }
                    3 => status = format!("Archive store: {}", default_archive_root()?.display()),
                    4 => {
                        let root = default_archive_root()?;
                        status = match cloud_status_summary(&root) {
                            Ok(summary) => summary,
                            Err(error) => format!("Cloud sync failed: {error}"),
                        };
                    }
                    _ => break,
                },
                _ => {}
            },
            Screen::Snapshots => match key.code {
                KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('W') if !snapshots.is_empty() => {
                    selected = (selected + snapshots.len() - 1) % snapshots.len();
                }
                KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') if !snapshots.is_empty() => {
                    selected = (selected + 1) % snapshots.len();
                }
                KeyCode::Enter | KeyCode::Char('r') | KeyCode::Char('R') if !snapshots.is_empty() => {
                    pending_restore = Some(snapshots[selected].clone());
                    screen = Screen::ConfirmRestore;
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    screen = Screen::Menu;
                    selected = 2;
                    pending_restore = None;
                }
                _ => {}
            },
            Screen::ConfirmRestore => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(snapshot) = pending_restore.take() {
                        let kind = snapshot.kind.as_str();
                        status = format!("Restoring {}…", snapshot.id);
                        draw(&mut terminal, &repository, Screen::Menu, 2, &snapshots, &status, None)?;
                        match restore_repository_archive(
                            &repository.to_string_lossy(),
                            &repository.to_string_lossy(),
                            None,
                            kind,
                            &snapshot.id,
                            true,
                        ) {
                            Ok(result) => {
                                status = format!("Restored {} · previous copy: {}", result.snapshot.id,
                                    result.preserved_previous_path.unwrap_or_else(|| "not needed".to_string()));
                            }
                            Err(error) => status = format!("Restore failed: {error}"),
                        }
                    }
                    screen = Screen::Menu;
                    selected = 2;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    pending_restore = None;
                    screen = Screen::Snapshots;
                }
                _ => {}
            },
        }
        draw(&mut terminal, &repository, screen, selected, &snapshots, &status, pending_restore.as_ref())?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Screen {
    Menu,
    Snapshots,
    ConfirmRestore,
}

struct TerminalSession {
    mouse_enabled: bool,
}

impl TerminalSession {
    fn enter() -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|error| format!("Could not enable terminal input: {error}"))?;
        let mut stdout = io::stdout();
        let mouse_enabled = env::var("TERM").ok().as_deref() != Some("dumb");
        if let Err(error) = execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(format!("Could not enter TUI screen: {error}"));
        }
        if mouse_enabled {
            if let Err(error) = execute!(stdout, EnableMouseCapture) {
                let _ = execute!(stdout, crossterm::cursor::Show, LeaveAlternateScreen);
                let _ = terminal::disable_raw_mode();
                return Err(format!("Could not enable TUI mouse selection: {error}"));
            }
        }
        Ok(Self { mouse_enabled })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        if self.mouse_enabled {
            let _ = execute!(stdout, DisableMouseCapture);
        }
        let _ = terminal::disable_raw_mode();
        let _ = execute!(stdout, crossterm::cursor::Show, LeaveAlternateScreen);
    }
}

fn draw(
    _terminal: &mut TerminalSession,
    repository: &Path,
    screen: Screen,
    selected: usize,
    snapshots: &[ArchiveSnapshot],
    status: &str,
    pending: Option<&ArchiveSnapshot>,
) -> Result<(), String> {
    let width = terminal_width();
    let unicode = env::var("GITM_ASCII").ok().as_deref() != Some("1")
        && env::var("TERM").ok().as_deref() != Some("dumb");
    let color = env::var_os("NO_COLOR").is_none()
        && env::var("FORCE_COLOR").ok().as_deref() != Some("0")
        && env::var("GITM_COLOR").ok().as_deref() != Some("0")
        && env::var("TERM").ok().as_deref() != Some("dumb");
    let left = if unicode { "╭" } else { "+" };
    let right = if unicode { "╮" } else { "+" };
    let bottom_left = if unicode { "╰" } else { "+" };
    let bottom_right = if unicode { "╯" } else { "+" };
    let horizontal = if unicode { "─" } else { "-" };
    let vertical = if unicode { "│" } else { "|" };
    let inner = width.saturating_sub(4);
    let mut rows = Vec::<String>::new();
    rows.push("GitManager · gitm".to_string());
    rows.push("Repository archive & recovery".to_string());
    rows.push(String::new());
    rows.push(format!("Repository: {}", repository.display()));
    rows.push(String::new());

    match screen {
        Screen::Menu => {
            rows.push("Choose an action".to_string());
            if two_column_menu(width) {
                let column_width = (inner.saturating_sub(2)) / 2;
                for row in 0..3 {
                    let left_index = row;
                    let right_index = row + 3;
                    let left = format!(
                        "{} [{}] {}",
                        if left_index == selected { if unicode { "❯" } else { ">" } } else { " " },
                        left_index + 1,
                        MENU[left_index],
                    );
                    let right = format!(
                        "{} [{}] {}",
                        if right_index == selected { if unicode { "❯" } else { ">" } } else { " " },
                        right_index + 1,
                        MENU[right_index],
                    );
                    rows.push(format!("{}  {}", pad_to_width(&clip(&left, column_width), column_width), clip(&right, column_width)));
                }
            } else {
                for (index, label) in MENU.iter().enumerate() {
                    rows.push(format!("{} [{}] {}", if index == selected { if unicode { "❯" } else { ">" } } else { " " }, index + 1, label));
                }
            }
        }
        Screen::Snapshots => {
            rows.push("Restore points · ↑/↓ move · Enter/R restore · Esc back".to_string());
            if snapshots.is_empty() {
                rows.push("No saved archives found.".to_string());
            } else {
                for (index, snapshot) in snapshots.iter().enumerate() {
                    rows.push(format!(
                        "{} {}  {:<6}  {}  {} files  {}",
                        if index == selected { if unicode { "❯" } else { ">" } } else { " " },
                        snapshot.id,
                        snapshot.kind.as_str(),
                        snapshot.branch,
                        snapshot.file_count,
                        human_bytes(snapshot.stored_bytes)
                    ));
                }
            }
        }
        Screen::ConfirmRestore => {
            rows.push("CONFIRM RESTORE".to_string());
            if let Some(snapshot) = pending {
                rows.push(format!("Archive: {} ({})", snapshot.id, snapshot.kind.as_str()));
                rows.push(format!("Destination: {}", repository.display()));
                rows.push("The current directory will be moved aside before restore.".to_string());
                rows.push(format!("Excluded secrets: {}", snapshot.excluded_paths.len()));
                rows.push(String::new());
                rows.push("Press Y to confirm replacement, N/Esc to cancel.".to_string());
            }
        }
    }
    rows.push(String::new());
    rows.push(format!("Status: {status}"));
    rows.push(if two_column_menu(width) {
        "Keys: ↑/↓ or W/S · ←/→ or A/D · Enter · Esc/Q · Click to select".to_string()
    } else {
        "Keys: ↑/↓ or W/S · Enter · Esc/Q · Click to select".to_string()
    });

    let mut stdout = io::stdout();
    queue!(stdout, crossterm::cursor::MoveTo(0, 0), Clear(ClearType::All))
        .map_err(|error| error.to_string())?;
    let top_title = " GitManager ";
    let top_fill = width.saturating_sub(top_title.len() + 2);
    let top = format!("{left}{horizontal}{top_title}{}{right}", horizontal.repeat(top_fill));
    if color {
        queue!(stdout, SetForegroundColor(Color::DarkGrey)).map_err(|error| error.to_string())?;
    }
    queue!(stdout, Print(top), Print("\r\n")).map_err(|error| error.to_string())?;
    if color {
        queue!(stdout, ResetColor).map_err(|error| error.to_string())?;
    }
    for row in rows {
        let clipped = clip(&row, inner);
        let padded = format!("{clipped:<inner$}");
        if color && (row.starts_with("Status:") || row == "CONFIRM RESTORE") {
            queue!(stdout, SetForegroundColor(Color::Cyan)).map_err(|error| error.to_string())?;
        } else if color && row.contains("❯") {
            queue!(stdout, SetForegroundColor(Color::Yellow)).map_err(|error| error.to_string())?;
        }
        queue!(stdout, Print(format!("{vertical} {padded} {vertical}\r\n")))
            .map_err(|error| error.to_string())?;
        if color {
            queue!(stdout, ResetColor).map_err(|error| error.to_string())?;
        }
    }
    let bottom = format!("{bottom_left}{}{bottom_right}", horizontal.repeat(width.saturating_sub(2)));
    if color {
        queue!(stdout, SetForegroundColor(Color::DarkGrey)).map_err(|error| error.to_string())?;
    }
    queue!(stdout, Print(bottom)).map_err(|error| error.to_string())?;
    if color {
        queue!(stdout, ResetColor).map_err(|error| error.to_string())?;
    }
    stdout.flush().map_err(|error| error.to_string())
}

fn clip(value: &str, max: usize) -> String {
    if UnicodeWidthStr::width(value) <= max {
        return value.to_string();
    }
    if max < 4 {
        let mut output = String::new();
        let mut width = 0usize;
        for character in value.chars() {
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            if width + character_width > max {
                break;
            }
            output.push(character);
            width += character_width;
        }
        return output;
    }
    let limit = max - 3;
    let mut output = String::new();
    let mut width = 0usize;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > limit {
            break;
        }
        output.push(character);
        width += character_width;
    }
    output + "..."
}

fn pad_to_width(value: &str, width: usize) -> String {
    format!("{value}{}", " ".repeat(width.saturating_sub(UnicodeWidthStr::width(value))))
}

fn print_snapshot(snapshot: &ArchiveSnapshot) {
    println!("Saved {} archive {}", snapshot.kind.as_str(), snapshot.id);
    println!("Repository: {}", snapshot.repository_path);
    println!("Branch: {}", snapshot.branch);
    println!("Files: {} · source: {} · new blob bytes: {}",
        snapshot.file_count, human_bytes(snapshot.source_bytes), human_bytes(snapshot.stored_bytes));
    println!("SHA-256: {}", snapshot.manifest_sha256);
    println!("Location: {}", snapshot.manifest_logical_path);
    if !snapshot.excluded_paths.is_empty() {
        println!("Excluded secret/special paths: {}", snapshot.excluded_paths.len());
        for path in snapshot.excluded_paths.iter().take(12) {
            println!("  - {path}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::positional;

    #[test]
    fn terminal_clip_uses_display_width_for_wide_unicode() {
        assert_eq!(super::clip("界A", 3), "界A");
        assert_eq!(super::clip("界ABC", 5), "界...");
        assert_eq!(super::pad_to_width("界A", 4), "界A ");
    }

    #[test]
    fn menu_moves_vertically_by_column_and_horizontally_by_row() {
        assert_eq!(super::move_menu_vertical(0, 1, 104), 1);
        assert_eq!(super::move_menu_vertical(1, -1, 104), 0);
        assert_eq!(super::move_menu_horizontal(1, true, 104), 4);
        assert_eq!(super::move_menu_horizontal(4, false, 104), 1);
    }

    #[test]
    fn remote_display_redacts_embedded_credentials_and_query_tokens() {
        assert_eq!(
            super::safe_remote_display("https://alice:secret@example.com/org/repo.git?token=hidden#frag"),
            "https://example.com/org/repo.git"
        );
        assert_eq!(
            super::safe_remote_display("alice@example.com:org/repo.git"),
            "alice@example.com:org/repo.git"
        );
        assert_eq!(
            super::safe_remote_display("alice:password@example.com:org/repo.git"),
            "git@example.com:org/repo.git"
        );
        assert_eq!(
            super::safe_remote_display("ssh://alice:password@example.com/org/repo.git"),
            "ssh://alice@example.com/org/repo.git"
        );
    }

    #[test]
    fn unknown_remote_schemes_do_not_echo_possible_credentials() {
        assert_eq!(
            super::safe_remote_display("custom://user:pass@example.com/path?token=hidden"),
            "custom://example.com/path"
        );
    }

    #[test]
    fn positional_arguments_skip_option_values() {
        let args = vec![
            "--kind".to_string(), "remote".to_string(),
            "--archive-root".to_string(), "D:/Backups".to_string(),
            "D:/Projects/App".to_string(),
        ];
        assert_eq!(positional(&args, 0), Some("D:/Projects/App"));
        assert_eq!(positional(&args, 1), None);
    }

    #[test]
    fn positional_arguments_keep_multiple_paths() {
        let args = vec!["source".to_string(), "--id".to_string(), "snapshot".to_string(), "destination".to_string()];
        assert_eq!(positional(&args, 0), Some("source"));
        assert_eq!(positional(&args, 1), Some("destination"));
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
