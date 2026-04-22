mod color;
mod git;
mod graph;
mod remote;
mod ui;

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use std::collections::HashSet;
use std::io::{self, Write};
use std::time::Duration;

use git::{Branch, StatusEntry, TrackingInfo};
use graph::GraphRow;
use ui::{Focus, Layout};

/// Modal states that intercept key handling.
enum Mode {
    Normal,
    CommitMessage(String),
    NewBranch { name: String, target_sha: String },
    BranchPicker { query: String, sel: usize },
    ConfirmStashAndSwitch { target: String },
    Help,
}

struct App {
    rows: Vec<GraphRow>,
    status: Vec<StatusEntry>,
    tracking: TrackingInfo,
    branches: Vec<Branch>, // refreshed when opening the picker
    graph_sel: usize,
    status_sel: usize,
    graph_scroll: usize,
    diff_scroll: usize,
    focus: Focus,
    diff: String,
    message: String,
    quit: bool,
    mode: Mode,
}

impl App {
    fn new() -> Result<Self, String> {
        let commits = git::log_all(2000)?;
        let chain: HashSet<String> = git::main_chain()?.into_iter().collect();
        let rows = graph::assign_lanes(&commits, &chain);
        let status = git::status().unwrap_or_default();
        let tracking = git::tracking().unwrap_or_default();
        Ok(Self {
            rows,
            status,
            tracking,
            branches: Vec::new(),
            graph_sel: 0,
            status_sel: 0,
            graph_scroll: 0,
            diff_scroll: 0,
            focus: Focus::Graph,
            diff: String::new(),
            message: String::from("ready"),
            quit: false,
            mode: Mode::Normal,
        })
    }

    fn refresh(&mut self) {
        match git::log_all(2000) {
            Ok(commits) => {
                let chain: HashSet<String> = git::main_chain().unwrap_or_default().into_iter().collect();
                self.rows = graph::assign_lanes(&commits, &chain);
            }
            Err(e) => self.message = format!("log error: {e}"),
        }
        self.status = git::status().unwrap_or_default();
        self.tracking = git::tracking().unwrap_or_default();
        if self.graph_sel >= self.rows.len() && !self.rows.is_empty() {
            self.graph_sel = self.rows.len() - 1;
        }
        if self.status_sel >= self.status.len() && !self.status.is_empty() {
            self.status_sel = self.status.len() - 1;
        }
        self.update_diff();
    }

    fn update_diff(&mut self) {
        self.diff_scroll = 0;
        self.diff = match self.focus {
            Focus::Graph => self
                .rows
                .get(self.graph_sel)
                .and_then(|r| git::diff_commit(&r.commit.hash).ok())
                .unwrap_or_default(),
            Focus::Status => self
                .status
                .get(self.status_sel)
                .and_then(|e| {
                    let staged = matches!(e.status, git::FileStatus::Staged);
                    git::diff_file(&e.path, staged).ok()
                })
                .unwrap_or_default(),
        };
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new()?;
    app.update_diff();

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    ui::hide_cursor()?;

    let result = run(&mut app);

    ui::show_cursor()?;
    ui::reset()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;

    result.map_err(|e| e.into())
}

fn run(app: &mut App) -> Result<(), String> {
    loop {
        let (w, h) = terminal::size().map_err(|e| e.to_string())?;
        let layout = Layout::new(w, h);

        render(app, &layout).map_err(|e| e.to_string())?;
        io::stdout().flush().ok();

        if app.quit {
            return Ok(());
        }

        if event::poll(Duration::from_millis(500)).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(key) => handle_key(app, key, &layout),
                Event::Resize(_, _) => {
                    // Top of the loop will call terminal::size() and recompute
                    // the layout, so all we need to do is fall through.
                    // Clear so any artifacts from the larger size are wiped.
                    ui::clear().ok();
                }
                _ => {}
            }
        }
    }
}

