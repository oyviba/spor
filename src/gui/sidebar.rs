//! Source-list sidebar, Finder/Mail style: the two views at the top
//! (Changes, History), then collapsible Branches, Remotes, Tags and Stashes.
//! Icons take the accent color; selection is a neutral rounded highlight.

use super::theme::{self, icon, Palette};
use super::widgets::{list_row, text_until};
use super::{SidebarSel, SporApp};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Ui, Vec2};

enum Action {
    Changes,
    History,
    Reveal(String),
    Checkout(String),
    NewBranch(String),
    ApplyStash(usize),
    Copy(String),
}

struct Item {
    key: String,
    label: String,
    glyph: &'static str,
    current: bool,
    trailing: String,
}

impl SporApp {
    pub(super) fn sidebar(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let Some(repo) = &self.repo else { return };

        let q = self.sidebar_filter.to_lowercase();
        let keep = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);

        let changes = repo.changes.len();
        let mut local = Vec::new();
        let mut remotes: Vec<(String, Vec<Item>)> = repo
            .remotes
            .iter()
            .map(|r| (r.clone(), Vec::new()))
            .collect();
        for b in repo.branches.iter().filter(|b| keep(&b.name)) {
            if b.is_remote {
                let (rem, rest) = b.name.split_once('/').unwrap_or(("", &b.name));
                let item = Item {
                    key: b.name.clone(),
                    label: rest.to_string(),
                    glyph: icon::GIT_BRANCH,
                    current: false,
                    trailing: String::new(),
                };
                match remotes.iter_mut().find(|(r, _)| r == rem) {
                    Some((_, list)) => list.push(item),
                    None => remotes.push((rem.to_string(), vec![item])),
                }
            } else {
                let mut trailing = String::new();
                if b.is_current {
                    let t = &repo.tracking;
                    if t.ahead > 0 {
                        trailing.push_str(&format!("{}{} ", icon::ARROW_UP, t.ahead));
                    }
                    if t.behind > 0 {
                        trailing.push_str(&format!("{}{} ", icon::ARROW_DOWN, t.behind));
                    }
                    trailing.push_str(icon::CHECK);
                }
                if let Some(pr) = repo.prs.get(&b.name) {
                    trailing = format!("#{} {trailing}", pr.number);
                }
                local.push(Item {
                    key: b.name.clone(),
                    label: b.name.clone(),
                    glyph: icon::GIT_BRANCH,
                    current: b.is_current,
                    trailing: trailing.trim().to_string(),
                });
            }
        }
        let mut tags: Vec<(usize, String)> = repo
            .ref_rows
            .iter()
            .filter_map(|(k, &i)| k.strip_prefix("tag:").map(|t| (i, t.to_string())))
            .filter(|(_, t)| keep(t))
            .collect();
        tags.sort();
        let tags: Vec<Item> = tags
            .into_iter()
            .map(|(_, t)| Item {
                key: format!("tag:{t}"),
                label: t,
                glyph: icon::TAG,
                current: false,
                trailing: String::new(),
            })
            .collect();
        let stashes: Vec<(usize, String)> = repo
            .stashes
            .iter()
            .enumerate()
            .filter(|(_, s)| keep(s))
            .map(|(i, s)| (i, s.clone()))
            .collect();

        let sel = self.sidebar_sel.clone();
        let mut action = None;

        // Filter pinned to the bottom, like Xcode's navigator.
        egui::Panel::bottom("sidebar_filter")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.sidebar_filter)
                        .hint_text(format!("{}  Filter", icon::FUNNEL_SIMPLE))
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 4)),
                );
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.add_space(6.0);
                        let count = (changes > 0).then(|| changes.to_string());
                        if nav_row(
                            ui,
                            icon::TRAY,
                            "Changes",
                            count,
                            sel == SidebarSel::Changes,
                            &p,
                        )
                        .clicked()
                        {
                            action = Some(Action::Changes);
                        }
                        if nav_row(
                            ui,
                            icon::CLOCK_COUNTER_CLOCKWISE,
                            "History",
                            None,
                            sel == SidebarSel::History,
                            &p,
                        )
                        .clicked()
                        {
                            action = Some(Action::History);
                        }

                        group(ui, "Branches", true, &p, |ui| {
                            for item in &local {
                                item_row(ui, item, &sel, &mut action, &p, true);
                            }
                        });
                        for (remote, items) in &remotes {
                            if items.is_empty() {
                                continue;
                            }
                            group(ui, &format!("Remote: {remote}"), false, &p, |ui| {
                                for item in items {
                                    item_row(ui, item, &sel, &mut action, &p, true);
                                }
                            });
                        }
                        if !tags.is_empty() {
                            group(ui, "Tags", false, &p, |ui| {
                                for item in &tags {
                                    item_row(ui, item, &sel, &mut action, &p, false);
                                }
                            });
                        }
                        if !stashes.is_empty() {
                            group(ui, "Stashes", true, &p, |ui| {
                                for (i, msg) in &stashes {
                                    // "WIP on main: abc123 subject" → "subject"
                                    let label = msg
                                        .split_once(": ")
                                        .map(|(_, rest)| {
                                            rest.split_once(' ').map_or(rest, |(_, s)| s)
                                        })
                                        .unwrap_or(msg);
                                    let resp = list_row(ui, false, 24.0, |painter, rect, p| {
                                        paint_item(
                                            painter,
                                            rect,
                                            icon::ARCHIVE_BOX,
                                            label,
                                            "",
                                            false,
                                            p,
                                        );
                                    })
                                    .on_hover_text(msg.as_str());
                                    resp.context_menu(|ui| {
                                        if ui.button("Apply and Remove").clicked() {
                                            action = Some(Action::ApplyStash(*i));
                                        }
                                    });
                                    if resp.double_clicked() {
                                        action = Some(Action::ApplyStash(*i));
                                    }
                                }
                            });
                        }
                        ui.add_space(10.0);
                    });
            });

        match action {
            Some(Action::Changes) => self.show_changes(),
            Some(Action::History) => {
                self.sidebar_sel = SidebarSel::History;
                self.show_history();
            }
            Some(Action::Reveal(key)) => self.reveal_ref(&key),
            Some(Action::Checkout(name)) => self.checkout(&name),
            Some(Action::NewBranch(key)) => {
                let target = self
                    .repo
                    .as_ref()
                    .and_then(|r| r.ref_rows.get(&key).and_then(|&i| r.rows.get(i)))
                    .map(|row| {
                        (
                            row.commit.hash.clone(),
                            format!("{} — {key}", row.commit.short),
                        )
                    });
                if let Some((sha, label)) = target {
                    self.new_branch_dialog(sha, label);
                }
            }
            Some(Action::ApplyStash(i)) => self.apply_stash(i),
            Some(Action::Copy(text)) => {
                ui.ctx().copy_text(text);
            }
            None => {}
        }
    }
}

