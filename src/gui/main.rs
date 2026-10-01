//! spor as a native desktop app. Same core as the terminal UI (`spor::git`,
//! `spor::graph`, …), rendered with egui. Built with `--features gui`; on
//! macOS `scripts/bundle-macos.sh` wraps it into `Spor.app`.
//!
//! Layout: toolbar on top, branches on the left, the commit list with the
//! diff of the selected file below it in the middle, and an inspector for
//! the selected commit (or the uncommitted changes) on the right.

mod diff_view;
mod graph_view;
mod inspector;
mod modals;
mod sidebar;
mod theme;
mod toolbar;
mod views;
mod widgets;

use diff_view::DiffDoc;
use eframe::egui::{self, Key};
use spor::diff::{self, FileDiff};
use spor::git::{self, Branch, CommitDetails, FileStatus, StatusEntry, TrackingInfo};
use spor::graph::{self, GraphRow};
use spor::remote::{self, PrInfo};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;
use widgets::{Toast, ToastKind};

const LOG_LIMIT: usize = 2000;
const MAX_RECENT: usize = 8;

fn main() -> eframe::Result {
    fix_path_for_gui_launch();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Spor")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([900.0, 520.0])
        .with_drag_and_drop(true);
    // The .app bundle carries its own icon; this covers `cargo run` and
    // non-mac platforms.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "spor",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(SporApp::new(initial_repo())))
        }),
    )
}

/// Apps launched from Finder/Dock inherit launchd's bare PATH
/// (/usr/bin:/bin:/usr/sbin:/sbin), which hides Homebrew's `git` and `gh`.
/// Append the usual install locations so PR badges and newer gits work.
fn fix_path_for_gui_launch() {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current
        .split(':')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !parts.iter().any(|p| p == extra) && Path::new(extra).is_dir() {
            parts.push(extra.to_string());
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}

/// Where to start: a path passed on the command line, else the working
/// directory if it's inside a repo (Finder launches apps in `/`, which never
/// is), else the most recently opened repository.
fn initial_repo() -> Option<PathBuf> {
    if let Some(arg) = std::env::args_os().nth(1) {
        return Some(PathBuf::from(arg));
    }
    std::env::current_dir()
        .ok()
        .filter(|cwd| cwd != Path::new("/") && repo_root(cwd).is_ok())
        .or_else(|| recent_repos().into_iter().next())
}

/// `~/Library/Application Support/spor/` on macOS,
/// `$XDG_CONFIG_HOME/spor/` (or `~/.config/spor/`) elsewhere.
fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home?.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))?
    };
    Some(base.join("spor"))
}

/// Recently opened repositories, newest first.
fn recent_repos() -> Vec<PathBuf> {
    let Some(file) = config_dir().map(|d| d.join("recent-repos")) else {
        return Vec::new();
    };
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

fn remember_repo(root: &Path) {
    let Some(dir) = config_dir() else { return };
    let mut list = recent_repos();
    list.retain(|p| p != root);
    list.insert(0, root.to_path_buf());
    list.truncate(MAX_RECENT);
    let body: Vec<String> = list.iter().map(|p| p.to_string_lossy().into()).collect();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("recent-repos"), body.join("\n"));
}

/// Resolve `dir` to its repository root, or explain why it isn't one.
fn repo_root(dir: &Path) -> Result<PathBuf, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string());
        return Err(format!("“{name}” isn't a Git repository"));
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

fn first_line(e: &str) -> &str {
    e.lines().next().unwrap_or(e)
}

/// What the inspector and diff pane are showing.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Sel {
    /// The uncommitted-changes pseudo-row.
    Wip,
    Commit(usize),
}

enum Modal {
    NewBranch {
        name: String,
        sha: String,
        label: String,
    },
    StashAndSwitch {
        target: String,
    },
    Discard {
        entry: StatusEntry,
    },
    PushBehind {
        behind: usize,
    },
}

/// A slow command (fetch/pull/push) running off the UI thread.
struct Job {
    label: &'static str,
    rx: mpsc::Receiver<Result<(), String>>,
}

