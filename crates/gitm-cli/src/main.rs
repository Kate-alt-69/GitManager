use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::queue;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{
    self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use gitmanager_archive::{
    capture_repository, default_archive_root, list_repository_archives,
    restore_repository_archive, ArchiveKind, ArchiveSnapshot,
};

const MENU: [&str; 5] = [
    "Create Local Archive",
    "Create Remote Archive",
    "Browse Restore Points",
    "Show Archive Location",
    "Exit",
];

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
    let snapshot = capture_repository(&repo, None, kind)?;
    print_snapshot(&snapshot);
    Ok(())
}

fn command_list(args: &[String]) -> Result<(), String> {
    let repo = option_value(args, "--repo")
        .map(str::to_owned)
        .or_else(|| positional(args, 0).map(str::to_owned))
        .unwrap_or(env::current_dir().map_err(|error| error.to_string())?.to_string_lossy().to_string());
    let kind = option_value(args, "--kind");
    let snapshots = list_repository_archives(&repo, None, kind)?;
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
    let result = restore_repository_archive(&repo, destination, None, kind, snapshot_id, true)?;
    println!("Restored {} archive to {}", result.snapshot.id, result.destination);
    if let Some(path) = result.preserved_previous_path {
        println!("Previous destination preserved at {}", path);
    }
    Ok(())
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
        "GitManager · gitm — native repository archive and recovery\n\
         Usage:\n\
           gitm                         Open the interactive TUI\n\
           gitm backup [PATH] --kind local|remote\n\
           gitm backups [PATH] [--kind local|remote]\n\
           gitm restore [PATH] --id ID [--kind local|remote] [--to PATH] --yes\n\
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

    loop {
        draw(&mut terminal, &repository, screen, selected, &snapshots, &status, pending_restore.as_ref())?;
        if !event::poll(Duration::from_millis(250)).map_err(|error| error.to_string())? {
            continue;
        }
        let Event::Key(key) = event::read().map_err(|error| error.to_string())? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match screen {
            Screen::Menu => match key.code {
                KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('W') => {
                    selected = (selected + MENU.len() - 1) % MENU.len();
                }
                KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') => {
                    selected = (selected + 1) % MENU.len();
                }
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Enter => match selected {
                    0 | 1 => {
                        let kind = if selected == 0 { "local" } else { "remote" };
                        status = format!("Capturing {kind} archive…");
                        draw(&mut terminal, &repository, screen, selected, &snapshots, &status, None)?;
                        match capture_repository(&repository.to_string_lossy(), None, kind) {
                            Ok(snapshot) => status = format!(
                                "Saved {} · {} files · {}",
                                snapshot.id, snapshot.file_count, human_bytes(snapshot.archive_bytes)
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
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Screen {
    Menu,
    Snapshots,
    ConfirmRestore,
}

struct TerminalSession;

impl TerminalSession {
    fn enter() -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|error| format!("Could not enable terminal input: {error}"))?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(format!("Could not enter TUI screen: {error}"));
        }
        Ok(Self)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), crossterm::cursor::Show, LeaveAlternateScreen);
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
    let (width, _) = terminal::size().unwrap_or((80, 24));
    let width = usize::from(width.clamp(48, 104));
    let unicode = env::var("GITM_ASCII").ok().as_deref() != Some("1")
        && env::var("TERM").ok().as_deref() != Some("dumb");
    let color = env::var_os("NO_COLOR").is_none();
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
            for (index, label) in MENU.iter().enumerate() {
                rows.push(format!("{} {}", if index == selected { if unicode { "❯" } else { ">" } } else { " " }, label));
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
                        human_bytes(snapshot.archive_bytes)
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
    rows.push("Keys: ↑/↓ or W/S · Enter · Esc/Q".to_string());

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
    if value.chars().count() <= max { return value.to_string(); }
    if max < 4 { return value.chars().take(max).collect(); }
    value.chars().take(max - 3).collect::<String>() + "..."
}

fn print_snapshot(snapshot: &ArchiveSnapshot) {
    println!("Saved {} archive {}", snapshot.kind.as_str(), snapshot.id);
    println!("Repository: {}", snapshot.repository_path);
    println!("Branch: {}", snapshot.branch);
    println!("Files: {} · source: {} · compressed: {}",
        snapshot.file_count, human_bytes(snapshot.source_bytes), human_bytes(snapshot.archive_bytes));
    println!("SHA-256: {}", snapshot.sha256);
    println!("Location: {}", snapshot.manifest_logical_path);
    if !snapshot.excluded_paths.is_empty() {
        println!("Excluded secret/special paths: {}", snapshot.excluded_paths.len());
        for path in snapshot.excluded_paths.iter().take(12) {
            println!("  - {path}");
        }
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
