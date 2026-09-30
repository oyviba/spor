//! spor as a native desktop app. Same core as the terminal UI (`spor::git`,
//! `spor::graph`, …), rendered with egui. Built with `--features gui`; on
//! macOS `scripts/bundle-macos.sh` wraps it into `Spor.app`.

mod graph_view;

use eframe::egui::{self, Color32, Key, RichText, Sense, TextWrapMode};
use spor::git::{self, Branch, FileStatus, StatusEntry, TrackingInfo};
use spor::graph::{self, GraphRow};
use spor::remote::{self, PrInfo};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use graph_view::{RepoMeta, DIM, HEAD_GOLD, ROW_HEIGHT};

const LOG_LIMIT: usize = 2000;
const GREEN: Color32 = Color32::from_rgb(120, 200, 120);
const RED: Color32 = Color32::from_rgb(220, 100, 100);
const YELLOW: Color32 = Color32::from_rgb(220, 180, 60);
const CYAN: Color32 = Color32::from_rgb(100, 180, 220);

fn main() -> eframe::Result {
    fix_path_for_gui_launch();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("spor")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([720.0, 420.0])
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
            cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
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
/// is), else the repository open when the app last quit.
fn initial_repo() -> Option<PathBuf> {
    if let Some(arg) = std::env::args_os().nth(1) {
        return Some(PathBuf::from(arg));
    }
    std::env::current_dir()
        .ok()
        .filter(|cwd| cwd != Path::new("/") && repo_root(cwd).is_ok())
        .or_else(|| {
            let saved = std::fs::read_to_string(last_repo_file()?).ok()?;
            Some(PathBuf::from(saved.trim()))
        })
}

/// `~/Library/Application Support/spor/last-repo` on macOS,
/// `$XDG_CONFIG_HOME/spor/last-repo` (or `~/.config/…`) elsewhere.
fn last_repo_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home?.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))?
    };
    Some(base.join("spor").join("last-repo"))
}

fn remember_repo(root: &Path) {
    if let Some(file) = last_repo_file() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, root.to_string_lossy().as_bytes());
    }
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
        return Err(format!("{} is not a git repository", dir.display()));
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

fn first_line(e: &str) -> &str {
    e.lines().next().unwrap_or(e)
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Graph,
    Files,
}

