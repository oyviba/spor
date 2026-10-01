//! Right-hand inspector: details and changed files of the selected commit, or
//! the staging area and commit composer for uncommitted changes.

use super::diff_view::change_icon;
use super::theme::{self, icon, Palette};
use super::widgets::{self, avatar, list_row, split_path, text_until};
use super::{Modal, Sel, SporApp};
use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, RichText, Sense, Ui, Vec2};
use spor::git::{FileStatus, StatusEntry};

const SUMMARY_SOFT_LIMIT: usize = 72;

enum WipAction {
    Select(usize),
    Toggle(usize),
    Discard(usize),
    All(bool),
}

impl SporApp {
    pub(super) fn inspector(&mut self, ui: &mut Ui) {
        match self.sel {
            Sel::Wip => self.wip_inspector(ui),
            Sel::Commit(_) => self.commit_inspector(ui),
        }
    }

    fn commit_inspector(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let Some(ins) = &self.inspected else {
            return super::diff_view::placeholder(ui, icon::GIT_COMMIT, "No commit selected");
        };
        let Sel::Commit(row_idx) = self.sel else {
            return;
        };
        let Some(repo) = &self.repo else { return };
        let Some(row) = repo.rows.get(row_idx) else {
            return;
        };

        let mut select_file = None;
        let mut jump_to = None;
        let mut copy = None;

        egui::ScrollArea::vertical()
            .id_salt("inspector")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.add_space(14.0);
                let mut lines = ins.details.message.lines();
                let subject = lines.next().unwrap_or(&row.commit.subject);
                let body = lines.collect::<Vec<_>>().join("\n");
                ui.add(
                    egui::Label::new(
                        RichText::new(subject)
                            .font(theme::semibold(16.0))
                            .color(p.text),
                    )
                    .wrap(),
                );
                let body = body.trim();
                if !body.is_empty() {
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(body).color(p.muted)).wrap());
                }
                ui.add_space(14.0);

                // Author card.
                ui.horizontal(|ui| {
                    avatar(ui, &ins.details.author, 34.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(&ins.details.author).font(theme::semibold(13.5)));
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                ins.details.date,
                                widgets::relative_time(ins.details.author_time)
                            ))
                            .size(12.0)
                            .color(p.muted),
                        );
                    });
                });
                if !ins.details.committer.is_empty() && ins.details.committer != ins.details.author
                {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!("Committed by {}", ins.details.committer))
                            .size(12.0)
                            .color(p.faint),
                    );
                }
                ui.add_space(12.0);

                // SHA + parents.
                meta_row(ui, &p, "Commit", |ui| {
                    if ui
                        .add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{}  {}",
                                    &ins.hash[..ins.hash.len().min(12)],
                                    icon::COPY
                                ))
                                .monospace()
                                .color(p.text),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_text("Copy full SHA")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        copy = Some(ins.hash.clone());
                    }
                });
                if !row.commit.parents.is_empty() {
                    let label = if row.commit.parents.len() > 1 {
                        "Parents"
                    } else {
                        "Parent"
                    };
                    meta_row(ui, &p, label, |ui| {
                        for parent in &row.commit.parents {
                            let short = &parent[..parent.len().min(7)];
                            if ui
                                .add(
                                    egui::Label::new(
                                        RichText::new(short).monospace().color(p.accent),
                                    )
                                    .sense(Sense::click()),
                                )
                                .on_hover_text("Go to parent")
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                jump_to = repo.rows.iter().position(|r| &r.commit.hash == parent);
                            }
                        }
                    });
                }

                ui.add_space(16.0);
                let (adds, dels) = ins
                    .files
                    .iter()
                    .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
                ui.horizontal(|ui| {
                    widgets::section_header(ui, "Changed files", Some(ins.files.len()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("−{dels}"))
                                .size(11.5)
                                .monospace()
                                .color(p.red),
                        );
                        ui.label(
                            RichText::new(format!("+{adds}"))
                                .size(11.5)
                                .monospace()
                                .color(p.green),
                        );
                    });
                });
                ui.add_space(2.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                for (i, f) in ins.files.iter().enumerate() {
                    let selected = self.file_sel == Some(i);
                    let (glyph, color) = change_icon(f.change, &p);
                    let resp = list_row(ui, selected, 28.0, |painter, rect, p| {
                        let counts = if f.binary {
                            "bin".to_string()
                        } else {
                            format!("+{} −{}", f.additions, f.deletions)
                        };
                        paint_file_row(painter, rect, p, glyph, color, f.path(), &counts, None);
                    });
                    if resp.clicked() {
                        select_file = Some(i);
                    }
                }
                if ins.files.is_empty() {
                    ui.label(RichText::new("No file changes").color(p.faint));
                }
                ui.add_space(12.0);
            });

        if let Some(h) = copy {
            ui.ctx().copy_text(h);
            self.info("Copied commit SHA");
        }
        if let Some(i) = select_file {
            self.select_file(i);
        }
        if let Some(i) = jump_to {
            self.select(Sel::Commit(i));
            self.scroll_to_sel = true;
        }
    }

    fn wip_inspector(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let Some(repo) = &self.repo else { return };
        let status = repo.status.clone();
        let staged: Vec<usize> = (0..status.len())
            .filter(|&i| status[i].status.is_staged())
            .collect();
        let unstaged: Vec<usize> = (0..status.len())
            .filter(|&i| !status[i].status.is_staged())
            .collect();
        let branch = repo
            .tracking
            .branch
            .clone()
            .unwrap_or_else(|| "detached HEAD".into());

        let mut action = None;

        // Composer pinned to the bottom; file lists scroll above it.
        egui::Panel::bottom("composer")
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 0,
                right: 0,
                top: 10,
                bottom: 14,
            }))
            .show(ui, |ui| self.composer(ui, &p, staged.len(), &branch));

        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("wip")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add_space(14.0);
                        ui.label(RichText::new("Uncommitted changes").font(theme::semibold(16.0)));
                        ui.label(
                            RichText::new(format!("on {} {branch}", icon::GIT_BRANCH))
                                .size(12.0)
                                .color(p.muted),
                        );
                        ui.add_space(12.0);
                        for (title, idxs, is_staged) in
                            [("Unstaged", &unstaged, false), ("Staged", &staged, true)]
                        {
                            ui.horizontal(|ui| {
                                widgets::section_header(ui, title, Some(idxs.len()));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let label = if is_staged {
                                            format!("{}  Unstage all", icon::MINUS)
                                        } else {
                                            format!("{}  Stage all", icon::PLUS)
                                        };
                                        if !idxs.is_empty() && ui.small_button(label).clicked() {
                                            action = Some(WipAction::All(is_staged));
                                        }
                                    },
                                );
                            });
                            ui.add_space(2.0);
                            ui.spacing_mut().item_spacing.y = 1.0;
                            if idxs.is_empty() {
                                let hint = if is_staged {
                                    "Stage files to include them in the commit"
                                } else {
                                    "Nothing left to stage"
                                };
                                ui.label(RichText::new(hint).size(12.0).color(p.faint));
                            }
                            for &i in idxs.iter() {
                                if let Some(a) = self.wip_row(ui, &p, &status[i], i, is_staged) {
                                    action = Some(a);
                                }
                            }
                            ui.spacing_mut().item_spacing.y = 6.0;
                            ui.add_space(14.0);
                        }
                    });
            });

        match action {
            Some(WipAction::Select(i)) => self.select_file(i),
            Some(WipAction::Toggle(i)) => {
                self.file_sel = Some(i);
                self.toggle_stage(&status[i]);
            }
            Some(WipAction::Discard(i)) => {
                self.modal = Some(Modal::Discard {
                    entry: status[i].clone(),
                })
            }
            Some(WipAction::All(staged)) => self.stage_all(staged),
            None => {}
        }
    }

    fn wip_row(
        &self,
        ui: &mut Ui,
        p: &Palette,
        e: &StatusEntry,
        i: usize,
        is_staged: bool,
    ) -> Option<WipAction> {
        let (glyph, color) = status_icon(&e.status, p);
        let selected = self.file_sel == Some(i);
        let path = match &e.orig_path {
            Some(orig) => format!("{orig} → {}", e.path),
            None => e.path.clone(),
        };
        let resp = list_row(ui, selected, 28.0, |painter, rect, p| {
            paint_file_row(painter, rect, p, glyph, color, &path, "", Some(58.0));
        });
        let mut action = None;
        if resp.clicked() {
            action = Some(WipAction::Select(i));
        }
        // Hover actions on the right edge of the row.
        if resp.hovered() || selected {
            let r = resp.rect;
            let mut x = r.right() - 26.0;
            let toggle = if is_staged { icon::MINUS } else { icon::PLUS };
            let tip = if is_staged { "Unstage" } else { "Stage" };
            if small_icon(ui, Pos2::new(x, r.center().y), toggle, tip, p.text, p).clicked() {
                action = Some(WipAction::Toggle(i));
            }
            x -= 26.0;
            if !is_staged
                && small_icon(
                    ui,
                    Pos2::new(x, r.center().y),
                    icon::TRASH,
                    "Discard changes…",
                    p.red,
                    p,
                )
                .clicked()
            {
                action = Some(WipAction::Discard(i));
            }
        }
        resp.context_menu(|ui| {
            let label = if is_staged {
                format!("{}  Unstage", icon::MINUS)
            } else {
                format!("{}  Stage", icon::PLUS)
            };
            if ui.button(label).clicked() {
                action = Some(WipAction::Toggle(i));
            }
            if !is_staged
                && ui
                    .button(
                        RichText::new(format!("{}  Discard changes…", icon::TRASH)).color(p.red),
                    )
                    .clicked()
            {
                action = Some(WipAction::Discard(i));
            }
        });
        action
    }

    fn composer(&mut self, ui: &mut Ui, p: &Palette, staged: usize, branch: &str) {
        let summary = ui.add(
            egui::TextEdit::singleline(&mut self.summary)
                .hint_text("Summary (required)")
                .font(theme::semibold(13.5))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(10, 8)),
        );
        let len = self.summary.chars().count();
        if len > 0 {
            let color = if len > SUMMARY_SOFT_LIMIT {
                p.yellow
            } else {
                p.faint
            };
            let pos = Pos2::new(summary.rect.right() - 10.0, summary.rect.center().y);
            ui.painter().text(
                pos,
                Align2::RIGHT_CENTER,
                len.to_string(),
                FontId::proportional(11.0),
                color,
            );
        }
        ui.add_space(6.0);
        let desc = ui.add(
            egui::TextEdit::multiline(&mut self.description)
                .hint_text("Description")
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(10, 8)),
        );
        ui.add_space(10.0);
        let can = staged > 0 && !self.summary.trim().is_empty();
        let label = match staged {
            0 => "Stage files to commit".to_string(),
            1 => format!("Commit 1 file to {branch}"),
            n => format!("Commit {n} files to {branch}"),
        };
        let clicked = ui
            .with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| widgets::primary_button(ui, &label, can),
            )
            .inner
            .on_hover_text("⌘⏎")
            .clicked();
        let cmd_enter = (summary.has_focus() || desc.has_focus())
            && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
        if can && (clicked || cmd_enter) {
            self.commit();
        }
    }
}