fn render(app: &mut App, layout: &Layout) -> io::Result<()> {
    if layout.too_small() {
        return ui::draw_too_small(layout);
    }

    ui::clear()?;

    // Graph pane
    let max_rows = layout.height.saturating_sub(1) as usize;
    let max_lanes = (layout.graph_width as usize).saturating_sub(2) / 2;

    // Keep selection on screen even if the terminal just shrank.
    if app.graph_sel >= app.graph_scroll + max_rows.max(1) {
        app.graph_scroll = app.graph_sel + 1 - max_rows.max(1);
    }
    if app.graph_scroll > app.graph_sel {
        app.graph_scroll = app.graph_sel;
    }

    for (i, row) in app
        .rows
        .iter()
        .skip(app.graph_scroll)
        .take(max_rows)
        .enumerate()
    {
        let idx = app.graph_scroll + i;
        ui::draw_graph_row(
            row,
            i as u16,
            idx == app.graph_sel,
            app.focus == Focus::Graph,
            max_lanes,
        )?;
    }

    // Vertical separator
    for y in 0..layout.height.saturating_sub(1) {
        ui::move_to(y, layout.graph_width)?;
        ui::fg((60, 60, 70))?;
        write!(io::stdout(), "│")?;
        ui::reset()?;
    }

    // Right side: status list on top, diff below.
    ui::draw_status_pane(&app.status, app.status_sel, app.focus == Focus::Status, layout)?;
    ui::draw_diff_pane(&app.diff, layout, app.diff_scroll)?;

    // Status bar or active prompt
    match &app.mode {
        Mode::CommitMessage(buf) => {
            let prompt = format!("commit message: {buf}_");
            ui::draw_statusbar(layout, &app.tracking, &prompt, "[enter] commit  [esc] cancel")?;
        }
        Mode::NewBranch { name, .. } => {
            let prompt = format!("new branch: {name}_");
            ui::draw_statusbar(layout, &app.tracking, &prompt, "[enter] create  [esc] cancel")?;
        }
        Mode::ConfirmStashAndSwitch { target } => {
            let prompt = format!("uncommitted changes conflict with switch to '{target}'");
            ui::draw_statusbar(layout, &app.tracking, &prompt, "[s] stash & switch  [c] cancel")?;
        }
        Mode::Help | Mode::BranchPicker { .. } | Mode::Normal => {
            // Slim hint — full reference lives in the help overlay (`?`).
            let hint = match app.focus {
                Focus::Graph => "[?] help  [j/k] move  [enter] checkout  [tab] files  [q]uit",
                Focus::Status => "[?] help  [j/k] move  [space] stage  [c]ommit  [tab] graph  [q]uit",
            };
            ui::draw_statusbar(layout, &app.tracking, &app.message, hint)?;
        }
    }

    // Overlays sit on top of everything.
    if let Mode::BranchPicker { query, sel } = &app.mode {
        let matches = filtered_branches(&app.branches, query);
        ui::draw_branch_picker(layout, query, &matches, *sel)?;
    }
    if matches!(app.mode, Mode::Help) {
        ui::draw_help(layout)?;
    }

    Ok(())
}

fn filtered_branches<'a>(all: &'a [Branch], query: &str) -> Vec<&'a Branch> {
    if query.is_empty() {
        return all.iter().collect();
    }
    let q = query.to_lowercase();
    all.iter().filter(|b| b.name.to_lowercase().contains(&q)).collect()
}