enum Modal {
    NewBranch {
        name: String,
        sha: String,
        short: String,
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

/// A slow command (push/pull) running off the UI thread.
struct Job {
    label: String,
    rx: mpsc::Receiver<Result<(), String>>,
}

struct Repo {
    root: PathBuf,
    name: String,
    rows: Vec<GraphRow>,
    status: Vec<StatusEntry>,
    tracking: TrackingInfo,
    remotes: Vec<String>,
    branches: Vec<Branch>,
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
            status: Vec::new(),
            tracking: TrackingInfo::default(),
            remotes: Vec::new(),
            branches: Vec::new(),
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
        self.status = git::status().unwrap_or_default();
        self.tracking = git::tracking().unwrap_or_default();
        let mut branches = git::list_branches().unwrap_or_default();
        branches.sort_by(|a, b| {
            b.is_current
                .cmp(&a.is_current)
                .then(a.is_remote.cmp(&b.is_remote))
                .then(a.name.cmp(&b.name))
        });
        self.branches = branches;
        Ok(())
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

struct SporApp {
    repo: Option<Repo>,
    open_error: Option<String>,
    focus: Focus,
    graph_sel: usize,
    file_sel: usize,
    scroll_to_sel: bool,
    diff: String,
    commit_msg: String,
    branch_filter: String,
    message: String,
    message_is_error: bool,
    modal: Option<Modal>,
    job: Option<Job>,
    needs_pr_fetch: bool,
    title: String,
    tab_pressed: bool,
}

impl SporApp {
    fn new(initial: Option<PathBuf>) -> Self {
        let mut app = Self {
            repo: None,
            open_error: None,
            focus: Focus::Graph,
            graph_sel: 0,
            file_sel: 0,
            scroll_to_sel: false,
            diff: String::new(),
            commit_msg: String::new(),
            branch_filter: String::new(),
            message: String::new(),
            message_is_error: false,
            modal: None,
            job: None,
            needs_pr_fetch: false,
            title: String::new(),
            tab_pressed: false,
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
                self.info(format!("opened {}", repo.root.display()));
                self.repo = Some(repo);
                self.open_error = None;
                self.graph_sel = 0;
                self.file_sel = 0;
                self.focus = Focus::Graph;
                self.commit_msg.clear();
                self.modal = None;
                self.needs_pr_fetch = true;
                self.update_diff();
            }
            Err(e) => self.open_error = Some(e),
        }
    }

    fn pick_repo(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Open Git Repository")
            .pick_folder()
        {
            self.open_repo(&dir);
        }
    }

    fn info(&mut self, msg: impl Into<String>) {
        self.message = msg.into();
        self.message_is_error = false;
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.message = msg.into();
        self.message_is_error = true;
    }

    fn refresh(&mut self) {
        let Some(repo) = &mut self.repo else { return };
        let reloaded = repo.reload();
        let (rows, files) = (repo.rows.len(), repo.status.len());
        if let Err(e) = reloaded {
            self.error(format!("log error: {}", first_line(&e)));
        }
        self.graph_sel = self.graph_sel.min(rows.saturating_sub(1));
        self.file_sel = self.file_sel.min(files.saturating_sub(1));
        if self.focus == Focus::Files && files == 0 {
            self.focus = Focus::Graph;
        }
        self.update_diff();
    }

    fn update_diff(&mut self) {
        let Some(repo) = &self.repo else {
            self.diff.clear();
            return;
        };
        self.diff = match self.focus {
            Focus::Graph => repo
                .rows
                .get(self.graph_sel)
                .and_then(|r| git::diff_commit(&r.commit.hash).ok()),
            Focus::Files => repo
                .status
                .get(self.file_sel)
                .and_then(|e| git::diff_entry(e).ok()),
        }
        .unwrap_or_default();
    }

    fn select_commit(&mut self, idx: usize) {
        self.focus = Focus::Graph;
        self.graph_sel = idx;
        self.update_diff();
    }

    fn select_file(&mut self, idx: usize) {
        self.focus = Focus::Files;
        self.file_sel = idx;
        self.update_diff();
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
            Err(e) => self.error(format!("stage failed: {}", first_line(&e))),
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
                self.error(format!("stage failed: {}", first_line(&err)));
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
                self.info(format!("discarded {}", entry.path));
                self.refresh();
            }
            Err(e) => self.error(format!("discard failed: {}", first_line(&e))),
        }
    }

    fn commit(&mut self) {
        let msg = self.commit_msg.trim().to_string();
        if msg.is_empty() {
            return;
        }
        match git::commit(&msg) {
            Ok(()) => {
                self.info(format!("committed: {}", first_line(&msg)));
                self.commit_msg.clear();
                self.focus = Focus::Graph;
                self.graph_sel = 0;
                self.refresh();
            }
            Err(e) => self.error(format!("commit failed: {}", first_line(&e))),
        }
    }

    fn checkout(&mut self, name: &str) {
        match git::checkout_branch(name) {
            Ok(()) => {
                self.info(format!("switched to {name}"));
                self.refresh();
            }
            Err(e) if git::is_worktree_conflict(&e) => {
                self.modal = Some(Modal::StashAndSwitch {
                    target: name.to_string(),
                });
            }
            Err(e) => self.error(format!("switch failed: {}", first_line(&e))),
        }
    }

    fn checkout_selected_commit(&mut self) {
        let Some(row) = self.repo.as_ref().and_then(|r| r.rows.get(self.graph_sel)) else {
            return;
        };
        let refs: Vec<String> = row
            .commit
            .refs
            .iter()
            .filter(|r| !r.starts_with("tag:"))
            .cloned()
            .collect();
        match refs.as_slice() {
            [] => self.info("no branch here — right-click to create one"),
            [only] => {
                let only = only.clone();
                self.checkout(&only);
            }
            _ => self.info("several branches here — right-click to pick one"),
        }
    }

    fn stash_and_switch(&mut self, target: &str) {
        match git::stash_push().and_then(|_| git::checkout_branch(target)) {
            Ok(()) => {
                self.info(format!("stashed and switched to {target}"));
                self.refresh();
            }
            Err(e) => self.error(format!("stash & switch failed: {}", first_line(&e))),
        }
    }