struct Repo {
    root: PathBuf,
    name: String,
    rows: Vec<GraphRow>,
    /// Row index of each ref's tip, keyed by ref name (tags as `tag:NAME`).
    ref_rows: HashMap<String, usize>,
    max_lanes: usize,
    status: Vec<StatusEntry>,
    tracking: TrackingInfo,
    remotes: Vec<String>,
    branches: Vec<Branch>,
    stashes: usize,
    prs: HashMap<String, PrInfo>,
    pr_rx: Option<mpsc::Receiver<Vec<PrInfo>>>,
}

impl Repo {
    fn open(dir: &Path) -> Result<Self, String> {
        let root = repo_root(dir)?;
        // Every git call in spor::git runs in the working directory.
        std::env::set_current_dir(&root).map_err(|e| e.to_string())?;
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let mut repo = Self {
            root,
            name,
            rows: Vec::new(),
            ref_rows: HashMap::new(),
            max_lanes: 1,
            status: Vec::new(),
            tracking: TrackingInfo::default(),
            remotes: Vec::new(),
            branches: Vec::new(),
            stashes: 0,
            prs: HashMap::new(),
            pr_rx: None,
        };
        repo.reload()?;
        Ok(repo)
    }

    fn reload(&mut self) -> Result<(), String> {
        self.remotes = git::remotes();
        let commits = git::log_all(LOG_LIMIT)?;
        let chain: HashSet<String> = git::main_chain().unwrap_or_default().into_iter().collect();
        self.rows = graph::assign_lanes(&commits, &chain, &self.remotes);
        self.max_lanes = self
            .rows
            .iter()
            .map(|r| {
                r.lanes_after
                    .len()
                    .max(r.lanes_before.len())
                    .max(r.lane + 1)
            })
            .max()
            .unwrap_or(1);
        self.ref_rows.clear();
        for (i, row) in self.rows.iter().enumerate() {
            for r in &row.commit.refs {
                let key = match r.strip_prefix("tag:") {
                    Some(t) => format!("tag:{}", t.trim()),
                    None => r.clone(),
                };
                self.ref_rows.entry(key).or_insert(i);
            }
        }
        self.status = git::status().unwrap_or_default();
        self.tracking = git::tracking().unwrap_or_default();
        self.stashes = git::stash_count();
        let mut branches = git::list_branches().unwrap_or_default();
        branches.sort_by_key(|b| b.name.to_lowercase());
        self.branches = branches;
        Ok(())
    }

    fn head_row(&self) -> Option<usize> {
        self.rows.iter().position(|r| r.commit.head_ref.is_some())
    }

