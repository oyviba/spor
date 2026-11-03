//! Git actions beyond staging and committing: cherry-pick, revert, rebase
//! (plain and interactive), reset, conflicts, line staging, branch and stash
//! management, tags, and commit search. Most can be undone with ⌘Z; the few
//! that can't ask first.

use super::app::*;
use super::assets::icon;
use super::support::{first_line, plural};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::{DiffFile, DiffSide},
    h_flex,
    input::{Input, Textarea},
    menu::{DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, WindowExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, AppContext as _, Context, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use spor::git::{
    self, ConflictSide, DiffKind, LineAction, Operation, Outcome, RebaseStep, ResetMode,
};
use spor::patch::{LinePos, Side};

/// Which uncommitted changes the Changes tab shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangesMode {
    /// Everything against the last commit.
    All,
    /// What isn't staged yet; lines can be staged or discarded.
    Unstaged,
    /// What the next commit takes; lines can be unstaged.
    Staged,
}

/// What a step of an interactive rebase does with its commit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepAction {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl StepAction {
    const ALL: [StepAction; 5] = [
        StepAction::Pick,
        StepAction::Reword,
        StepAction::Squash,
        StepAction::Fixup,
        StepAction::Drop,
    ];

    fn label(self) -> &'static str {
        match self {
            StepAction::Pick => "Pick",
            StepAction::Reword => "Reword",
            StepAction::Squash => "Squash",
            StepAction::Fixup => "Fixup",
            StepAction::Drop => "Drop",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            StepAction::Pick => "Keep as it is",
            StepAction::Reword => "Keep, with a new message",
            StepAction::Squash => "Fold into the commit above, keeping both messages",
            StepAction::Fixup => "Fold into the commit above, dropping this message",
            StepAction::Drop => "Leave it out",
        }
    }
}

pub struct DraftStep {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub action: StepAction,
    /// The new message, for Reword.
    pub message: String,
}

/// An interactive rebase being planned in its own tab.
pub struct RebaseDraft {
    /// The commit the rewritten ones go on top of.
    pub base: String,
    pub base_label: String,
    pub branch: String,
    /// Oldest first, as git replays them.
    pub steps: Vec<DraftStep>,
    pub selected: usize,
}

impl RebaseDraft {
    fn plan(&self) -> Result<Vec<RebaseStep>, String> {
        let mut plan = Vec::new();
        let mut kept = 0;
        for step in &self.steps {
            let h = step.hash.clone();
            plan.push(match step.action {
                StepAction::Pick => RebaseStep::Pick(h),
                StepAction::Reword => {
                    if step.message.trim().is_empty() {
                        return Err(format!("Write a message for {}", step.short));
                    }
                    RebaseStep::Reword(h, step.message.trim().to_string())
                }
                StepAction::Squash | StepAction::Fixup if kept == 0 => {
                    return Err(format!(
                        "{} has no commit above it to fold into",
                        step.short
                    ))
                }
                StepAction::Squash => RebaseStep::Squash(h),
                StepAction::Fixup => RebaseStep::Fixup(h),
                StepAction::Drop => RebaseStep::Drop(h),
            });
            if step.action != StepAction::Drop {
                kept += 1;
            }
        }
        Ok(plan)
    }

    fn changed(&self, original: &[String]) -> bool {
        self.steps.iter().map(|s| &s.hash).ne(original.iter())
            || self.steps.iter().any(|s| s.action != StepAction::Pick)
    }
}

impl Spor {
    // ── Commits: cherry-pick, revert, reset ──────────────────────────────

    /// The commit selected in History, as a full hash.
    pub(crate) fn selected_sha(&self) -> Option<String> {
        let repo = self.repo.as_ref()?;
        Some(repo.rows.get(self.selected?)?.commit.hash.clone())
    }

    /// The checked-out branch and its commit, to take a rewrite back to.
    fn rewrite_start(&self) -> Option<(String, String)> {
        let repo = self.repo.as_ref()?;
        let branch = repo.tracking.branch.clone()?;
        Some((branch, self.git().head_sha().ok()?))
    }

