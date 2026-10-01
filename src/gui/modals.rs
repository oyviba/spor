//! Confirmation and input dialogs.

use super::theme::{self, icon};
use super::widgets;
use super::{Modal, SporApp};
use eframe::egui::{self, FontId, Key, RichText, Ui};
use spor::git::FileStatus;

/// An action chosen inside a modal, run once the modal's borrow ends.
type Deferred = Box<dyn FnOnce(&mut SporApp)>;

fn dialog_header(ui: &mut Ui, glyph: &str, color: egui::Color32, title: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(glyph)
                .font(FontId::proportional(22.0))
                .color(color),
        );
        ui.label(RichText::new(title).font(theme::semibold(16.0)));
    });
    ui.add_space(6.0);
}

fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(16.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
}

impl SporApp {
    pub(super) fn modals(&mut self, ctx: &egui::Context) {
        let Some(modal) = &mut self.modal else { return };
        let p = theme::palette(ctx);
        let mut close = false;
        let mut run: Option<Deferred> = None;

        let resp = egui::Modal::new(egui::Id::new("spor_modal"))
            .frame(
                egui::Frame::popup(&ctx.global_style())
                    .inner_margin(egui::Margin::same(22))
                    .corner_radius(egui::CornerRadius::same(12)),
            )
            .show(ctx, |ui| {
                ui.set_width(420.0);
                match modal {
                    Modal::NewBranch { name, sha, label } => {
                        dialog_header(ui, icon::GIT_BRANCH, p.accent, "Create branch");
                        ui.label(RichText::new(format!("Starting at {label}")).color(p.muted));
                        ui.add_space(12.0);
                        let edit = ui.add(
                            egui::TextEdit::singleline(name)
                                .hint_text("feat/my-change")
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(10, 8)),
                        );
                        if !edit.has_focus() && !edit.lost_focus() {
                            edit.request_focus();
                        }
                        let ok = !name.trim().is_empty() && !name.contains(' ');
                        if name.contains(' ') {
                            ui.label(
                                RichText::new("Branch names can't contain spaces")
                                    .size(12.0)
                                    .color(p.red),
                            );
                        }
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        buttons(ui, |ui| {
                            if (widgets::primary_button(ui, "Create & check out", ok).clicked()
                                || enter)
                                && ok
                            {
                                let (n, s) = (name.trim().to_string(), sha.clone());
                                run = Some(Box::new(move |app| app.create_branch(&n, &s)));
                            }
                            if widgets::secondary_button(ui, "Cancel").clicked() {
                                close = true;
                            }
                        });
                    }
                    Modal::StashAndSwitch { target } => {
                        dialog_header(ui, icon::STACK, p.yellow, "Uncommitted changes");
                        ui.label(format!(
                            "Your changes would be overwritten by switching to {target}. \
                             Stash them first? You can restore them later with Pop."
                        ));
                        buttons(ui, |ui| {
                            if widgets::primary_button(ui, "Stash & switch", true).clicked() {
                                let t = target.clone();
                                run = Some(Box::new(move |app| app.stash_and_switch(&t)));
                            }
                            if widgets::secondary_button(ui, "Cancel").clicked() {
                                close = true;
                            }
                        });
                    }
                    Modal::Discard { entry } => {
                        let what = match entry.status {
                            FileStatus::Untracked => "Delete this untracked file?",
                            FileStatus::Deleted => "Restore this deleted file?",
                            _ => "Discard changes to this file?",
                        };
                        dialog_header(ui, icon::TRASH, p.red, what);
                        ui.label(RichText::new(&entry.path).monospace().color(p.text));
                        ui.add_space(4.0);
                        ui.label(RichText::new("This can't be undone.").color(p.muted));
                        buttons(ui, |ui| {
                            if widgets::danger_button(ui, "Discard").clicked() {
                                let e = entry.clone();
                                run = Some(Box::new(move |app| app.discard(&e)));
                            }
                            if widgets::secondary_button(ui, "Keep").clicked() {
                                close = true;
                            }
                        });
                    }
                    Modal::PushBehind { behind } => {
                        dialog_header(ui, icon::WARNING, p.yellow, "Branch is behind its upstream");
                        ui.label(format!(
                            "The remote has {behind} commit{} you don't have. A plain push will be \
                             rejected — pull first to bring them in.",
                            if *behind == 1 { "" } else { "s" }
                        ));
                        buttons(ui, |ui| {
                            if widgets::primary_button(ui, "Pull first", true).clicked() {
                                run = Some(Box::new(|app| app.pull()));
                            }
                            if widgets::secondary_button(ui, "Push anyway").clicked() {
                                run = Some(Box::new(|app| app.push()));
                            }
                            if widgets::secondary_button(ui, "Cancel").clicked() {
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