/// Collapsible section with a caption; the chevron shows on hover, as in
/// Finder.
fn group(ui: &mut Ui, title: &str, default_open: bool, p: &Palette, body: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(("sidebar-group", title));
    let mut open = ui
        .ctx()
        .data_mut(|d| *d.get_persisted_mut_or(id, default_open));
    ui.add_space(10.0);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
    ui.painter().text(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::semibold(11.0),
        p.faint,
    );
    if resp.hovered() {
        let glyph = if open {
            icon::CARET_DOWN
        } else {
            icon::CARET_RIGHT
        };
        ui.painter().text(
            Pos2::new(rect.right() - 10.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(11.0),
            p.muted,
        );
    }
    if resp.clicked() {
        open = !open;
        ui.ctx().data_mut(|d| d.insert_persisted(id, open));
    }
    if open {
        body(ui);
    }
}

/// Top-level view entry with an optional count badge.
fn nav_row(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    count: Option<String>,
    selected: bool,
    p: &Palette,
) -> egui::Response {
    list_row(ui, selected, 28.0, |painter, rect, _| {
        let mid = rect.center().y;
        painter.text(
            Pos2::new(rect.left() + 16.0, mid),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(16.0),
            p.accent,
        );
        painter.text(
            Pos2::new(rect.left() + 32.0, mid),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            p.text,
        );
        if let Some(c) = count {
            let g = painter.layout_no_wrap(c, theme::semibold(10.5), p.muted);
            let w = g.size().x + 12.0;
            let r = Rect::from_center_size(
                Pos2::new(rect.right() - 8.0 - w / 2.0, mid),
                Vec2::new(w, 16.0),
            );
            painter.rect_filled(
                r,
                CornerRadius::same(8),
                if p.dark {
                    Color32::from_white_alpha(22)
                } else {
                    Color32::from_black_alpha(16)
                },
            );
            let gs = g.size();
            painter.galley(r.center() - gs / 2.0, g, p.muted);
        }
    })
}

fn item_row(
    ui: &mut Ui,
    item: &Item,
    sel: &SidebarSel,
    action: &mut Option<Action>,
    p: &Palette,
    branch: bool,
) {
    let selected = matches!(sel, SidebarSel::Ref(k) if *k == item.key);
    let resp = list_row(ui, selected, 24.0, |painter, rect, _| {
        paint_item(
            painter,
            rect,
            item.glyph,
            &item.label,
            &item.trailing,
            item.current,
            p,
        )
    });
    if resp.clicked() {
        *action = Some(Action::Reveal(item.key.clone()));
    }
    if branch && resp.double_clicked() && !item.current {
        *action = Some(Action::Checkout(item.key.clone()));
    }
    resp.context_menu(|ui| {
        if branch && !item.current && ui.button(format!("Check Out “{}”", item.label)).clicked()
        {
            *action = Some(Action::Checkout(item.key.clone()));
        }
        if ui.button("New Branch from Here…").clicked() {
            *action = Some(Action::NewBranch(item.key.clone()));
        }
        ui.separator();
        if ui.button("Copy Name").clicked() {
            *action = Some(Action::Copy(item.label.clone()));
        }
    });
}

fn paint_item(
    painter: &egui::Painter,
    rect: Rect,
    glyph: &str,
    label: &str,
    trailing: &str,
    current: bool,
    p: &Palette,
) {
    let mid = rect.center().y;
    painter.text(
        Pos2::new(rect.left() + 16.0, mid),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(14.0),
        p.accent,
    );
    let mut right = rect.right() - 8.0;
    if !trailing.is_empty() {
        let r = painter.text(
            Pos2::new(right, mid),
            Align2::RIGHT_CENTER,
            trailing,
            FontId::proportional(11.0),
            p.muted,
        );
        right = r.left() - 6.0;
    }
    let font = if current {
        theme::semibold(13.0)
    } else {
        FontId::proportional(13.0)
    };
    text_until(painter, rect.left() + 32.0, mid, right, label, font, p.text);
}