    /// Report how a history rewrite went: done (undoable), stopped on
    /// conflicts (the banner takes it from there), or failed.
    fn finish_rewrite(
        &mut self,
        done: String,
        start: Option<(String, String)>,
        result: Result<Outcome, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Outcome::Done) => match start {
                Some((branch, before)) => self.did(
                    done,
                    Undo::Reset {
                        branch,
                        before,
                        mode: ResetMode::Keep,
                        label: "undone".into(),
                    },
                    window,
                    cx,
                ),
                None => self.ok(done, window, cx),
            },
            Ok(Outcome::Conflicts(files)) => {
                self.pending_rewrite = start.map(|s| (done, s));
                self.tab = Tab::Changes;
                self.dock = Some(DockPanel::Git);
                window.push_notification(
                    Notification::warning(format!(
                        "Stopped on conflicts in {} — resolve them, then Continue",
                        plural(files.len(), "file")
                    )),
                    cx,
                );
            }
            Err(e) => self.error(first_line_of_error(&e), window, cx),
        }
        self.reload(window, cx);
    }

    fn require_clean(&self, what: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let dirty = self
            .repo
            .as_ref()
            .is_some_and(|r| r.changes.iter().any(|c| !c.untracked));
        if dirty {
            self.info(
                format!("Commit or stash your changes before {what}"),
                window,
                cx,
            );
        }
        !dirty
    }

    pub(crate) fn cherry_pick(&mut self, sha: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.git().is_ancestor(sha, "HEAD") {
            let here = self
                .repo
                .as_ref()
                .and_then(|r| r.tracking.branch.clone())
                .unwrap_or_else(|| "HEAD".into());
            return self.info(format!("{} is already on {here}", short(sha)), window, cx);
        }
        let start = self.rewrite_start();
        let short = short(sha);
        let result = self.git().cherry_pick(sha);
        self.finish_rewrite(format!("Cherry-picked {short}"), start, result, window, cx);
    }

    pub(crate) fn revert(&mut self, sha: &str, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.rewrite_start();
        let short = short(sha);
        let result = self.git().revert(sha);
        self.finish_rewrite(format!("Reverted {short}"), start, result, window, cx);
    }

    /// Replay the checked-out branch on top of `onto`.
    pub(crate) fn rebase_onto(&mut self, onto: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.require_clean("rebasing", window, cx) {
            return;
        }
        let Some(start) = self.rewrite_start() else {
            return self.info("Check out a branch to rebase", window, cx);
        };
        let branch = start.0.clone();
        let result = self.git().rebase(onto);
        self.finish_rewrite(
            format!("Rebased {branch} onto {onto}"),
            Some(start),
            result,
            window,
            cx,
        );
    }

    /// Move the checked-out branch to `sha`. A hard reset throws work
    /// away, so it asks first.
    pub(crate) fn reset_to(
        &mut self,
        sha: &str,
        mode: ResetMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((branch, before)) = self.rewrite_start() else {
            return self.info("Check out a branch to reset it", window, cx);
        };
        let sha = sha.to_string();
        if mode == ResetMode::Hard {
            let dirty = self.repo.as_ref().map_or(0, |r| r.changes.len());
            let spor = cx.entity().downgrade();
            let target = short(&sha);
            window.open_dialog(cx, move |dialog, _, cx| {
                let spor = spor.clone();
                let (sha, branch, before) = (sha.clone(), branch.clone(), before.clone());
                dialog
                    .title(format!("Reset {branch} to {target}?"))
                    .w(px(440.))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if dirty > 0 {
                                format!(
                                    "Your uncommitted changes to {} are thrown away and can't be \
                                 brought back. Undo moves {branch} back, nothing more.",
                                    plural(dirty, "file")
                                )
                            } else {
                                format!("Undo moves {branch} back to where it is now.")
                            }),
                    )
                    .footer(dialog_buttons("reset-hard", "Reset"))
                    .on_ok(move |_, window, cx| {
                        let (sha, branch, before) = (sha.clone(), branch.clone(), before.clone());
                        let _ = spor.update(cx, |this, cx| {
                            this.do_reset(&sha, mode, branch, before, window, cx)
                        });
                        true
                    })
            });
            return;
        }
        self.do_reset(&sha, mode, branch, before, window, cx);
    }

    fn do_reset(
        &mut self,
        sha: &str,
        mode: ResetMode,
        branch: String,
        before: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.git().reset(sha, mode) {
            Ok(()) => {
                // Hard can't put discarded files back; Keep at least doesn't
                // clobber anything changed since.
                let undo_mode = if mode == ResetMode::Hard {
                    ResetMode::Keep
                } else {
                    mode
                };
                self.did(
                    format!("Reset {branch} to {}", short(sha)),
                    Undo::Reset {
                        branch,
                        before,
                        mode: undo_mode,
                        label: "Reset undone".into(),
                    },
                    window,
                    cx,
                );
            }
            Err(e) => self.error(format!("Couldn't reset: {}", first_line(&e)), window, cx),
        }
        self.reload(window, cx);
    }

    pub(crate) fn checkout_commit(
        &mut self,
        sha: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.git().checkout_detached(sha) {
            Ok(()) => self.ok(
                format!("Checked out {} (no branch)", short(sha)),
                window,
                cx,
            ),
            Err(e) if git::is_worktree_conflict(&e) => self.info(
                "Commit or stash your changes before checking out another commit",
                window,
                cx,
            ),
            Err(e) => self.error(
                format!("Couldn't check out: {}", first_line(&e)),
                window,
                cx,
            ),
        }
        self.reload(window, cx);
    }

    // ── Operations in progress ───────────────────────────────────────────

    pub(crate) fn continue_operation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(op) = self.git().operation() else {
            return self.reload(window, cx);
        };
        match self.git().continue_operation() {
            Ok(Outcome::Done) => {
                let label = op.label();
                match self.pending_rewrite.take() {
                    Some((done, (branch, before))) => self.did(
                        done,
                        Undo::Reset {
                            branch,
                            before,
                            mode: ResetMode::Keep,
                            label: "undone".into(),
                        },
                        window,
                        cx,
                    ),
                    None => self.ok(format!("{label} finished"), window, cx),
                }
            }
            Ok(Outcome::Conflicts(files)) => self.info(
                format!(
                    "{} still {} conflicts",
                    plural(files.len(), "file"),
                    if files.len() == 1 { "has" } else { "have" }
                ),
                window,
                cx,
            ),
            Err(e) => self.error(first_line_of_error(&e), window, cx),
        }
        self.reload(window, cx);
    }

    pub(crate) fn skip_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.git().skip_commit() {
            Ok(Outcome::Done) => self.ok("Skipped — all done", window, cx),
            Ok(Outcome::Conflicts(_)) => {
                self.info("Skipped — the next commit conflicts too", window, cx)
            }
            Err(e) => self.error(first_line_of_error(&e), window, cx),
        }
        self.reload(window, cx);
    }

    pub(crate) fn abort_operation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.git().operation().map_or("Operation", Operation::label);
        match self.git().abort_operation() {
            Ok(()) => {
                self.pending_rewrite = None;
                self.conflict_path = None;
                self.info(
                    format!("{label} aborted — everything is as it was"),
                    window,
                    cx,
                )
            }
            Err(e) => self.error(format!("Couldn't abort: {}", first_line(&e)), window, cx),
        }
        self.reload(window, cx);
    }

    /// Resolve a whole file by taking one side.
    pub(crate) fn resolve_file(
        &mut self,
        path: &str,
        side: ConflictSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.git().resolve_with(path, side) {
            Ok(()) => {
                if self.conflict_path.as_deref() == Some(path) {
                    self.conflict_path = None;
                }
                let which = match side {
                    ConflictSide::Ours => "current",
                    ConflictSide::Theirs => "incoming",
                };
                self.ok(
                    format!("Resolved {} with the {which} side", file_name(path)),
                    window,
                    cx,
                );
            }
            Err(e) => self.error(format!("Couldn't resolve: {}", first_line(&e)), window, cx),
        }
        self.reload(window, cx);
    }

    pub(crate) fn mark_resolved(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = std::fs::read_to_string(self.git().path(path)).unwrap_or_default();
        if text
            .lines()
            .any(|l| l.starts_with("<<<<<<<") || l.starts_with(">>>>>>>"))
        {
            return self.info(
                format!("{} still has conflict markers", file_name(path)),
                window,
                cx,
            );
        }
        match self.git().mark_resolved(path) {
            Ok(()) => self.ok(format!("Marked {} resolved", file_name(path)), window, cx),
            Err(e) => self.error(
                format!("Couldn't mark resolved: {}", first_line(&e)),
                window,
                cx,
            ),
        }
        self.reload(window, cx);
    }

    /// Show a conflicted file with its conflicts, to pick a side for each.
    pub(crate) fn open_conflict(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match std::fs::read_to_string(self.git().path(path)) {
            Ok(text) => text,
            Err(_) => {
                // Deleted on one side: there's nothing to merge line by line.
                return self.info(
                    format!("{path} was deleted on one side — use the current or incoming version"),
                    window,
                    cx,
                );
            }
        };
        match DiffFile::parse_conflicts(SharedString::from(path.to_string()), &text) {
            Ok(file) if !file.conflicts().is_empty() => {
                self.conflict_diff
                    .update(cx, |state, cx| state.set_files([file], cx));
                self.conflict_path = Some(path.to_string());
                self.tab = Tab::Changes;
                cx.notify();
            }
            Ok(_) => self.info(
                format!(
                    "{} has no conflict markers left — mark it resolved",
                    file_name(path)
                ),
                window,
                cx,
            ),
            Err(e) => self.error(
                format!("Couldn't read the conflicts: {}", e.message()),
                window,
                cx,
            ),
        }
    }

    /// Write the conflict view's choices to the file and mark it resolved.
    pub(crate) fn save_resolution(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.conflict_path.clone() else {
            return;
        };
        let Some(text) = self.conflict_diff.read(cx).resolved_text(&path) else {
            return self.info("Pick a side for every conflict first", window, cx);
        };
        let full = self.git().path(&path);
        let before = std::fs::read(&full).ok();
        if let Err(e) = std::fs::write(&full, text) {
            return self.error(format!("Couldn't save {path}: {e}"), window, cx);
        }
        match self.git().mark_resolved(&path) {
            Ok(()) => {
                self.conflict_path = None;
                self.undo.push((
                    format!("resolve {path}"),
                    Undo::Restore(vec![(full, before)]),
                ));
                self.ok(format!("Resolved {}", file_name(&path)), window, cx);
            }
            Err(e) => self.error(
                format!("Couldn't mark resolved: {}", first_line(&e)),
                window,
                cx,
            ),
        }
        self.reload(window, cx);
    }

    /// The bar over the center while a merge, rebase, cherry-pick or revert
    /// is stopped: what's going on, and the ways out.
    pub(crate) fn render_operation_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let repo = self.repo.as_ref()?;
        let op = repo.operation?;
        let theme = cx.theme().clone();
        let conflicts = repo.conflicted().len();
        let text = match (op, conflicts) {
            (Operation::Rebase, 0) => "Rebase paused — continue when you're ready".to_string(),
            (_, 0) => format!(
                "{} ready to finish — every conflict is resolved",
                op.label()
            ),
            (_, n) => format!(
                "{} stopped on conflicts in {}",
                op.label(),
                plural(n, "file")
            ),
        };
        let can_skip = matches!(
            op,
            Operation::Rebase | Operation::CherryPick | Operation::Revert
        );
        Some(
            h_flex()
                .flex_none()
                .h(px(36.))
                .px_3()
                .gap_2()
                .bg(theme.warning.opacity(0.12))
                .border_b_1()
                .border_color(theme.warning.opacity(0.4))
                .child(
                    Icon::empty()
                        .path(icon::GIT_MERGE)
                        .small()
                        .text_color(theme.warning),
                )
                .child(div().flex_none().text_sm().child(text))
                .child(div().w(px(8.)))
                .when(can_skip, |this| {
                    this.child(
                        Button::new("op-skip")
                            .ghost()
                            .xsmall()
                            .label("Skip Commit")
                            .tooltip("Leave out the commit it stopped on")
                            .on_click(cx.listener(|this, _, w, cx| this.skip_commit(w, cx))),
                    )
                })
                .child(
                    Button::new("op-abort")
                        .ghost()
                        .xsmall()
                        .label("Abort")
                        .tooltip("Put everything back as it was before")
                        .on_click(cx.listener(|this, _, w, cx| this.abort_operation(w, cx))),
                )
                .child(
                    Button::new("op-continue")
                        .primary()
                        .xsmall()
                        .label("Continue")
                        .disabled(conflicts > 0)
                        .tooltip(if conflicts > 0 {
                            "Resolve every conflict first"
                        } else {
                            "Commit the result and carry on"
                        })
                        .on_click(cx.listener(|this, _, w, cx| this.continue_operation(w, cx))),
                )
                // Buttons sit by the text: notifications pop up over the
                // right-hand end.
                .child(div().flex_1())
                .into_any_element(),
        )
    }

    // ── Lines ────────────────────────────────────────────────────────────

    pub(crate) fn set_changes_mode(&mut self, mode: ChangesMode, cx: &mut Context<Self>) {
        self.changes_mode = mode;
        self.conflict_path = None;
        self.load_changes_diff(cx);
        cx.notify();
    }

    /// Stage, unstage or discard the lines selected in the Changes tab.
    pub(crate) fn apply_selected_lines(
        &mut self,
        action: LineAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = match self.changes_mode {
            ChangesMode::Unstaged => DiffKind::Unstaged,
            ChangesMode::Staged => DiffKind::Staged,
            ChangesMode::All => return,
        };
        let current = self.changes_diff.read(cx).selected_lines();
        let Some(range) = current.or_else(|| self.line_selection.take()) else {
            return self.info("Select lines in the diff first", window, cx);
        };
        let side = |s: DiffSide| match s {
            DiffSide::Original => Side::Old,
            DiffSide::Modified => Side::New,
        };
        let from = LinePos::new(side(range.side()), range.start());
        let to = LinePos::new(side(range.end_side()), range.end());
        let path = range.path().to_string();
        match self.git().apply_lines(&path, kind, action, from, to) {
            Ok(applied) => {
                let what = match action {
                    LineAction::Stage => "Staged",
                    LineAction::Unstage => "Unstaged",
                    LineAction::Discard => "Discarded",
                };
                let text = format!("{what} lines in {}", file_name(&path));
                if action == LineAction::Discard {
                    self.did(text, Undo::Patch(applied), window, cx);
                } else {
                    self.undo.push((text, Undo::Patch(applied)));
                }
                self.changes_diff
                    .update(cx, |state, cx| state.set_selected_lines(None, cx));
            }
            Err(e) => self.error(first_line(&e).to_string(), window, cx),
        }
        self.reload(window, cx);
    }

    // ── Branches ─────────────────────────────────────────────────────────

    /// Merge `source` into the checked-out branch.
    pub(crate) fn merge_into_current(
        &mut self,
        source: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.repo.as_ref().and_then(|r| r.tracking.branch.clone()) else {
            return self.info("Check out a branch to merge into", window, cx);
        };
        self.merge_branch(source, &current, window, cx);
    }

    /// Delete a local branch — immediately, but undoably. One with commits
    /// nowhere else asks first.
    pub(crate) fn delete_branch(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.git().delete_branch(name, false) {
            Ok(sha) => self.branch_deleted(name, sha, window, cx),
            Err(e) if e.contains("not fully merged") => {
                let spor = cx.entity().downgrade();
                let name = name.to_string();
                window.open_dialog(cx, move |dialog, _, cx| {
                    let spor = spor.clone();
                    let name = name.clone();
                    dialog
                        .title(format!("Delete {name}?"))
                        .w(px(420.))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                "It has commits that aren't on any other branch. Undo brings it \
                             back while Spor is open.",
                            ),
                        )
                        .footer(dialog_buttons("delete-branch", "Delete Branch"))
                        .on_ok(move |_, window, cx| {
                            let name = name.clone();
                            let _ = spor.update(cx, |this, cx| {
                                match this.git().delete_branch(&name, true) {
                                    Ok(sha) => this.branch_deleted(&name, sha, window, cx),
                                    Err(e) => this.error(
                                        format!("Couldn't delete: {}", first_line(&e)),
                                        window,
                                        cx,
                                    ),
                                }
                            });
                            true
                        })
                });
            }
            Err(e) => self.error(format!("Couldn't delete: {}", first_line(&e)), window, cx),
        }
    }

    fn branch_deleted(
        &mut self,
        name: &str,
        sha: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.did(
            format!("Deleted branch {name}"),
            Undo::Branch {
                name: name.to_string(),
                sha,
            },
            window,
            cx,
        );
        self.reload(window, cx);
    }

    /// Delete `remote/branch` on the remote, after confirming.
    pub(crate) fn delete_remote_branch(
        &mut self,
        full: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = &self.repo else { return };
        let Some((remote, branch)) = full.split_once('/') else {
            return;
        };
        let (remote, branch, root) = (remote.to_string(), branch.to_string(), repo.root.clone());
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let spor = spor.clone();
            let (remote, branch, root) = (remote.clone(), branch.clone(), root.clone());
            dialog
                .title(format!("Delete {branch} from {remote}?"))
                .w(px(420.))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "The branch is deleted on {remote} for everyone. This can't be undone \
                     from here; a local {branch} stays."
                        )),
                )
                .footer(dialog_buttons("delete-remote-branch", "Delete"))
                .on_ok(move |_, window, cx| {
                    let (remote, branch, root) = (remote.clone(), branch.clone(), root.clone());
                    let _ = spor.update(cx, |this, cx| {
                        this.run_job("Delete Branch", window, cx, move || {
                            super::support::git_network(
                                &root,
                                &["push", &remote, "--delete", &branch],
                            )
                            .map(|_| format!("Deleted {branch} from {remote}"))
                        })
                    });
                    true
                })
        });
    }

    pub(crate) fn rename_branch(&mut self, old: &str, window: &mut Window, cx: &mut Context<Self>) {
        let old = old.to_string();
        let input = self.branch_name.clone();
        input.update(cx, |state, cx| state.set_value(old.clone(), window, cx));
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let old = old.clone();
            let input_for_ok = input.clone();
            input.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .title(format!("Rename {old}"))
                .w(px(420.))
                .child(Input::new(&input))
                .footer(dialog_buttons("rename-branch", "Rename"))
                .on_ok(move |_, window, cx| {
                    let new = input_for_ok.read(cx).value().trim().to_string();
                    if new.is_empty() || new == old {
                        return new == old;
                    }
                    let old = old.clone();
                    let _ = spor.update(cx, |this, cx| {
                        if !this.git().valid_branch_name(&new) {
                            return this.info(
                                format!("“{new}” isn't a valid branch name"),
                                window,
                                cx,
                            );
                        }
                        match this.git().rename_branch(&old, &new) {
                            Ok(()) => {
                                this.did(
                                    format!("Renamed {old} to {new}"),
                                    Undo::Rename { old, new },
                                    window,
                                    cx,
                                );
                                this.reload(window, cx);
                            }
                            Err(e) => this.error(
                                format!("Couldn't rename: {}", first_line(&e)),
                                window,
                                cx,
                            ),
                        }
                    });
                    true
                })
        });
    }

    // ── Tags ─────────────────────────────────────────────────────────────

    /// Ask for a name (and an optional message) and tag `sha`.
    pub(crate) fn new_tag(&mut self, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.branch_name.clone();
        let note = self.note.clone();
        name.update(cx, |state, cx| state.set_value("", window, cx));
        note.update(cx, |state, cx| state.set_value("", window, cx));
        let target = short(&sha);
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let sha = sha.clone();
            let (name_ok, note_ok) = (name.clone(), note.clone());
            name.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .title(format!("New Tag at {target}"))
                .w(px(440.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(Input::new(&name))
                        .child(Input::new(&note))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    "With a message the tag is annotated, as releases usually are.",
                                ),
                        ),
                )
                .footer(dialog_buttons("create-tag", "Create Tag"))
                .on_ok(move |_, window, cx| {
                    let tag = name_ok.read(cx).value().trim().to_string();
                    let message = note_ok.read(cx).value().trim().to_string();
                    if tag.is_empty() || tag.contains(char::is_whitespace) {
                        return false;
                    }
                    let sha = sha.clone();
                    let _ = spor.update(cx, |this, cx| {
                        let message = (!message.is_empty()).then_some(message.as_str());
                        match this.git().create_tag(&tag, &sha, message) {
                            Ok(()) => {
                                this.did(
                                    format!("Tagged {} as {tag}", short(&sha)),
                                    Undo::NewTag(tag),
                                    window,
                                    cx,
                                );
                                this.reload(window, cx);
                            }
                            Err(e) => {
                                this.error(format!("Couldn't tag: {}", first_line(&e)), window, cx)
                            }
                        }
                    });
                    true
                })
        });
    }

    // ── Stashes ──────────────────────────────────────────────────────────

    /// Ask for a message and stash everything, untracked files included.
    pub(crate) fn stash_with_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.repo.as_ref().is_some_and(|r| r.changes.is_empty()) {
            return self.info("There's nothing to stash", window, cx);
        }
        let note = self.note.clone();
        note.update(cx, |state, cx| state.set_value("", window, cx));
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let note_ok = note.clone();
            note.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .title("Stash Changes")
                .w(px(420.))
                .child(
                    v_flex().gap_2().child(Input::new(&note)).child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Everything uncommitted, new files too. It waits under Stashes in the branch list."),
                    ),
                )
                .footer(dialog_buttons("stash-message", "Stash"))
                .on_ok(move |_, window, cx| {
                    let message = note_ok.read(cx).value().trim().to_string();
                    let _ = spor.update(cx, |this, cx| {
                        let m = (!message.is_empty()).then_some(message.as_str());
                        match this.git().stash_save(m, true) {
                            Ok(()) => {
                                this.ok("Changes stashed", window, cx);
                                this.reload(window, cx);
                            }
                            Err(e) => this.error(
                                format!("Couldn't stash: {}", first_line(&e)),
                                window,
                                cx,
                            ),
                        }
                    });
                    true
                })
        });
    }

    pub(crate) fn apply_stash_keep(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.git().stash_apply_at(index) {
            Ok(()) => {
                self.ok("Stash applied — it's kept too", window, cx);
                self.tab = Tab::Changes;
            }
            Err(e) => self.error(
                format!("Couldn't apply the stash: {}", first_line(&e)),
                window,
                cx,
            ),
        }
        self.reload(window, cx);
    }

    pub(crate) fn drop_stash(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.git().stash_drop_at(index) {
            Ok(stash) => self.did("Stash deleted".into(), Undo::Stash(stash), window, cx),
            Err(e) => self.error(format!("Couldn't delete: {}", first_line(&e)), window, cx),
        }
        self.reload(window, cx);
    }

    // ── Worktrees ────────────────────────────────────────────────────────

    pub(crate) fn remove_worktree(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.git().remove_worktree(&path, false) {
            Ok(()) => self.ok("Working tree removed — its branch stays", window, cx),
            Err(e) if e.contains("modified or untracked") || e.contains("--force") => {
                let spor = cx.entity().downgrade();
                let shown = super::support::tildify(&path);
                window.open_dialog(cx, move |dialog, _, cx| {
                    let spor = spor.clone();
                    let path = path.clone();
                    dialog
                        .title("Remove a working tree with changes?")
                        .w(px(440.))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{shown} has uncommitted changes. They're deleted with it and \
                                 can't be brought back."
                                )),
                        )
                        .footer(dialog_buttons("remove-worktree", "Remove"))
                        .on_ok(move |_, window, cx| {
                            let path = path.clone();
                            let _ = spor.update(cx, |this, cx| {
                                match this.git().remove_worktree(&path, true) {
                                    Ok(()) => this.ok("Working tree removed", window, cx),
                                    Err(e) => this.error(first_line(&e).to_string(), window, cx),
                                }
                                this.reload(window, cx);
                            });
                            true
                        })
                });
            }
            Err(e) => self.error(first_line(&e).to_string(), window, cx),
        }
        self.reload(window, cx);
    }

    // ── Remote ───────────────────────────────────────────────────────────

    /// Pull, replaying local commits on top of what came in.
    pub(crate) fn pull_rebase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        if !self.require_clean("pulling with rebase", window, cx) {
            return;
        }
        self.run_job("Pull", window, cx, move || {
            super::support::git_network(&root, &["pull", "--rebase"])
                .map(|_| "Pulled and rebased".into())
        });
    }

    // ── Search ───────────────────────────────────────────────────────────

    /// Focus History's search field.
    pub(crate) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = Tab::History;
        let input = self.history_search.clone();
        input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Show the history of one file: History, searching `path:<file>`.
    pub(crate) fn show_file_history(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = format!("path:{path}");
        self.history_search
            .update(cx, |state, cx| state.set_value(query, window, cx));
        self.tab = Tab::History;
        self.run_search(cx);
    }

    /// Search commits for what's in the field: message, author or hash, or
    /// the commits touching a file with `path:`.
    pub(crate) fn run_search(&mut self, cx: &mut Context<Self>) {
        let query = self.history_search.read(cx).value().trim().to_string();
        if query == self.search_query {
            return;
        }
        self.search_query = query.clone();
        if query.is_empty() {
            self.search_hits.clear();
            cx.notify();
            return;
        }
        let git = self.git();
        self._search_task = cx.spawn(async move |this, cx| {
            let hits = cx
                .background_spawn(async move {
                    match query.strip_prefix("path:") {
                        Some(path) => git
                            .file_log(path.trim(), 2000)
                            .map(|c| c.into_iter().map(|c| c.hash).collect())
                            .unwrap_or_default(),
                        None => git.search_commits(&query, 2000).unwrap_or_default(),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.search_hits = hits.into_iter().collect();
                if let Some(ix) = this.search_rows().first().copied() {
                    this.selected = None;
                    this.select_commit(ix, cx);
                }
                cx.notify();
            });
        });
    }

    /// Rows of History that match the search, top to bottom.
    pub(crate) fn search_rows(&self) -> Vec<usize> {
        let Some(repo) = &self.repo else {
            return Vec::new();
        };
        repo.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| self.search_hits.contains(&r.commit.hash))
            .map(|(i, _)| i)
            .collect()
    }

    /// Jump to the next (or previous) match.
    pub(crate) fn next_match(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.search_rows();
        if rows.is_empty() {
            return;
        }
        let current = self.selected.unwrap_or(0);
        let next = if delta > 0 {
            rows.iter().find(|&&r| r > current).or(rows.first())
        } else {
            rows.iter().rev().find(|&&r| r < current).or(rows.last())
        };
        if let Some(&ix) = next {
            self.select_commit(ix, cx);
        }
    }

    pub(crate) fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_search
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.search_query.clear();
        self.search_hits.clear();
        cx.notify();
    }

    // ── Interactive rebase ───────────────────────────────────────────────

    /// Plan rewriting the commits after `base` in a tab of its own.
    pub(crate) fn start_interactive_rebase(
        &mut self,
        base: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(branch) = self.repo.as_ref().and_then(|r| r.tracking.branch.clone()) else {
            return self.info("Check out a branch to rewrite its history", window, cx);
        };
        if !self.git().is_ancestor(base, "HEAD") {
            return self.info(format!("That commit isn't part of {branch}"), window, cx);
        }
        if self.git().has_merges_since(base) {
            return self.info(
                format!(
                    "{branch} has merge commits after that one — rewriting would flatten them. \
                     Pick a commit after the last merge."
                ),
                window,
                cx,
            );
        }
        let commits = match self.git().commits_since(base) {
            Ok(c) if c.is_empty() => {
                return self.info(
                    format!("{branch} has no commits after that one"),
                    window,
                    cx,
                )
            }
            Ok(c) => c,
            Err(e) => return self.error(first_line(&e).to_string(), window, cx),
        };
        let base_label = self
            .repo
            .as_ref()
            .and_then(|r| r.rows.iter().find(|r| r.commit.hash == base))
            .map(|r| format!("{} {}", r.commit.short, r.commit.subject))
            .unwrap_or_else(|| short(base));
        self.rebase = Some(RebaseDraft {
            base: base.to_string(),
            base_label,
            branch,
            selected: 0,
            steps: commits
                .into_iter()
                .map(|c| DraftStep {
                    message: String::new(),
                    hash: c.hash,
                    short: c.short,
                    subject: c.subject,
                    action: StepAction::Pick,
                })
                .collect(),
        });
        self.rebase_original = self
            .rebase
            .as_ref()
            .map(|d| d.steps.iter().map(|s| s.hash.clone()).collect())
            .unwrap_or_default();
        self.select_step(0, window, cx);
        self.tab = Tab::Rebase;
        cx.notify();
    }

    fn select_step(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &mut self.rebase else {
            return;
        };
        let Some(step) = draft.steps.get(ix) else {
            return;
        };
        draft.selected = ix;
        let text = if step.message.is_empty() {
            // Start a reword from the commit's full message.
            self.repo
                .as_ref()
                .and_then(|r| r.git.commit_details(&step.hash).ok())
                .map(|d| d.message)
                .unwrap_or_else(|| step.subject.clone())
        } else {
            step.message.clone()
        };
        self.rebase_message
            .update(cx, |state, cx| state.set_value(text, window, cx));
        cx.notify();
    }

    fn set_step_action(
        &mut self,
        ix: usize,
        action: StepAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(step) = self.rebase.as_mut().and_then(|d| d.steps.get_mut(ix)) {
            step.action = action;
        }
        self.select_step(ix, window, cx);
    }

    fn move_step(&mut self, ix: usize, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &mut self.rebase else {
            return;
        };
        let to = if up { ix.checked_sub(1) } else { Some(ix + 1) };
        let Some(to) = to.filter(|&t| t < draft.steps.len()) else {
            return;
        };
        draft.steps.swap(ix, to);
        self.select_step(to, window, cx);
    }

    /// Keep the selected step's message in step with the editor.
    pub(crate) fn sync_rebase_message(&mut self, cx: &mut Context<Self>) {
        let text = self.rebase_message.read(cx).value().to_string();
        if let Some(draft) = &mut self.rebase {
            if let Some(step) = draft.steps.get_mut(draft.selected) {
                if step.action == StepAction::Reword {
                    step.message = text;
                }
            }
        }
    }

    pub(crate) fn close_rebase(&mut self, cx: &mut Context<Self>) {
        self.rebase = None;
        if self.tab == Tab::Rebase {
            self.tab = Tab::History;
        }
        cx.notify();
    }

    fn run_rebase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_rebase_message(cx);
        let Some(draft) = &self.rebase else { return };
        let plan = match draft.plan() {
            Ok(plan) => plan,
            Err(e) => return self.info(e, window, cx),
        };
        if !draft.changed(&self.rebase_original) {
            return self.info(
                "Nothing changed — reorder, reword, squash or drop a commit first",
                window,
                cx,
            );
        }
        if !self.require_clean("rewriting history", window, cx) {
            return;
        }
        let (base, branch) = (draft.base.clone(), draft.branch.clone());
        let start = self.rewrite_start();
        let result = self.git().rebase_interactive(&base, &plan);
        if result.is_ok() {
            self.rebase = None;
            self.tab = Tab::History;
        }
        self.finish_rewrite(format!("Rewrote {branch}"), start, result, window, cx);
    }

    pub(crate) fn render_rebase(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(draft) = &self.rebase else {
            return div().into_any_element();
        };
        let selected = draft.selected;
        let count = draft.steps.len();
        let reword = draft
            .steps
            .get(selected)
            .is_some_and(|s| s.action == StepAction::Reword);
        let spor = Self::weak(cx);
        let rows = draft.steps.iter().enumerate().map(|(ix, step)| {
            let action = step.action;
            let dropped = action == StepAction::Drop;
            let folded = matches!(action, StepAction::Squash | StepAction::Fixup);
            let subject = if action == StepAction::Reword && !step.message.trim().is_empty() {
                first_line(step.message.trim()).to_string()
            } else {
                step.subject.clone()
            };
            let menu_spor = spor.clone();
            h_flex()
                .id(("rebase-step", ix))
                .h(px(32.))
                .px_3()
                .gap_2()
                .text_sm()
                .rounded(theme.radius)
                .when(ix == selected, |this| this.bg(theme.list_active))
                .when(ix != selected, |this| {
                    this.hover(|this| this.bg(theme.list_hover))
                })
                .on_click(cx.listener(move |this, _, w, cx| this.select_step(ix, w, cx)))
                .child(
                    Button::new(("step-action", ix))
                        .outline()
                        .xsmall()
                        .w(px(84.))
                        .label(action.label())
                        .dropdown_caret(true)
                        .dropdown_menu(move |mut menu, _, _| {
                            for a in StepAction::ALL {
                                let spor = menu_spor.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(format!("{} — {}", a.label(), a.hint()))
                                        .checked(a == action)
                                        .on_click(move |_, window, cx| {
                                            let _ = spor.update(cx, |this, cx| {
                                                this.set_step_action(ix, a, window, cx)
                                            });
                                        }),
                                );
                            }
                            menu
                        }),
                )
                .when(folded, |this| {
                    this.child(
                        Icon::new(IconName::ArrowUp)
                            .xsmall()
                            .text_color(theme.muted_foreground),
                    )
                })
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(theme.muted_foreground)
                        .child(step.short.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when(dropped, |this| {
                            this.line_through().text_color(theme.muted_foreground)
                        })
                        .child(subject),
                )
                .child(
                    Button::new(("step-up", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronUp)
                        .disabled(ix == 0)
                        .tooltip("Move earlier")
                        .on_click(
                            cx.listener(move |this, _, w, cx| this.move_step(ix, true, w, cx)),
                        ),
                )
                .child(
                    Button::new(("step-down", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronDown)
                        .disabled(ix + 1 == count)
                        .tooltip("Move later")
                        .on_click(
                            cx.listener(move |this, _, w, cx| this.move_step(ix, false, w, cx)),
                        ),
                )
        });
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(32.))
                    .flex_none()
                    .px_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "Rewriting {} on {} after {} — oldest first, as git replays them",
                        plural(count, "commit"),
                        draft.branch,
                        draft.base_label
                    )),
            )
            .child(
                div()
                    .id("rebase-steps")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_1()
                    .child(v_flex().gap_0p5().children(rows)),
            )
            .when(reword, |this| {
                this.child(
                    v_flex()
                        .flex_none()
                        .p_2()
                        .gap_1()
                        .border_t_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("New message"),
                        )
                        .child(Textarea::new(&self.rebase_message)),
                )
            })
            .child(
                h_flex()
                    .flex_none()
                    .p_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("⌘Z afterwards puts the branch back as it was"),
                    )
                    .child(
                        Button::new("rebase-cancel")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.close_rebase(cx))),
                    )
                    .child(
                        Button::new("rebase-run")
                            .primary()
                            .small()
                            .label("Rewrite History")
                            .on_click(cx.listener(|this, _, w, cx| this.run_rebase(w, cx))),
                    ),
            )
            .into_any_element()
    }
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// git's errors come with hints around them; keep the line that says what
/// went wrong.
fn first_line_of_error(e: &str) -> String {
    e.lines()
        .map(str::trim)
        .find(|l| l.starts_with("error:") || l.starts_with("fatal:"))
        .map(|l| {
            l.trim_start_matches("error:")
                .trim_start_matches("fatal:")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| first_line(e).to_string())
}

fn dialog_buttons(id: &'static str, action: &'static str) -> impl IntoElement {
    use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
    DialogFooter::new()
        .gap_2()
        .child(DialogClose::new().child(Button::new("dialog-cancel").label("Cancel").outline()))
        .child(DialogAction::new().child(Button::new(id).label(action).primary()))
}