fn meta_row(ui: &mut Ui, p: &Palette, label: &str, value: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 18.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.0),
            p.faint,
        );
        value(ui);
    });
}

fn status_icon(s: &FileStatus, p: &Palette) -> (&'static str, Color32) {
    match s {
        FileStatus::Staged => (icon::PENCIL_SIMPLE, p.green),
        FileStatus::StagedDeleted => (icon::FILE_MINUS, p.red),
        FileStatus::Modified => (icon::PENCIL_SIMPLE, p.yellow),
        FileStatus::Deleted => (icon::FILE_MINUS, p.red),
        FileStatus::Untracked => (icon::FILE_PLUS, p.green),
    }
}

/// Icon, file name (bold) + directory (muted), and right-aligned text.
/// `reserve` keeps space free on the right for hover buttons.
#[allow(clippy::too_many_arguments)]
fn paint_file_row(
    painter: &egui::Painter,
    rect: Rect,
    p: &Palette,
    glyph: &str,
    color: Color32,
    path: &str,
    trailing: &str,
    reserve: Option<f32>,
) {
    let mid = rect.center().y;
    painter.text(
        Pos2::new(rect.left() + 16.0, mid),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(14.0),
        color,
    );
    let mut right = rect.right() - reserve.unwrap_or(8.0);
    if !trailing.is_empty() {
        let r = painter.text(
            Pos2::new(right, mid),
            Align2::RIGHT_CENTER,
            trailing,
            FontId::monospace(11.0),
            p.faint,
        );
        right = r.left() - 8.0;
    }
    let (name, dir) = split_path(path);
    let end = text_until(
        painter,
        rect.left() + 30.0,
        mid,
        right,
        name,
        FontId::proportional(13.0),
        p.text,
    );
    if !dir.is_empty() {
        text_until(
            painter,
            end + 6.0,
            mid,
            right,
            dir.trim_end_matches('/'),
            FontId::proportional(12.0),
            p.faint,
        );
    }
}

/// A 22px icon button painted at `center`, interactable on top of a row.
fn small_icon(
    ui: &mut Ui,
    center: Pos2,
    glyph: &str,
    tip: &str,
    color: Color32,
    p: &Palette,
) -> egui::Response {
    let rect = Rect::from_center_size(center, Vec2::splat(22.0));
    let id = ui.id().with(("small_icon", glyph, center.y as i32));
    let resp = ui.interact(rect, id, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, p.raised);
    }
    ui.painter().text(
        center,
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(14.0),
        color,
    );
    resp.on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