fn handle_key(app: &mut App, key: KeyEvent, layout: &Layout) {
    // Modal states intercept all keys.
    match &app.mode {
        Mode::CommitMessage(_) => return handle_commit_message_key(app, key),
        Mode::NewBranch { .. } => return handle_new_branch_key(app, key),
        Mode::BranchPicker { .. } => return handle_picker_key(app, key),
        Mode::ConfirmStashAndSwitch { .. } => return handle_stash_confirm_key(app, key),
        Mode::Help => return handle_help_key(app, key),
        Mode::Normal => {}
    }

    // Global keys
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => {
            app.quit = true;
            return;
        }
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            app.quit = true;
            return;
        }
        (KeyCode::Char('?'), _) => {
            app.mode = Mode::Help;
            return;
        }
        (KeyCode::Tab, _) => {
            app.focus = if app.focus == Focus::Graph {
                Focus::Status
            } else {
                Focus::Graph
            };
            app.update_diff();
            return;
        }
        (KeyCode::Char('r'), _) => {
            app.refresh();
            app.message = "refreshed".into();
            return;
        }
        (KeyCode::Char('b'), _) => {
            open_branch_picker(app);
            return;
        }
        (KeyCode::Char('R'), _) => {
            open_pull_request(app);
            return;
        }
        (KeyCode::Char('P'), _) => {
            match git::pull() {
                Ok(out) => {
                    app.message = format!("pulled: {}", out.lines().next().unwrap_or("ok"));
                    app.refresh();
                }
                Err(e) => app.message = format!("pull failed: {}", e.lines().next().unwrap_or(&e)),
            }
            return;
        }
        (KeyCode::Char('p'), _) => {
            if app.tracking.behind > 0 {
                app.message = format!(
                    "behind by {} — pull first ([P]), or press [p] again to force-push intent",
                    app.tracking.behind
                );
                app.tracking.behind = 0;
                return;
            }
            match git::push() {
                Ok(_) => {
                    app.message = "pushed".into();
                    app.refresh();
                }
                Err(e) => app.message = format!("push failed: {}", e.lines().next().unwrap_or(&e)),
            }
            return;
        }
        _ => {}
    }

    // Focus-specific
    match app.focus {
        Focus::Graph => handle_graph_key(app, key, layout),
        Focus::Status => handle_status_key(app, key, layout),
    }
}

fn handle_graph_key(app: &mut App, key: KeyEvent, layout: &Layout) {
    let max_rows = layout.height.saturating_sub(1) as usize;
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if app.graph_sel + 1 < app.rows.len() {
                app.graph_sel += 1;
                if app.graph_sel >= app.graph_scroll + max_rows {
                    app.graph_scroll += 1;
                }
                app.update_diff();
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.graph_sel > 0 {
                app.graph_sel -= 1;
                if app.graph_sel < app.graph_scroll {
                    app.graph_scroll = app.graph_scroll.saturating_sub(1);
                }
                app.update_diff();
            }
        }
        KeyCode::Char('J') => app.diff_scroll += 1,
        KeyCode::Char('K') => app.diff_scroll = app.diff_scroll.saturating_sub(1),
        KeyCode::Enter | KeyCode::Char('o') => checkout_at_selection(app),
        KeyCode::Char('n') => {
            if let Some(row) = app.rows.get(app.graph_sel) {
                app.mode = Mode::NewBranch {
                    name: String::new(),
                    target_sha: row.commit.hash.clone(),
                };
            }
        }
        _ => {}
    }
}

fn handle_status_key(app: &mut App, key: KeyEvent, _layout: &Layout) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if app.status_sel + 1 < app.status.len() {
                app.status_sel += 1;
                app.update_diff();
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.status_sel > 0 {
                app.status_sel -= 1;
                app.update_diff();
            }
        }
        KeyCode::Char(' ') => {
            if let Some(entry) = app.status.get(app.status_sel).cloned() {
                let result = if matches!(entry.status, git::FileStatus::Staged) {
                    git::unstage(&entry.path)
                } else {
                    git::stage(&entry.path)
                };
                match result {
                    Ok(_) => {
                        app.message = format!("toggled {}", entry.path);
                        app.refresh();
                    }
                    Err(e) => app.message = format!("stage failed: {e}"),
                }
            }
        }
        KeyCode::Char('c') => {
            app.mode = Mode::CommitMessage(String::new());
        }
        _ => {}
    }
}

// ── Modal handlers ───────────────────────────────────────────────────────────

fn handle_commit_message_key(app: &mut App, key: KeyEvent) {
    let Mode::CommitMessage(buf) = &mut app.mode else { return };
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.message = "commit cancelled".into();
        }
        KeyCode::Enter => {
            let msg = buf.clone();
            app.mode = Mode::Normal;
            if msg.trim().is_empty() {
                app.message = "empty message, cancelled".into();
            } else {
                match git::commit(&msg) {
                    Ok(_) => {
                        app.message = format!("committed: {msg}");
                        app.refresh();
                    }
                    Err(e) => app.message = format!("commit failed: {}", e.lines().next().unwrap_or(&e)),
                }
            }
        }
        KeyCode::Backspace => {
            buf.pop();
        }
        KeyCode::Char(c) => buf.push(c),
        _ => {}
    }
}

