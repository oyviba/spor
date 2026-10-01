//! Sheets: dialogs that drop down from the title bar, as on macOS. Only the
//! two that need input or a real decision remain; destructive actions are
//! undoable instead of confirmed.

use super::theme::{self, icon};
use super::titlebar;
use super::widgets;
use super::{Modal, SporApp};
use eframe::egui::{self, Align2, FontId, Key, RichText, Ui, Vec2};

/// An action chosen inside a sheet, run once the sheet's borrow ends.
type Deferred = Box<dyn FnOnce(&mut SporApp)>;

fn sheet_header(ui: &mut Ui, glyph: &str, title: &str) {
    let p = theme::palette(ui.ctx());
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(glyph)
                .font(FontId::proportional(34.0))
                .color(p.accent),
        );
        ui.add_space(4.0);
        ui.label(RichText::new(title).font(theme::semibold(14.0)));
    });
    ui.add_space(6.0);
}

fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(18.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
}

impl SporApp {
    pub(super) fn modals(&mut self, ctx: &egui::Context) {
        let Some(modal) = &mut self.modal else { return };
        let p = theme::palette(ctx);
        let mut close = false;
        let mut run: Option<Deferred> = None;

        let id = egui::Id::new("spor_sheet");
        let resp = egui::Modal::new(id)
            .area(
                egui::Modal::default_area(id)
                    .anchor(Align2::CENTER_TOP, Vec2::new(0.0, titlebar::HEIGHT - 2.0)),
            )
            .frame(
                egui::Frame::popup(&ctx.global_style())
                    .inner_margin(egui::Margin::same(20))
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 12,
                        se: 12,
                    }),
            )
            .show(ctx, |ui| {
                ui.set_width(380.0);
                match modal {
                    Modal::NewBranch { name, sha, label } => {
                        sheet_header(ui, icon::GIT_BRANCH, "New Branch");
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(format!("from {label}"))
                                    .size(12.0)
                                    .color(p.muted),
                            );
                        });
                        ui.add_space(12.0);
                        let edit = ui.add(
                            egui::TextEdit::singleline(name)
                                .hint_text("Branch name")
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if !edit.has_focus() && !edit.lost_focus() {
                            edit.request_focus();
                        }
                        let invalid = name.contains(char::is_whitespace);
                        let ok = !name.trim().is_empty() && !invalid;
                        if invalid {
                            ui.label(
                                RichText::new("Branch names can't contain spaces.")
                                    .size(11.5)
                                    .color(p.red),
                            );
                        }
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        buttons(ui, |ui| {
                            if (widgets::primary_button(ui, "Create", ok).clicked() || enter) && ok
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
                        sheet_header(ui, icon::ARCHIVE_BOX, "Stash Your Changes?");
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Switching to “{target}” would overwrite uncommitted \
                                     changes. Stash them first — they'll be waiting under \
                                     Stashes in the sidebar."
                                ))
                                .size(12.5)
                                .color(p.muted),
                            );
                        });
                        buttons(ui, |ui| {
                            if widgets::primary_button(ui, "Stash and Switch", true).clicked() {
                                let t = target.clone();
                                run = Some(Box::new(move |app| app.stash_and_switch(&t)));
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
