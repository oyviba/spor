//! spor as a native desktop app. Same core as the terminal UI (`spor::git`,
//! `spor::graph`, …), rendered with egui. Built with `--features gui`; on
//! macOS `scripts/bundle-macos.sh` wraps it into `Spor.app`.
//!
//! The design follows macOS conventions rather than a classic git GUI: a
//! source-list sidebar switches between two views — **Changes** (what you're
//! working on, with a commit composer) and **History** (a quiet timeline) —
//! each laid out in columns like Mail. The toolbar is a unified title bar
//! with one Sync button; destructive actions are undoable (⌘Z) instead of
//! guarded by confirmation dialogs.

mod changes;
mod diff_view;
mod graph_view;
mod history;
mod modals;
mod sidebar;
mod theme;
mod titlebar;
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

pub use diff_view::DiffMode;

const LOG_LIMIT: usize = 2000;
const MAX_RECENT: usize = 8;

fn main() -> eframe::Result {
    fix_path_for_gui_launch();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Spor")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([900.0, 520.0])
        .with_drag_and_drop(true)
        // macOS: draw under a transparent title bar so the toolbar and the
        // traffic lights share one strip, like Finder, Mail or Xcode.
        .with_fullsize_content_view(true)
        .with_title_shown(false)
        .with_titlebar_shown(false);
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

/// Which main view is showing.
#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Changes,
    History,
}

/// What the sidebar highlights.
#[derive(Clone, PartialEq, Debug)]
enum SidebarSel {
    Changes,
    History,
    /// A branch or tag whose tip was revealed in History.
    Ref(String),
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
}

/// One changed file in the Changes list. git status reports a file that is
/// partly staged as two entries; here they're one row with a mixed checkbox.
#[derive(Clone, Debug)]
pub struct Change {
    pub path: String,
    pub orig_path: Option<String>,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    /// Deleted (in the index or the working tree).
    pub deleted: bool,
    pub added: bool,
    /// The raw entries, for staging/unstaging.
    pub entries: Vec<StatusEntry>,
}