fn handle_new_branch_key(app: &mut App, key: KeyEvent) {
    let Mode::NewBranch { name, target_sha } = &mut app.mode else { return };
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.message = "cancelled".into();
        }
        KeyCode::Enter => {
            let name_owned = name.clone();
            let sha = target_sha.clone();
            app.mode = Mode::Normal;
            if name_owned.trim().is_empty() {
                app.message = "empty name, cancelled".into();
            } else {
                match git::create_branch_at(name_owned.trim(), &sha) {
                    Ok(_) => {
                        app.message = format!("created and switched to {name_owned}");
                        app.refresh();
                    }
                    Err(e) => app.message = format!("create failed: {}", e.lines().next().unwrap_or(&e)),
                }
            }
        }
        KeyCode::Backspace => {
            name.pop();
        }
        KeyCode::Char(c) => name.push(c),
        _ => {}
    }
}

fn handle_picker_key(app: &mut App, key: KeyEvent) {
    // Take current query/sel out to sidestep the split-borrow hassle.
    let (query, sel) = match &app.mode {
        Mode::BranchPicker { query, sel } => (query.clone(), *sel),
        _ => return,
    };
    let match_count = filtered_branches(&app.branches, &query).len();

    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Enter => {
            let matches = filtered_branches(&app.branches, &query);
            if let Some(b) = matches.get(sel) {
                let name = b.name.clone();
                app.mode = Mode::Normal;
                attempt_checkout(app, &name);
            }
        }
        KeyCode::Down => {
            if sel + 1 < match_count {
                app.mode = Mode::BranchPicker { query, sel: sel + 1 };
            }
        }
        KeyCode::Up => {
            app.mode = Mode::BranchPicker { query, sel: sel.saturating_sub(1) };
        }
        KeyCode::Backspace => {
            let mut q = query;
            q.pop();
            app.mode = Mode::BranchPicker { query: q, sel: 0 };
        }
        KeyCode::Char(c) => {
            let mut q = query;
            q.push(c);
            app.mode = Mode::BranchPicker { query: q, sel: 0 };
        }
        _ => {}
    }
}

fn handle_help_key(app: &mut App, key: KeyEvent) {
    // Any key dismisses help — but Ctrl-C still quits the program.
    if let (KeyCode::Char('c'), KeyModifiers::CONTROL) = (key.code, key.modifiers) {
        app.quit = true;
        return;
    }
    app.mode = Mode::Normal;
}

fn handle_stash_confirm_key(app: &mut App, key: KeyEvent) {
    let Mode::ConfirmStashAndSwitch { target } = &app.mode else { return };
    let target = target.clone();
    match key.code {
        KeyCode::Char('s') => {
            app.mode = Mode::Normal;
            match git::stash_push() {
                Ok(_) => match git::checkout_branch(&target) {
                    Ok(_) => {
                        app.message = format!("stashed and switched to {target}");
                        app.refresh();
                    }
                    Err(e) => app.message = format!("switch still failed: {}", e.lines().next().unwrap_or(&e)),
                },
                Err(e) => app.message = format!("stash failed: {}", e.lines().next().unwrap_or(&e)),
            }
        }
        KeyCode::Char('c') | KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.message = "cancelled".into();
        }
        _ => {}
    }
}

// ── Checkout logic ───────────────────────────────────────────────────────────

fn open_branch_picker(app: &mut App) {
    match git::list_branches() {
        Ok(mut branches) => {
            // Put current branch on top, then locals alphabetically, then remotes.
            branches.sort_by(|a, b| {
                b.is_current
                    .cmp(&a.is_current)
                    .then(a.is_remote.cmp(&b.is_remote))
                    .then(a.name.cmp(&b.name))
            });
            app.branches = branches;
            app.mode = Mode::BranchPicker {
                query: String::new(),
                sel: 0,
            };
        }
        Err(e) => app.message = format!("list branches failed: {e}"),
    }
}