    /// `gh pr list` hits the network — never on the UI thread.
    fn spawn_pr_fetch(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.pr_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(remote::fetch_prs().unwrap_or_default());
            ctx.request_repaint();
        });
    }

    fn poll_pr_fetch(&mut self) {
        let Some(rx) = &self.pr_rx else { return };
        match rx.try_recv() {
            Ok(prs) => {
                self.prs = prs
                    .into_iter()
                    .map(|p| (p.head_branch.clone(), p))
                    .collect();
                self.pr_rx = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => self.pr_rx = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

/// The selected commit, loaded for the inspector.
struct Inspected {
    hash: String,
    details: CommitDetails,
    files: Vec<FileDiff>,
}

struct SporApp {
    repo: Option<Repo>,
    open_error: Option<String>,
    recent: Vec<PathBuf>,
    sel: Sel,
    inspected: Option<Inspected>,
    /// Selected file: an index into `inspected.files` for a commit, or into
    /// `repo.status` for the uncommitted changes.
    file_sel: Option<usize>,
    diff: Option<DiffDoc>,
    scroll_to_sel: bool,
    summary: String,
    description: String,
    sidebar_filter: String,
    branch_filter: String,
    modal: Option<Modal>,
    job: Option<Job>,
    toasts: Vec<Toast>,
    needs_pr_fetch: bool,
    title: String,
}

impl SporApp {
    fn new(initial: Option<PathBuf>) -> Self {
        let mut app = Self {
            repo: None,
            open_error: None,
            recent: recent_repos(),
            sel: Sel::Commit(0),
            inspected: None,
            file_sel: None,
            diff: None,
            scroll_to_sel: false,
            summary: String::new(),
            description: String::new(),
            sidebar_filter: String::new(),
            branch_filter: String::new(),
            modal: None,
            job: None,
            toasts: Vec::new(),
            needs_pr_fetch: false,
            title: String::new(),
        };
        if let Some(dir) = initial {
            // On failure this lands on the welcome screen with the reason.
            app.open_repo(&dir);
        }
        app
    }

    fn open_repo(&mut self, dir: &Path) {
        match Repo::open(dir) {
            Ok(repo) => {
                remember_repo(&repo.root);
                self.recent = recent_repos();
                self.repo = Some(repo);
                self.open_error = None;
                self.summary.clear();
                self.description.clear();
                self.modal = None;
                self.needs_pr_fetch = true;
                self.inspected = None;
                let has_wip = self.repo.as_ref().is_some_and(|r| !r.status.is_empty());
                let start = if has_wip {
                    Sel::Wip
                } else {
                    Sel::Commit(self.repo.as_ref().and_then(Repo::head_row).unwrap_or(0))
                };
                self.select(start);
                self.scroll_to_sel = true;
            }
            Err(e) => self.open_error = Some(e),
        }
    }

    fn close_repo(&mut self) {
        self.repo = None;
        self.inspected = None;
        self.diff = None;
        self.file_sel = None;
        self.recent = recent_repos();
    }

    fn pick_repo(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Open Git Repository")
            .pick_folder()
        {
            self.open_repo(&dir);
        }
    }

    fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            kind,
            born: Instant::now(),
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn ok(&mut self, text: impl Into<String>) {
        self.toast(ToastKind::Success, text);
    }

    fn info(&mut self, text: impl Into<String>) {
        self.toast(ToastKind::Info, text);
    }

    fn error(&mut self, text: impl Into<String>) {
        self.toast(ToastKind::Error, text);
    }

    fn has_wip(&self) -> bool {
        self.repo.as_ref().is_some_and(|r| !r.status.is_empty())
    }

    /// Reload everything from git, keeping the selection where it makes sense.
    fn refresh(&mut self) {
        let Some(repo) = &mut self.repo else { return };
        let keep_hash = self.inspected.as_ref().map(|i| i.hash.clone());
        if let Err(e) = repo.reload() {
            let msg = format!("Couldn't read history: {}", first_line(&e));
            self.error(msg);
        }
        let rows = &self.repo.as_ref().expect("checked above").rows;
        let sel = match self.sel {
            Sel::Wip if self.has_wip() => Sel::Wip,
            Sel::Wip => Sel::Commit(self.repo.as_ref().and_then(Repo::head_row).unwrap_or(0)),
            Sel::Commit(i) => {
                // Follow the same commit if it moved (e.g. after a commit).
                let found = keep_hash
                    .as_ref()
                    .and_then(|h| rows.iter().position(|r| &r.commit.hash == h));
                Sel::Commit(found.unwrap_or(i.min(rows.len().saturating_sub(1))))
            }
        };
        let file = self.file_sel;
        self.inspected = None;
        self.select(sel);
        // Stay on the same file in the working tree when it still exists.
        if sel == Sel::Wip {
            if let Some(f) = file {
                let n = self.repo.as_ref().map_or(0, |r| r.status.len());
                if n > 0 {
                    self.select_file(f.min(n - 1));
                }
            }
        }
    }

    fn select(&mut self, sel: Sel) {
        self.sel = sel;
        match sel {
            Sel::Wip => {
                self.inspected = None;
                let any = self.has_wip();
                self.file_sel = None;
                self.diff = None;
                if any {
                    self.select_file(0);
                }
            }
            Sel::Commit(i) => {
                let Some(row) = self.repo.as_ref().and_then(|r| r.rows.get(i)) else {
                    self.inspected = None;
                    self.diff = None;
                    return;
                };
                if self
                    .inspected
                    .as_ref()
                    .is_some_and(|x| x.hash == row.commit.hash)
                {
                    return;
                }
                let hash = row.commit.hash.clone();
                let details = git::commit_details(&hash).unwrap_or_default();
                let files = git::commit_patch(&hash, &row.commit.parents)
                    .map(|p| diff::parse(&p))
                    .unwrap_or_default();
                self.inspected = Some(Inspected {
                    hash,
                    details,
                    files,
                });
                self.file_sel = None;
                self.diff = None;
                self.select_file(0);
            }
        }
    }

    fn select_file(&mut self, idx: usize) {
        match self.sel {
            Sel::Wip => {
                let Some(entry) = self.repo.as_ref().and_then(|r| r.status.get(idx)) else {
                    return;
                };
                self.file_sel = Some(idx);
                let parsed = git::diff_entry(entry)
                    .map(|p| diff::parse(&p))
                    .unwrap_or_default();
                self.diff = parsed.into_iter().next().map(DiffDoc::new);
            }
            Sel::Commit(_) => {
                let Some(f) = self.inspected.as_ref().and_then(|i| i.files.get(idx)) else {
                    return;
                };
                self.file_sel = Some(idx);
                self.diff = Some(DiffDoc::new(f.clone()));
            }
        }
    }

    /// Move the commit-list selection by `delta` rows (the WIP row counts).
    fn step(&mut self, delta: i32) {
        let Some(repo) = &self.repo else { return };
        let wip = self.has_wip();
        let pos = match self.sel {
            Sel::Wip => 0,
            Sel::Commit(i) => i as i32 + wip as i32,
        };
        let total = repo.rows.len() as i32 + wip as i32;
        let next = (pos + delta).clamp(0, total - 1);
        if next == pos {
            return;
        }
        let sel = if wip && next == 0 {
            Sel::Wip
        } else {
            Sel::Commit((next - wip as i32) as usize)
        };
        self.select(sel);
        self.scroll_to_sel = true;
    }

    /// Select the commit a ref points at (sidebar click).
    fn reveal_ref(&mut self, key: &str) {
        let Some(i) = self
            .repo
            .as_ref()
            .and_then(|r| r.ref_rows.get(key).copied())
        else {
            self.info(format!("{key} is beyond the loaded history"));
            return;
        };
        self.select(Sel::Commit(i));
        self.scroll_to_sel = true;
    }

    // ── Actions ──────────────────────────────────────────────────────────────

    fn toggle_stage(&mut self, entry: &StatusEntry) {
        let result = if entry.status.is_staged() {
            git::unstage(entry)
        } else {
            git::stage(&entry.path)
        };
        match result {
            Ok(()) => self.refresh(),
            Err(e) => self.error(format!("Couldn't stage: {}", first_line(&e))),
        }
    }

    fn stage_all(&mut self, staged: bool) {
        let Some(repo) = &self.repo else { return };
        let entries: Vec<StatusEntry> = repo
            .status
            .iter()
            .filter(|e| e.status.is_staged() == staged)
            .cloned()
            .collect();
        for e in &entries {
            let r = if staged {
                git::unstage(e)
            } else {
                git::stage(&e.path)
            };
            if let Err(err) = r {
                self.error(format!("Couldn't stage: {}", first_line(&err)));
                break;
            }
        }
        self.refresh();
    }

    fn discard(&mut self, entry: &StatusEntry) {
        let result = match entry.status {
            FileStatus::Untracked => git::remove_untracked(&entry.path),
            _ => git::discard_worktree(&entry.path),
        };
        match result {
            Ok(()) => {
                self.ok(format!("Discarded changes to {}", entry.path));
                self.refresh();
            }
            Err(e) => self.error(format!("Couldn't discard: {}", first_line(&e))),
        }
    }

    fn commit(&mut self) {
        let summary = self.summary.trim();
        if summary.is_empty() {
            return;
        }
        let msg = match self.description.trim() {
            "" => summary.to_string(),
            body => format!("{summary}\n\n{body}"),
        };
        match git::commit(&msg) {
            Ok(()) => {
                self.ok(format!("Committed “{}”", first_line(&msg)));
                self.summary.clear();
                self.description.clear();
                self.refresh();
                let head = self.repo.as_ref().and_then(Repo::head_row).unwrap_or(0);
                if !self.has_wip() || self.sel != Sel::Wip {
                    self.select(Sel::Commit(head));
                }
            }
            Err(e) => self.error(format!("Commit failed: {}", first_line(&e))),
        }
    }

    fn checkout(&mut self, name: &str) {
        // Checking out `origin/x` should land on a local `x` tracking it, not
        // a detached HEAD.
        let target = match self.repo.as_ref() {
            Some(repo) => match name.split_once('/') {
                Some((remote, rest)) if repo.remotes.iter().any(|r| r == remote) => {
                    rest.to_string()
                }
                _ => name.to_string(),
            },
            None => name.to_string(),
        };
        match git::checkout_branch(&target) {
            Ok(()) => {
                self.ok(format!("Switched to {target}"));
                self.refresh();
            }
            Err(e) if git::is_worktree_conflict(&e) => {
                self.modal = Some(Modal::StashAndSwitch { target });
            }
            Err(e) => self.error(format!("Couldn't switch: {}", first_line(&e))),
        }
    }

    fn checkout_selected(&mut self) {
        let Sel::Commit(i) = self.sel else { return };
        let Some(row) = self.repo.as_ref().and_then(|r| r.rows.get(i)) else {
            return;
        };
        let refs: Vec<String> = row
            .commit
            .refs
            .iter()
            .filter(|r| !r.starts_with("tag:") && !r.ends_with("/HEAD"))
            .cloned()
            .collect();
        match refs.as_slice() {
            [] => self.info("No branch here — right-click to create one"),
            [only] => {
                let only = only.clone();
                self.checkout(&only);
            }
            // Prefer a local branch when local and remote twins share the tip.
            [first, ..] => {
                let local = refs
                    .iter()
                    .find(|r| {
                        !self.repo.as_ref().is_some_and(|repo| {
                            r.split_once('/')
                                .is_some_and(|(rem, _)| repo.remotes.iter().any(|x| x == rem))
                        })
                    })
                    .unwrap_or(first)
                    .clone();
                self.checkout(&local);
            }
        }
    }

    fn stash_and_switch(&mut self, target: &str) {
        match git::stash_push().and_then(|_| git::checkout_branch(target)) {
            Ok(()) => {
                self.ok(format!("Stashed changes and switched to {target}"));
                self.refresh();
            }
            Err(e) => self.error(format!("Stash & switch failed: {}", first_line(&e))),
        }
    }

    fn stash(&mut self) {
        match git::stash_push() {
            Ok(()) => {
                self.ok("Stashed uncommitted changes");
                self.refresh();
            }
            Err(e) => self.error(format!("Stash failed: {}", first_line(&e))),
        }
    }

    fn stash_pop(&mut self) {
        match git::stash_pop() {
            Ok(()) => {
                self.ok("Restored stashed changes");
                self.refresh();
                if self.has_wip() {
                    self.select(Sel::Wip);
                    self.scroll_to_sel = true;
                }
            }
            Err(e) => self.error(format!("Couldn't pop stash: {}", first_line(&e))),
        }
    }

    fn create_branch(&mut self, name: &str, sha: &str) {
        match git::create_branch_at(name, sha) {
            Ok(()) => {
                self.ok(format!("Created and switched to {name}"));
                self.refresh();
            }
            Err(e) => self.error(format!("Couldn't create branch: {}", first_line(&e))),
        }
    }

    fn new_branch_dialog(&mut self, sha: String, label: String) {
        self.modal = Some(Modal::NewBranch {
            name: String::new(),
            sha,
            label,
        });
    }

    fn request_push(&mut self) {
        let behind = self.repo.as_ref().map_or(0, |r| r.tracking.behind);
        if behind > 0 {
            self.modal = Some(Modal::PushBehind { behind });
        } else {
            self.push();
        }
    }

    fn push(&mut self) {
        match git::push_args() {
            Ok(args) => self.spawn_job("Push", args),
            Err(e) => self.error(format!("Push failed: {}", first_line(&e))),
        }
    }

    fn pull(&mut self) {
        self.spawn_job("Pull", vec!["pull".into(), "--ff-only".into()]);
    }

    fn fetch(&mut self) {
        self.spawn_job(
            "Fetch",
            vec!["fetch".into(), "--all".into(), "--prune".into()],
        );
    }

    /// Run a network git command in the background. There's no terminal to
    /// prompt in, so credentials must come from a helper (the macOS keychain,
    /// ssh-agent); interactive prompts are disabled so git fails instead of
    /// hanging forever.
    fn spawn_job(&mut self, label: &'static str, args: Vec<String>) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let dir = self.repo.as_ref().map(|r| r.root.clone());
        std::thread::spawn(move || {
            let mut cmd = Command::new("git");
            if let Some(dir) = dir {
                cmd.current_dir(dir);
            }
            let result = cmd
                .args(&args)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_SSH_COMMAND", ssh_command())
                .stdin(Stdio::null())
                .output()
                .map_err(|e| format!("failed to run git: {e}"))
                .and_then(|out| {
                    if out.status.success() {
                        Ok(())
                    } else {
                        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
                    }
                });
            let _ = tx.send(result);
        });
        self.job = Some(Job { label, rx });
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        match job.rx.try_recv() {
            Ok(result) => {
                let label = job.label;
                self.job = None;
                match result {
                    Ok(()) => {
                        let done = match label {
                            "Push" => "Pushed",
                            "Pull" => "Pulled",
                            _ => "Fetched",
                        };
                        self.ok(format!("{done} successfully"));
                        self.refresh();
                        self.needs_pr_fetch = true;
                    }
                    Err(e) => {
                        // git prints hints after the real error; the last
                        // "fatal:"/"error:" line is the useful one.
                        let line = e
                            .lines()
                            .rev()
                            .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
                            .unwrap_or_else(|| first_line(&e))
                            .trim_start_matches("fatal: ")
                            .trim_start_matches("error: ")
                            .to_string();
                        self.error(format!("{label} failed: {line}"));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100))
            }
            Err(mpsc::TryRecvError::Disconnected) => self.job = None,
        }
    }

    fn open_pull_request(&mut self, ctx: &egui::Context) {
        let Some(repo) = &self.repo else { return };
        let Some(head) = repo.tracking.branch.clone() else {
            return self.info("Detached HEAD — switch to a branch first");
        };
        let Some(info) = remote::detect() else {
            return self.error("Couldn't detect the remote host");
        };
        let base = git::default_base_branch().unwrap_or_else(|| "main".into());
        if base == head {
            return self.info(format!(
                "You're on {base} — switch to a feature branch first"
            ));
        }
        if repo.tracking.upstream.is_none() {
            return self.info("Push this branch first so it exists on the remote");
        }
        if let Some(pr) = repo.prs.get(&head) {
            let url = format!("{}/pull/{}", info.web_url, pr.number);
            ctx.open_url(egui::OpenUrl::new_tab(url));
            return;
        }
        let url = remote::compare_url(&info, &base, &head);
        ctx.open_url(egui::OpenUrl::new_tab(&url));
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (cmd_o, cmd_r) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(Key::O),
                i.modifiers.command && i.key_pressed(Key::R),
            )
        });
        if cmd_o {
            self.pick_repo();
        }
        if cmd_r && self.repo.is_some() {
            self.refresh();
            self.needs_pr_fetch = true;
        }
        if self.modal.is_some() || ctx.memory(|m| m.focused().is_some()) || self.repo.is_none() {
            return;
        }
        let (down, up, enter) = ctx.input(|i| {
            (
                i.key_pressed(Key::J) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::K) || i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter),
            )
        });
        if down {
            self.step(1);
        }
        if up {
            self.step(-1);
        }
        if enter {
            self.checkout_selected();
        }
    }
}

/// ssh must never wait on a terminal prompt we can't show; fail fast instead
/// and let the error surface as a toast. Respects a user-set command.
fn ssh_command() -> String {
    std::env::var("GIT_SSH_COMMAND").unwrap_or_else(|_| "ssh -o BatchMode=yes".into())
}

impl eframe::App for SporApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Folders dropped on the window open as repos.
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.open_repo(&path);
        }

        self.poll_job(&ctx);
        if let Some(repo) = &mut self.repo {
            if std::mem::take(&mut self.needs_pr_fetch) {
                repo.spawn_pr_fetch(&ctx);
            }
            repo.poll_pr_fetch();
        }
        self.handle_keys(&ctx);

        let title = match &self.repo {
            Some(r) => format!("{} — Spor", r.name),
            None => "Spor".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        if self.repo.is_none() {
            self.welcome(ui);
        } else {
            self.workspace(ui);
        }

        self.modals(&ctx);
        widgets::show_toasts(&ctx, &mut self.toasts);
    }
}