impl Change {
    fn from_status(status: &[StatusEntry], staged_new: &HashSet<String>) -> Vec<Change> {
        let mut out: Vec<Change> = Vec::new();
        for e in status {
            let idx = match out.iter().position(|c| c.path == e.path) {
                Some(i) => i,
                None => {
                    out.push(Change {
                        path: e.path.clone(),
                        orig_path: None,
                        staged: false,
                        unstaged: false,
                        untracked: false,
                        deleted: false,
                        added: false,
                        entries: Vec::new(),
                    });
                    out.len() - 1
                }
            };
            let c = &mut out[idx];
            match e.status {
                FileStatus::Staged => {
                    c.staged = true;
                    c.added |= staged_new.contains(&e.path);
                }
                FileStatus::StagedDeleted => {
                    c.staged = true;
                    c.deleted = true;
                }
                FileStatus::Modified => c.unstaged = true,
                FileStatus::Deleted => {
                    c.unstaged = true;
                    c.deleted = true;
                }
                FileStatus::Untracked => {
                    c.unstaged = true;
                    c.untracked = true;
                    c.added = true;
                }
            }
            if e.orig_path.is_some() {
                c.orig_path = e.orig_path.clone();
            }
            c.entries.push(e.clone());
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// Fully staged, so its checkbox is ticked.
    pub fn included(&self) -> bool {
        self.staged && !self.unstaged
    }
}

/// Something ⌘Z can take back.
enum Undo {
    /// Put a file's working-tree contents back (None: it didn't exist).
    Restore {
        path: String,
        contents: Option<Vec<u8>>,
    },
    Stage(Vec<String>),
    Unstage(Vec<StatusEntry>),
    Uncommit {
        hash: String,
        summary: String,
        description: String,
    },
}

/// A slow network command running off the UI thread. It reports a
/// human-readable outcome ("Sent 2 commits") or an error.
struct Job {
    label: &'static str,
    rx: mpsc::Receiver<Result<String, String>>,
}

struct Repo {
    root: PathBuf,
    name: String,
    rows: Vec<GraphRow>,
    /// Row index of each ref's tip, keyed by ref name (tags as `tag:NAME`).
    ref_rows: HashMap<String, usize>,
    max_lanes: usize,
    status: Vec<StatusEntry>,
    changes: Vec<Change>,
    tracking: TrackingInfo,
    remotes: Vec<String>,
    branches: Vec<Branch>,
    stashes: Vec<String>,
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
            changes: Vec::new(),
            tracking: TrackingInfo::default(),
            remotes: Vec::new(),
            branches: Vec::new(),
            stashes: Vec::new(),
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
        self.changes = Change::from_status(&self.status, &git::staged_new_files());
        self.tracking = git::tracking().unwrap_or_default();
        self.stashes = git::stash_list();
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

/// The selected commit, loaded for the detail column.
struct Inspected {
    hash: String,
    details: CommitDetails,
    files: Vec<FileDiff>,
}

struct SporApp {
    repo: Option<Repo>,
    open_error: Option<String>,
    recent: Vec<PathBuf>,
    view: View,
    sidebar_sel: SidebarSel,
    sidebar_open: bool,
    /// Last laid-out sidebar width, so the title bar can line up with it.
    sidebar_w: f32,
    /// History: selected row, its loaded details, and the selected file.
    commit_sel: usize,
    inspected: Option<Inspected>,
    commit_file: Option<usize>,
    /// Changes: selected path.
    change_sel: Option<String>,
    diff: Option<DiffDoc>,
    diff_mode: DiffMode,
    scroll_to_sel: bool,
    summary: String,
    description: String,
    sidebar_filter: String,
    branch_filter: String,
    modal: Option<Modal>,
    job: Option<Job>,
    toasts: Vec<Toast>,
    undo: Vec<(String, Undo)>,
    needs_pr_fetch: bool,
    title: String,
}

impl SporApp {
    fn new(initial: Option<PathBuf>) -> Self {
        let mut app = Self {
            repo: None,
            open_error: None,
            recent: recent_repos(),
            view: View::Changes,
            sidebar_sel: SidebarSel::Changes,
            sidebar_open: true,
            sidebar_w: 220.0,
            commit_sel: 0,
            inspected: None,
            commit_file: None,
            change_sel: None,
            diff: None,
            diff_mode: DiffMode::Unified,
            scroll_to_sel: false,
            summary: String::new(),
            description: String::new(),
            sidebar_filter: String::new(),
            branch_filter: String::new(),
            modal: None,
            job: None,
            toasts: Vec::new(),
            undo: Vec::new(),
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
                let dirty = !repo.changes.is_empty();
                let head = repo.head_row().unwrap_or(0);
                self.repo = Some(repo);
                self.open_error = None;
                self.summary.clear();
                self.description.clear();
                self.modal = None;
                self.undo.clear();
                self.needs_pr_fetch = true;
                self.inspected = None;
                self.commit_sel = head;
                self.change_sel = None;
                // Start where the work is: uncommitted changes if there are
                // any, otherwise the history at HEAD.
                if dirty {
                    self.show_changes();
                } else {
                    self.show_history();
                }
                self.scroll_to_sel = true;
            }
            Err(e) => self.open_error = Some(e),
        }
    }

    fn close_repo(&mut self) {
        self.repo = None;
        self.inspected = None;
        self.diff = None;
        self.undo.clear();
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
            undoable: false,
        });
        if self.toasts.len() > 3 {
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

    /// Record an undoable action and say what happened, with an Undo button.
    fn did(&mut self, text: impl Into<String>, undo: Undo) {
        let text = text.into();
        self.undo.push((text.clone(), undo));
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
        self.toasts.retain(|t| !t.undoable);
        self.toasts.push(Toast {
            text,
            kind: ToastKind::Success,
            born: Instant::now(),
            undoable: true,
        });
    }

    // ── Navigation ───────────────────────────────────────────────────────────

    fn show_changes(&mut self) {
        self.view = View::Changes;
        self.sidebar_sel = SidebarSel::Changes;
        let keep = self.change_sel.clone();
        self.select_change(keep);
    }

    fn show_history(&mut self) {
        self.view = View::History;
        if !matches!(self.sidebar_sel, SidebarSel::Ref(_)) {
            self.sidebar_sel = SidebarSel::History;
        }
        self.inspected = None;
        self.select_commit(self.commit_sel);
    }

    /// Select a changed file (or the first one) and load its diff.
    fn select_change(&mut self, path: Option<String>) {
        let Some(repo) = &self.repo else { return };
        let change = path
            .and_then(|p| repo.changes.iter().find(|c| c.path == p))
            .or_else(|| repo.changes.first())
            .cloned();
        self.change_sel = change.as_ref().map(|c| c.path.clone());
        self.diff = change.and_then(|c| {
            git::diff_since_head(&c.path, c.untracked)
                .ok()
                .and_then(|p| diff::parse(&p).into_iter().next())
                .map(DiffDoc::new)
        });
    }

    fn select_commit(&mut self, i: usize) {
        let Some(row) = self.repo.as_ref().and_then(|r| r.rows.get(i)) else {
            self.inspected = None;
            self.diff = None;
            return;
        };
        self.commit_sel = i;
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
        self.select_commit_file(0);
    }

    fn select_commit_file(&mut self, idx: usize) {
        let file = self.inspected.as_ref().and_then(|i| i.files.get(idx));
        self.commit_file = file.map(|_| idx);
        self.diff = file.cloned().map(DiffDoc::new);
    }

    /// Move the History selection by `delta` rows.
    fn step(&mut self, delta: i32) {
        let Some(repo) = &self.repo else { return };
        let last = repo.rows.len().saturating_sub(1) as i32;
        let next = (self.commit_sel as i32 + delta).clamp(0, last) as usize;
        if next != self.commit_sel {
            self.select_commit(next);
            self.scroll_to_sel = true;
        }
    }

    /// Step through the Changes list.
    fn step_change(&mut self, delta: i32) {
        let Some(repo) = &self.repo else { return };
        if repo.changes.is_empty() {
            return;
        }
        let pos = self
            .change_sel
            .as_ref()
            .and_then(|p| repo.changes.iter().position(|c| &c.path == p))
            .unwrap_or(0) as i32;
        let next = (pos + delta).clamp(0, repo.changes.len() as i32 - 1) as usize;
        let path = repo.changes[next].path.clone();
        self.select_change(Some(path));
    }

    /// Show a branch or tag's tip in History.
    fn reveal_ref(&mut self, key: &str) {
        let Some(i) = self
            .repo
            .as_ref()
            .and_then(|r| r.ref_rows.get(key).copied())
        else {
            self.info(format!("{key} is beyond the loaded history"));
            return;
        };
        self.view = View::History;
        self.sidebar_sel = SidebarSel::Ref(key.to_string());
        self.select_commit(i);
        self.scroll_to_sel = true;
    }

    /// Reload everything from git, keeping selections where they still apply.
    fn refresh(&mut self) {
        let Some(repo) = &mut self.repo else { return };
        let keep_hash = self.inspected.as_ref().map(|i| i.hash.clone());
        if let Err(e) = repo.reload() {
            let msg = format!("Couldn't read history: {}", first_line(&e));
            self.error(msg);
        }
        let repo = self.repo.as_ref().expect("checked above");
        // Follow the same commit if it moved (e.g. after a commit).
        self.commit_sel = keep_hash
            .and_then(|h| repo.rows.iter().position(|r| r.commit.hash == h))
            .unwrap_or(self.commit_sel.min(repo.rows.len().saturating_sub(1)));
        self.inspected = None;
        match self.view {
            View::Changes => {
                let keep = self.change_sel.clone();
                self.select_change(keep);
            }
            View::History => {
                let keep_file = self.commit_file;
                self.select_commit(self.commit_sel);
                if let Some(f) = keep_file {
                    self.select_commit_file(f);
                }
            }
        }
    }

    // ── Working tree ─────────────────────────────────────────────────────────

    /// Tick or untick a file: stage everything in it, or unstage it.
    fn toggle_change(&mut self, change: &Change) {
        if change.included() {
            let staged: Vec<StatusEntry> = change
                .entries
                .iter()
                .filter(|e| e.status.is_staged())
                .cloned()
                .collect();
            if let Err(e) = staged.iter().try_for_each(git::unstage) {
                return self.error(format!("Couldn't unstage: {}", first_line(&e)));
            }
            self.undo.push((
                format!("Unstage {}", change.path),
                Undo::Stage(vec![change.path.clone()]),
            ));
        } else {
            if let Err(e) = git::stage(&change.path) {
                return self.error(format!("Couldn't stage: {}", first_line(&e)));
            }
            self.undo.push((
                format!("Stage {}", change.path),
                Undo::Unstage(vec![StatusEntry {
                    status: FileStatus::Staged,
                    path: change.path.clone(),
                    orig_path: change.orig_path.clone(),
                }]),
            ));
        }
        self.refresh();
    }

    /// Include or exclude every file.
    fn set_all_included(&mut self, include: bool) {
        let Some(repo) = &self.repo else { return };
        let changes = repo.changes.clone();
        let mut done_paths = Vec::new();
        let mut done_entries = Vec::new();
        for c in &changes {
            let r = if include && !c.included() {
                done_paths.push(c.path.clone());
                git::stage(&c.path)
            } else if !include && c.staged {
                let staged: Vec<StatusEntry> = c
                    .entries
                    .iter()
                    .filter(|e| e.status.is_staged())
                    .cloned()
                    .collect();
                done_entries.extend(staged.iter().cloned());
                staged.iter().try_for_each(git::unstage)
            } else {
                Ok(())
            };
            if let Err(e) = r {
                self.error(format!("Couldn't stage: {}", first_line(&e)));
                break;
            }
        }
        if include && !done_paths.is_empty() {
            self.undo.push((
                "Stage all".into(),
                Undo::Unstage(
                    done_paths
                        .into_iter()
                        .map(|path| StatusEntry {
                            status: FileStatus::Staged,
                            path,
                            orig_path: None,
                        })
                        .collect(),
                ),
            ));
        } else if !include && !done_entries.is_empty() {
            let paths = done_entries.iter().map(|e| e.path.clone()).collect();
            self.undo.push(("Unstage all".into(), Undo::Stage(paths)));
        }
        self.refresh();
    }

    /// Throw away a file's unstaged changes — immediately, but undoably: the
    /// current contents are kept in memory so ⌘Z can put them back.
    fn discard(&mut self, change: &Change) {
        let path = change.path.clone();
        let before = std::fs::read(&path).ok();
        let result = if change.untracked {
            git::remove_untracked(&path)
        } else {
            git::discard_worktree(&path)
        };
        match result {
            Ok(()) => {
                let name = widgets::split_path(&path).0.to_string();
                self.did(
                    format!("Discarded changes to {name}"),
                    Undo::Restore {
                        path,
                        contents: before,
                    },
                );
                self.refresh();
            }
            Err(e) => self.error(format!("Couldn't discard: {}", first_line(&e))),
        }
    }

    fn commit(&mut self) {
        let summary = self.summary.trim().to_string();
        if summary.is_empty() {
            return;
        }
        let description = self.description.trim().to_string();
        let msg = if description.is_empty() {
            summary.clone()
        } else {
            format!("{summary}\n\n{description}")
        };
        match git::commit(&msg) {
            Ok(()) => {
                let hash = git::head_sha().unwrap_or_default();
                self.summary.clear();
                self.description.clear();
                self.did(
                    format!("Committed “{summary}”"),
                    Undo::Uncommit {
                        hash,
                        summary,
                        description,
                    },
                );
                self.commit_sel = 0;
                self.refresh();
                self.commit_sel = self.repo.as_ref().and_then(Repo::head_row).unwrap_or(0);
            }
            Err(e) => self.error(format!("Commit failed: {}", first_line(&e))),
        }
    }

    /// Take back the most recent undoable action.
    fn undo_last(&mut self) {
        let Some((label, action)) = self.undo.pop() else {
            return self.info("Nothing to undo");
        };
        self.toasts.retain(|t| !t.undoable);
        let done = match &action {
            Undo::Restore { path, .. } => format!("Restored {}", widgets::split_path(path).0),
            Undo::Uncommit { .. } => "Commit undone — its changes are staged again".to_string(),
            _ => format!("Undid {}", label.to_lowercase()),
        };
        let result = match action {
            Undo::Restore { path, contents } => match contents {
                Some(bytes) => {
                    if let Some(dir) = Path::new(&path).parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    std::fs::write(&path, bytes).map_err(|e| e.to_string())
                }
                None => std::fs::remove_file(&path).map_err(|e| e.to_string()),
            },
            Undo::Stage(paths) => paths.iter().try_for_each(|p| git::stage(p)),
            Undo::Unstage(entries) => entries.iter().try_for_each(git::unstage),
            Undo::Uncommit {
                hash,
                summary,
                description,
            } => {
                if git::head_sha().ok().as_deref() != Some(hash.as_str()) {
                    Err("the branch has moved on since that commit".into())
                } else {
                    git::uncommit().map(|()| {
                        self.summary = summary;
                        self.description = description;
                        self.view = View::Changes;
                        self.sidebar_sel = SidebarSel::Changes;
                    })
                }
            }
        };
        match result {
            Ok(()) => {
                self.info(done);
                self.refresh();
            }
            Err(e) => self.error(format!("Couldn't undo: {}", first_line(&e))),
        }
    }

    // ── Branches & stashes ───────────────────────────────────────────────────

    fn checkout(&mut self, name: &str) {
        // Checking out `origin/x` lands on a local `x` tracking it rather than
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

    /// Check out the branch at the selected commit (double-click / Return).
    fn checkout_selected(&mut self) {
        let Some(repo) = &self.repo else { return };
        let Some(row) = repo.rows.get(self.commit_sel) else {
            return;
        };
        let is_remote = |r: &str| {
            r.split_once('/')
                .is_some_and(|(rem, _)| repo.remotes.iter().any(|x| x == rem))
        };
        let refs: Vec<&String> = row
            .commit
            .refs
            .iter()
            .filter(|r| !r.starts_with("tag:") && !r.ends_with("/HEAD"))
            .collect();
        // Prefer a local branch when local and remote twins share the tip.
        let pick = refs
            .iter()
            .find(|r| !is_remote(r))
            .or(refs.first())
            .map(|r| r.to_string());
        match pick {
            Some(name) if row.commit.head_ref.as_deref() == Some(name.as_str()) => {}
            Some(name) => self.checkout(&name),
            None => self.info("No branch here — right-click to create one"),
        }
    }

    fn stash_and_switch(&mut self, target: &str) {
        match git::stash_push().and_then(|_| git::checkout_branch(target)) {
            Ok(()) => {
                self.ok(format!("Stashed your changes and switched to {target}"));
                self.refresh();
            }
            Err(e) => self.error(format!("Couldn't switch: {}", first_line(&e))),
        }
    }

    fn stash(&mut self) {
        match git::stash_push() {
            Ok(()) => {
                self.ok("Changes stashed");
                self.refresh();
            }
            Err(e) => self.error(format!("Couldn't stash: {}", first_line(&e))),
        }
    }

    fn apply_stash(&mut self, index: usize) {
        match git::stash_pop_at(index) {
            Ok(()) => {
                self.ok("Stashed changes restored");
                self.refresh();
                self.show_changes();
            }
            Err(e) => self.error(format!("Couldn't apply stash: {}", first_line(&e))),
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

    /// New branch from the selected commit in History, or from HEAD.
    fn new_branch_here(&mut self) {
        let Some(repo) = &self.repo else { return };
        let i = match self.view {
            View::History => Some(self.commit_sel),
            View::Changes => repo.head_row(),
        };
        if let Some(row) = i.and_then(|i| repo.rows.get(i)) {
            let label = format!("{} — {}", row.commit.short, row.commit.subject);
            let sha = row.commit.hash.clone();
            self.new_branch_dialog(sha, label);
        }
    }

    // ── Network ──────────────────────────────────────────────────────────────

    /// One button for fetch + pull + push: bring the branch level with its
    /// upstream in whichever direction is needed, or publish it if it has
    /// none yet.
    fn sync(&mut self) {
        let Some(repo) = &self.repo else { return };
        let branch = repo.tracking.branch.clone();
        let has_upstream = repo.tracking.upstream.is_some();
        let remote = repo
            .remotes
            .iter()
            .find(|r| *r == "origin")
            .or(repo.remotes.first())
            .cloned();
        let Some(remote) = remote else {
            return self.info("This repository has no remote to sync with");
        };
        self.spawn("Sync", move |git| {
            git(&["fetch", "--all", "--prune"])?;
            let Some(branch) = branch else {
                return Ok("Fetched — HEAD is detached, so nothing to sync".into());
            };
            if !has_upstream {
                git(&["push", "--set-upstream", &remote, &branch])?;
                return Ok(format!("Published {branch} to {remote}"));
            }
            let counts = git(&["rev-list", "--left-right", "--count", "HEAD...@{u}"])?;
            let mut it = counts
                .split_whitespace()
                .map(|n| n.parse::<usize>().unwrap_or(0));
            let (ahead, behind) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
            if ahead > 0 && behind > 0 {
                return Err(format!(
                    "{branch} and its upstream have diverged ({ahead} here, {behind} there). \
                         Merge or rebase, then sync again."
                ));
            }
            if behind > 0 {
                git(&["pull", "--ff-only"])?;
                return Ok(format!("Received {behind} commit{}", plural(behind)));
            }
            if ahead > 0 {
                git(&["push"])?;
                return Ok(format!("Sent {ahead} commit{}", plural(ahead)));
            }
            Ok("Everything is up to date".into())
        });
    }

    fn fetch(&mut self) {
        self.spawn("Fetch", |git| {
            git(&["fetch", "--all", "--prune"]).map(|_| "Fetched all remotes".into())
        });
    }

    fn pull(&mut self) {
        self.spawn("Pull", |git| {
            git(&["pull", "--ff-only"]).map(|_| "Pulled".into())
        });
    }

    fn push(&mut self) {
        match git::push_args() {
            Ok(args) => self.spawn("Push", move |git| {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                git(&args).map(|_| "Pushed".into())
            }),
            Err(e) => self.error(format!("Push failed: {}", first_line(&e))),
        }
    }

    /// Run network work in the background. There's no terminal to prompt
    /// in, so credentials must come from a helper (the macOS keychain,
    /// ssh-agent); interactive prompts are disabled so git fails instead of
    /// hanging forever.
    fn spawn(
        &mut self,
        label: &'static str,
        work: impl FnOnce(&dyn Fn(&[&str]) -> Result<String, String>) -> Result<String, String>
            + Send
            + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let dir = self.repo.as_ref().map(|r| r.root.clone());
        std::thread::spawn(move || {
            let git = |args: &[&str]| -> Result<String, String> {
                let mut cmd = Command::new("git");
                if let Some(dir) = &dir {
                    cmd.current_dir(dir);
                }
                let out = cmd
                    .args(args)
                    .env("GIT_TERMINAL_PROMPT", "0")
                    .env("GIT_SSH_COMMAND", ssh_command())
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|e| format!("failed to run git: {e}"))?;
                if out.status.success() {
                    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
                } else {
                    Err(git_error(&String::from_utf8_lossy(&out.stderr)))
                }
            };
            let _ = tx.send(work(&git));
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
                    Ok(msg) => {
                        // Once commits are pushed, taking one back would
                        // rewrite published history — no longer undoable.
                        if matches!(label, "Sync" | "Push") {
                            self.undo
                                .retain(|(_, u)| !matches!(u, Undo::Uncommit { .. }));
                            self.toasts.retain(|t| !t.undoable);
                        }
                        self.ok(msg)
                    }
                    Err(e) => self.error(format!("{label} failed: {e}")),
                }
                self.refresh();
                self.needs_pr_fetch = true;
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
            return self.info("Switch to a branch first");
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
            return self.info("Sync first to publish this branch");
        }
        if let Some(pr) = repo.prs.get(&head) {
            let url = format!("{}/pull/{}", info.web_url, pr.number);
            ctx.open_url(egui::OpenUrl::new_tab(url));
            return;
        }
        let url = remote::compare_url(&info, &base, &head);
        ctx.open_url(egui::OpenUrl::new_tab(&url));
    }

    fn reveal_in_finder(&mut self, path: &str) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        let full = root.join(path);
        let result = if cfg!(target_os = "macos") {
            Command::new("open").arg("-R").arg(&full).spawn()
        } else {
            let dir = full.parent().map(Path::to_path_buf).unwrap_or(root);
            Command::new("xdg-open").arg(dir).spawn()
        };
        if let Err(e) = result {
            self.error(format!("Couldn't reveal file: {e}"));
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let cmd = |key: Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key));
        if cmd(Key::O) {
            self.pick_repo();
        }
        if self.repo.is_none() {
            return;
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        if cmd(Key::R) {
            self.refresh();
            self.needs_pr_fetch = true;
        }
        if cmd(Key::Num1) {
            self.show_changes();
        }
        if cmd(Key::Num2) {
            self.show_history();
        }
        // ⌘↩ commits from anywhere in Changes, including the message fields.
        if self.view == View::Changes && self.modal.is_none() && cmd(Key::Enter) {
            let staged = self
                .repo
                .as_ref()
                .is_some_and(|r| r.changes.iter().any(|c| c.staged));
            if staged && !self.summary.trim().is_empty() {
                self.commit();
            }
        }
        // ⌘Z belongs to the text field while typing.
        if !typing && cmd(Key::Z) {
            self.undo_last();
        }
        if self.modal.is_some() || typing {
            return;
        }
        let (down, up, enter, space) = ctx.input(|i| {
            (
                i.key_pressed(Key::J) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::K) || i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Space),
            )
        });
        match self.view {
            View::History => {
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
            View::Changes => {
                if down {
                    self.step_change(1);
                }
                if up {
                    self.step_change(-1);
                }
                if space {
                    let change = self.repo.as_ref().and_then(|r| {
                        r.changes
                            .iter()
                            .find(|c| Some(&c.path) == self.change_sel.as_ref())
                            .cloned()
                    });
                    if let Some(c) = change {
                        self.toggle_change(&c);
                    }
                }
            }
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// git prints hints around the real error; keep the last "fatal:"/"error:"
/// line, minus its prefix.
fn git_error(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        .or_else(|| stderr.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or("unknown error");
    line.trim_start_matches("fatal: ")
        .trim_start_matches("error: ")
        .trim()
        .to_string()
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
        if widgets::show_toasts(&ctx, &mut self.toasts) {
            self.undo_last();
        }
    }
}