fn checkout_at_selection(app: &mut App) {
    let Some(row) = app.rows.get(app.graph_sel) else { return };
    let refs: Vec<String> = row
        .commit
        .refs
        .iter()
        .filter(|r| !r.starts_with("tag:"))
        .cloned()
        .collect();

    match refs.len() {
        0 => {
            app.message = "no branch here — [n] to create one, [b] to pick another".into();
        }
        1 => {
            let name = refs[0].clone();
            attempt_checkout(app, &name);
        }
        _ => {
            // Multiple refs on this commit → open picker prefiltered.
            // Simplest: open full picker with the first ref as query.
            open_branch_picker(app);
            if let Mode::BranchPicker { query, .. } = &mut app.mode {
                *query = refs[0].clone();
            }
        }
    }
}

/// Try to check out a branch. On dirty-tree conflict, enter stash-confirm mode.
fn attempt_checkout(app: &mut App, name: &str) {
    match git::checkout_branch(name) {
        Ok(_) => {
            app.message = format!("switched to {name}");
            app.refresh();
        }
        Err(e) => {
            if git::is_worktree_conflict(&e) {
                app.mode = Mode::ConfirmStashAndSwitch {
                    target: name.to_string(),
                };
            } else {
                app.message = format!("switch failed: {}", e.lines().next().unwrap_or(&e));
            }
        }
    }
}

// ── PR / MR opening ──────────────────────────────────────────────────────────

fn open_pull_request(app: &mut App) {
    // 1. Need to be on a branch (not detached).
    let head = match &app.tracking.branch {
        Some(b) => b.clone(),
        None => {
            app.message = "detached HEAD — switch to a branch first".into();
            return;
        }
    };

    // 2. Need a remote we recognize.
    let info = match remote::detect() {
        Some(i) => i,
        None => {
            app.message = "couldn't detect remote host".into();
            return;
        }
    };

    // 3. Need a target base branch — prefer the remote's HEAD, fall back to main.
    let base = git::default_base_branch().unwrap_or_else(|| "main".into());
    if base == head {
        app.message = format!("you're on {base} — switch to a feature branch first");
        return;
    }

    // 4. Need an upstream — `gh pr create` and the compare URLs both want
    //    the branch to exist on the remote.
    if app.tracking.upstream.is_none() {
        app.message = "no upstream — push first ([p]) so the branch exists on the remote".into();
        return;
    }

    // 5. Try the host CLI first; otherwise print the compare URL.
    if let Some(tool) = remote::cli_tool(&info.host) {
        let args = remote::pr_create_args(&info.host, &base);
        match run_suspended(tool, &args) {
            Ok(()) => {
                app.message = format!("PR opened via {tool}");
                app.refresh();
            }
            Err(e) => app.message = format!("{tool} failed: {e}"),
        }
    } else {
        let url = remote::compare_url(&info, &base, &head);
        app.message = format!("open: {url}");
    }
}

/// Drop out of the alternate screen, run a command attached to the real
/// terminal (so it can prompt the user), then come back. This is the same
/// pattern git itself uses to open `$EDITOR` mid-command.
fn run_suspended(program: &str, args: &[String]) -> Result<(), String> {
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
    use crossterm::ExecutableCommand;
    use std::process::Command;

    // Tear down the TUI.
    let _ = ui::show_cursor();
    let _ = ui::reset();
    io::stdout().execute(LeaveAlternateScreen).ok();
    disable_raw_mode().ok();

    // Run the command attached to the real stdin/stdout/stderr.
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("failed to spawn {program}: {e}"))?;

    // Restore the TUI no matter what the child did.
    enable_raw_mode().map_err(|e| e.to_string())?;
    io::stdout().execute(EnterAlternateScreen).map_err(|e| e.to_string())?;
    let _ = ui::hide_cursor();

    if !status.success() {
        return Err(format!("{program} exited with {status}"));
    }
    Ok(())
}