    fn create_branch(&mut self, name: &str, sha: &str) {
        match git::create_branch_at(name, sha) {
            Ok(()) => {
                self.info(format!("created and switched to {name}"));
                self.refresh();
            }
            Err(e) => self.error(format!("create failed: {}", first_line(&e))),
        }
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
            Ok(args) => self.spawn_job("push", args),
            Err(e) => self.error(format!("push failed: {}", first_line(&e))),
        }
    }

    fn pull(&mut self) {
        self.spawn_job("pull", vec!["pull".into(), "--ff-only".into()]);
    }

    /// Run a network git command in the background. There's no terminal to
    /// prompt in, so credentials must come from a helper (the macOS keychain,
    /// ssh-agent); interactive prompts are disabled so git fails instead of
    /// hanging forever.
    fn spawn_job(&mut self, label: &str, args: Vec<String>) {
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
        self.info(format!("{label}ing…"));
        self.job = Some(Job {
            label: label.to_string(),
            rx,
        });
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        match job.rx.try_recv() {
            Ok(result) => {
                let label = job.label.clone();
                self.job = None;
                match result {
                    Ok(()) => {
                        self.info(format!("{label} done"));
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
            return self.info("detached HEAD — switch to a branch first");
        };
        let Some(info) = remote::detect() else {
            return self.error("couldn't detect remote host");
        };
        let base = git::default_base_branch().unwrap_or_else(|| "main".into());
        if base == head {
            return self.info(format!(
                "you're on {base} — switch to a feature branch first"
            ));
        }
        if repo.tracking.upstream.is_none() {
            return self.info("no upstream — push first so the branch exists on the remote");
        }
        if let Some(pr) = repo.prs.get(&head) {
            let url = format!("{}/pull/{}", info.web_url, pr.number);
            ctx.open_url(egui::OpenUrl::new_tab(url));
            return self.info(format!("opened PR #{}", pr.number));
        }
        let url = remote::compare_url(&info, &base, &head);
        ctx.open_url(egui::OpenUrl::new_tab(&url));
        self.info(format!("opened {url}"));
    }

    // ── Keyboard ─────────────────────────────────────────────────────────────

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.modal.is_some() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let Some(repo) = &self.repo else { return };
        let (rows, files) = (repo.rows.len(), repo.status.len());
        let tab = std::mem::take(&mut self.tab_pressed);
        let (down, up, enter, space) = ctx.input(|i| {
            (
                i.key_pressed(Key::J) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::K) || i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Space),
            )
        });
        if tab && files > 0 {
            self.focus = match self.focus {
                Focus::Graph => Focus::Files,
                Focus::Files => Focus::Graph,
            };
            self.update_diff();
        }
        match self.focus {
            Focus::Graph => {
                if down && self.graph_sel + 1 < rows {
                    self.select_commit(self.graph_sel + 1);
                    self.scroll_to_sel = true;
                }
                if up && self.graph_sel > 0 {
                    self.select_commit(self.graph_sel - 1);
                    self.scroll_to_sel = true;
                }
                if enter {
                    self.checkout_selected_commit();
                }
            }
            Focus::Files => {
                if down && self.file_sel + 1 < files {
                    self.select_file(self.file_sel + 1);
                }
                if up && self.file_sel > 0 {
                    self.select_file(self.file_sel - 1);
                }
                if space {
                    if let Some(e) = self.repo.as_ref().and_then(|r| r.status.get(self.file_sel)) {
                        let e = e.clone();
                        self.toggle_stage(&e);
                    }
                }
            }
        }
    }

    // ── Views ────────────────────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .button("📂 Open…")
                .on_hover_text("Open a repository (⌘O)")
                .clicked()
            {
                self.pick_repo();
            }
            let Some(repo) = &self.repo else { return };
            ui.separator();
            ui.label(RichText::new(&repo.name).strong())
                .on_hover_text(repo.root.display().to_string());
            let t = &repo.tracking;
            let branch = match (&t.branch, t.detached) {
                (Some(b), _) => b.clone(),
                (None, true) => "detached HEAD".into(),
                (None, false) => "—".into(),
            };
            ui.label(RichText::new(format!("▶ {branch}")).color(HEAD_GOLD));
            if t.ahead > 0 {
                ui.label(RichText::new(format!("↑{}", t.ahead)).color(GREEN));
            }
            if t.behind > 0 {
                ui.label(RichText::new(format!("↓{}", t.behind)).color(YELLOW));
            }
            ui.separator();

            let busy = self.job.is_some();
            let ctx = ui.ctx().clone();
            if ui.button("⟳ Refresh").on_hover_text("⌘R").clicked() {
                self.refresh();
                self.needs_pr_fetch = true;
                self.info("refreshed");
            }
            if ui
                .add_enabled(!busy, egui::Button::new("⬇ Pull"))
                .on_hover_text("git pull --ff-only")
                .clicked()
            {
                self.pull();
            }
            if ui.add_enabled(!busy, egui::Button::new("⬆ Push")).clicked() {
                self.request_push();
            }
            self.branch_menu(ui);
            if ui
                .button("⤴ Pull Request")
                .on_hover_text("Open the PR for this branch in your browser, or start one")
                .clicked()
            {
                self.open_pull_request(&ctx);
            }
            if busy {
                ui.spinner();
            }
        });
    }

    fn branch_menu(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        ui.menu_button("🔀 Branches", |ui| {
            ui.set_min_width(280.0);
            let filter = ui.add(
                egui::TextEdit::singleline(&mut self.branch_filter)
                    .hint_text("filter…")
                    .desired_width(f32::INFINITY),
            );
            if !filter.has_focus() && !filter.lost_focus() {
                filter.request_focus();
            }
            let q = self.branch_filter.to_lowercase();
            let Some(repo) = &self.repo else { return };
            let matches: Vec<&Branch> = repo
                .branches
                .iter()
                .filter(|b| q.is_empty() || b.name.to_lowercase().contains(&q))
                .collect();
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                chosen = matches.first().map(|b| b.name.clone());
            }
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for b in matches {
                        let color = if b.is_current {
                            HEAD_GOLD
                        } else {
                            graph_view::rgb(spor::color::color_for(
                                spor::color::branch_family(&b.name, &repo.remotes),
                                &b.name,
                            ))
                        };
                        let label = if b.is_current {
                            format!("▶ {}", b.name)
                        } else {
                            b.name.clone()
                        };
                        let text = RichText::new(label).color(color);
                        let text = if b.is_remote { text.italics() } else { text };
                        if ui.selectable_label(b.is_current, text).clicked() {
                            chosen = Some(b.name.clone());
                        }
                    }
                });
        });
        if let Some(name) = chosen {
            self.branch_filter.clear();
            ui.close();
            self.checkout(&name);
        }
    }

    fn graph_panel(&mut self, ui: &mut egui::Ui) {
        let Some(repo) = &self.repo else { return };
        let meta = RepoMeta {
            remotes: &repo.remotes,
            prs: &repo.prs,
        };
        let text_color = ui.visuals().text_color();
        let sel_fill = if self.focus == Focus::Graph {
            ui.visuals().selection.bg_fill.gamma_multiply(0.55)
        } else {
            ui.visuals().selection.bg_fill.gamma_multiply(0.25)
        };
        let hover_fill = ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.35);

        let mut clicked = None;
        let mut double_clicked = false;
        let mut action: Option<RowAction> = None;
        let scroll_to = std::mem::take(&mut self.scroll_to_sel).then_some(self.graph_sel);

        let mut area = egui::ScrollArea::vertical().auto_shrink(false);
        if let Some(idx) = scroll_to {
            // Keep the keyboard selection in view with a row of margin.
            let viewport = ui.available_height();
            let y = idx as f32 * ROW_HEIGHT;
            let offset = ui
                .ctx()
                .data(|d| d.get_temp::<f32>(egui::Id::new("graph_offset")))
                .unwrap_or(0.0);
            if y < offset {
                area = area.vertical_scroll_offset(y);
            } else if y + ROW_HEIGHT * 2.0 > offset + viewport {
                area = area.vertical_scroll_offset(y + ROW_HEIGHT * 2.0 - viewport);
            }
        }
        let output = area.show_rows(ui, ROW_HEIGHT, repo.rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for idx in range {
                let row = &repo.rows[idx];
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), ROW_HEIGHT),
                    Sense::click(),
                );
                let painter = ui.painter_at(rect);
                if idx == self.graph_sel {
                    painter.rect_filled(rect, 0.0, sel_fill);
                } else if resp.hovered() {
                    painter.rect_filled(rect, 0.0, hover_fill);
                }
                graph_view::paint_row(&painter, rect, row, &meta, text_color);

                if resp.clicked() {
                    clicked = Some(idx);
                }
                if resp.double_clicked() {
                    double_clicked = true;
                }
                resp.context_menu(|ui| {
                    if clicked.is_none() {
                        clicked = Some(idx);
                    }
                    ui.label(
                        RichText::new(format!("{}  {}", row.commit.short, row.commit.subject))
                            .color(DIM),
                    );
                    ui.separator();
                    for r in row.commit.refs.iter().filter(|r| !r.starts_with("tag:")) {
                        if ui.button(format!("Checkout {r}")).clicked() {
                            action = Some(RowAction::Checkout(r.clone()));
                            ui.close();
                        }
                    }
                    if ui.button("New branch here…").clicked() {
                        action = Some(RowAction::NewBranch(
                            row.commit.hash.clone(),
                            row.commit.short.clone(),
                        ));
                        ui.close();
                    }
                    if ui.button("Copy SHA").clicked() {
                        ui.ctx().copy_text(row.commit.hash.clone());
                        ui.close();
                    }
                });
            }
        });
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("graph_offset"), output.state.offset.y));

        if let Some(idx) = clicked {
            if idx != self.graph_sel || self.focus != Focus::Graph {
                self.select_commit(idx);
            }
        }
        if double_clicked {
            self.checkout_selected_commit();
        }
        match action {
            Some(RowAction::Checkout(name)) => self.checkout(&name),
            Some(RowAction::NewBranch(sha, short)) => {
                self.modal = Some(Modal::NewBranch {
                    name: String::new(),
                    sha,
                    short,
                })
            }
            None => {}
        }
    }

    fn files_panel(&mut self, ui: &mut egui::Ui) {
        let Some(repo) = &self.repo else { return };
        let status = repo.status.clone();
        let staged: Vec<usize> = (0..status.len())
            .filter(|&i| status[i].status.is_staged())
            .collect();
        let unstaged: Vec<usize> = (0..status.len())
            .filter(|&i| !status[i].status.is_staged())
            .collect();

        let mut select = None;
        let mut toggle = None;
        let mut discard = None;
        let mut bulk = None;

        // Fixed-height list so the panel keeps its size (and the commit box
        // its place) however many files there are.
        let list_height = (ui.available_height() - 120.0).max(60.0);
        egui::ScrollArea::vertical()
            .id_salt("files")
            .auto_shrink(false)
            .max_height(list_height)
            .show(ui, |ui| {
                if status.is_empty() {
                    ui.label(RichText::new("Working tree clean").color(DIM));
                }
                for (title, idxs, is_staged) in
                    [("Staged", &staged, true), ("Changes", &unstaged, false)]
                {
                    if idxs.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{title} ({})", idxs.len())).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if is_staged {
                                "Unstage all"
                            } else {
                                "Stage all"
                            };
                            if ui.small_button(label).clicked() {
                                bulk = Some(is_staged);
                            }
                        });
                    });
                    for &i in idxs.iter() {
                        let e = &status[i];
                        let (glyph, color) = status_glyph(&e.status);
                        ui.horizontal(|ui| {
                            let mut checked = is_staged;
                            if ui
                                .checkbox(&mut checked, "")
                                .on_hover_text(if is_staged { "Unstage" } else { "Stage" })
                                .changed()
                            {
                                toggle = Some(i);
                            }
                            ui.label(RichText::new(glyph).monospace().color(color));
                            let selected = self.focus == Focus::Files && self.file_sel == i;
                            let label = match &e.orig_path {
                                Some(orig) => format!("{orig} → {}", e.path),
                                None => e.path.clone(),
                            };
                            let resp = ui.add(egui::Button::selectable(selected, label).truncate());
                            if resp.clicked() {
                                select = Some(i);
                            }
                            if !is_staged {
                                resp.context_menu(|ui| {
                                    let what = match e.status {
                                        FileStatus::Untracked => "Delete file…",
                                        _ => "Discard changes…",
                                    };
                                    if ui.button(what).clicked() {
                                        discard = Some(i);
                                        ui.close();
                                    }
                                });
                            }
                        });
                    }
                    ui.add_space(6.0);
                }
            });

        ui.separator();
        let has_staged = !staged.is_empty();
        let edit = ui.add(
            egui::TextEdit::multiline(&mut self.commit_msg)
                .hint_text("Commit message")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        let cmd_enter =
            edit.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
        let can_commit = has_staged && !self.commit_msg.trim().is_empty();
        let commit_clicked = ui
            .add_enabled(can_commit, egui::Button::new("✔ Commit"))
            .on_hover_text("⌘⏎")
            .on_disabled_hover_text(if has_staged {
                "Write a message first"
            } else {
                "Stage some changes first"
            })
            .clicked();

        if let Some(i) = select {
            self.select_file(i);
        }
        if let Some(i) = toggle {
            self.toggle_stage(&status[i]);
        }
        if let Some(i) = discard {
            self.modal = Some(Modal::Discard {
                entry: status[i].clone(),
            });
        }
        if let Some(s) = bulk {
            self.stage_all(s);
        }
        if can_commit && (commit_clicked || cmd_enter) {
            self.commit();
        }
    }

    fn diff_panel(&self, ui: &mut egui::Ui) {
        if self.diff.is_empty() {
            ui.label(RichText::new("No diff").color(DIM));
            return;
        }
        let lines: Vec<&str> = self.diff.lines().collect();
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
        egui::ScrollArea::both()
            .id_salt("diff")
            .auto_shrink(false)
            .show_rows(ui, row_h, lines.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for line in &lines[range] {
                    let color = diff_color(line, ui.visuals().text_color());
                    ui.label(RichText::new(*line).monospace().color(color));
                }
            });
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.heading(RichText::new("spor").size(40.0));
            ui.label(RichText::new("follow the track of your branches").color(DIM));
            ui.add_space(20.0);
            if ui
                .button(RichText::new("📂 Open Repository…").size(16.0))
                .clicked()
            {
                self.pick_repo();
            }
            ui.add_space(8.0);
            ui.label(RichText::new("or drop a folder onto this window").color(DIM));
            if let Some(e) = &self.open_error {
                ui.add_space(12.0);
                ui.label(RichText::new(e).color(RED));
            }
        });
    }

    fn modals(&mut self, ctx: &egui::Context) {
        let Some(modal) = &mut self.modal else { return };
        let mut close = false;
        let mut run: Option<Deferred> = None;

        let resp = egui::Modal::new(egui::Id::new("spor_modal")).show(ctx, |ui| {
            ui.set_width(380.0);
            match modal {
                Modal::NewBranch { name, sha, short } => {
                    ui.heading("New branch");
                    ui.label(RichText::new(format!("from {short}")).color(DIM));
                    let edit = ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text("feat/my-change")
                            .desired_width(f32::INFINITY),
                    );
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        let ok = !name.trim().is_empty();
                        if (ui.add_enabled(ok, egui::Button::new("Create & switch")).clicked()
                            || enter)
                            && ok
                        {
                            let (n, s) = (name.trim().to_string(), sha.clone());
                            run = Some(Box::new(move |app| app.create_branch(&n, &s)));
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Modal::StashAndSwitch { target } => {
                    ui.heading("Uncommitted changes");
                    ui.label(format!(
                        "Your changes conflict with switching to '{target}'. Stash them and switch?"
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Stash & switch").clicked() {
                            let t = target.clone();
                            run = Some(Box::new(move |app| app.stash_and_switch(&t)));
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Modal::Discard { entry } => {
                    let action = match entry.status {
                        FileStatus::Untracked => "Delete untracked file",
                        FileStatus::Deleted => "Restore deleted file",
                        _ => "Discard changes to",
                    };
                    ui.heading("Discard changes?");
                    ui.label(format!("{action} '{}'? This cannot be undone.", entry.path));
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Discard").color(RED))
                            .clicked()
                        {
                            let e = entry.clone();
                            run = Some(Box::new(move |app| app.discard(&e)));
                        }
                        if ui.button("Keep").clicked() {
                            close = true;
                        }
                    });
                }
                Modal::PushBehind { behind } => {
                    ui.heading("Behind upstream");
                    ui.label(format!(
                        "You're {behind} commit(s) behind. Git will reject a plain push of diverged history."
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Pull first").clicked() {
                            run = Some(Box::new(|app| app.pull()));
                        }
                        if ui.button("Push anyway").clicked() {
                            run = Some(Box::new(|app| app.push()));
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
            }
        });
        if resp.should_close() {
            close = true;
        }
        if let Some(f) = run {
            self.modal = None;
            f(self);
        } else if close {
            self.modal = None;
        }
    }
}

/// An action chosen inside a modal, run once the modal's borrow ends.
type Deferred = Box<dyn FnOnce(&mut SporApp)>;

enum RowAction {
    Checkout(String),
    NewBranch(String, String),
}

fn status_glyph(s: &FileStatus) -> (&'static str, Color32) {
    match s {
        FileStatus::Staged => ("+", GREEN),
        FileStatus::StagedDeleted => ("−", GREEN),
        FileStatus::Modified => ("~", YELLOW),
        FileStatus::Deleted => ("−", RED),
        FileStatus::Untracked => ("?", DIM),
    }
}

fn diff_color(line: &str, normal: Color32) -> Color32 {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") {
        DIM
    } else if line.starts_with('+') {
        GREEN
    } else if line.starts_with('-') {
        RED
    } else if line.starts_with("@@") {
        CYAN
    } else if line.starts_with("commit ") {
        YELLOW
    } else {
        normal
    }
}

/// ssh must never wait on a terminal prompt we can't show; fail fast instead
/// and let the error surface in the status bar. Respects a user-set command.
fn ssh_command() -> String {
    std::env::var("GIT_SSH_COMMAND").unwrap_or_else(|_| "ssh -o BatchMode=yes".into())
}

impl eframe::App for SporApp {
    /// egui uses Tab to walk keyboard focus through widgets. Here Tab means
    /// "graph ↔ files", and a focused toolbar button would then swallow Space
    /// and Enter — so take Tab out of the input unless a text field (or a
    /// modal) is using it.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.modal.is_some() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        raw_input.events.retain(|e| match e {
            egui::Event::Key {
                key: Key::Tab,
                pressed,
                modifiers,
                ..
            } if modifiers.is_none() => {
                self.tab_pressed |= *pressed;
                false
            }
            _ => true,
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Folders dropped on the window open as repos.
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.open_repo(&path);
        }
        let (cmd_o, cmd_r) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(Key::O),
                i.modifiers.command && i.key_pressed(Key::R),
            )
        });
        if cmd_o {
            self.pick_repo();
        }

        self.poll_job(&ctx);
        if let Some(repo) = &mut self.repo {
            if std::mem::take(&mut self.needs_pr_fetch) {
                repo.spawn_pr_fetch(&ctx);
            }
            repo.poll_pr_fetch();
        }
        if cmd_r && self.repo.is_some() {
            self.refresh();
            self.needs_pr_fetch = true;
            self.info("refreshed");
        } else {
            self.handle_keys(&ctx);
        }

        let title = match &self.repo {
            Some(r) => format!("spor — {}", r.name),
            None => "spor".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            self.toolbar(ui);
            ui.add_space(2.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            let color = if self.message_is_error { RED } else { DIM };
            ui.label(RichText::new(&self.message).color(color));
        });

        if self.repo.is_none() {
            egui::CentralPanel::default().show(ui, |ui| self.welcome(ui));
            return;
        }

        egui::Panel::right("side")
            .resizable(true)
            .default_size(520.0)
            .min_size(300.0)
            .show(ui, |ui| {
                egui::Panel::top("files")
                    .resizable(true)
                    .default_size(300.0)
                    .min_size(160.0)
                    .show(ui, |ui| {
                        // Claim the whole panel: it remembers its content's
                        // height, so anything less shrinks it every frame.
                        ui.set_min_height(ui.available_height());
                        ui.add_space(4.0);
                        self.files_panel(ui);
                    });
                egui::CentralPanel::default().show(ui, |ui| self.diff_panel(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(0.0))
            .show(ui, |ui| self.graph_panel(ui));

        self.modals(&ctx);
    }
}
