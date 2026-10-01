//! Left sidebar: local branches, remote branches grouped by remote, and tags.
//! Click reveals the tip in the commit list; double-click checks out.

use super::graph_view::rgb;
use super::theme::{self, adapt, icon, Palette};
use super::widgets::{list_row, text_until};
use super::SporApp;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Ui};
use spor::color::{branch_family, color_for};
use spor::remote::{ChecksState, PrInfo};

enum Action {
    Reveal(String),
    Checkout(String),
    NewBranch(String),
    Copy(String),
}

struct Item {
    /// Ref key used to find the tip row (`main`, `origin/main`, `tag:v1`).
    key: String,
    label: String,
    color: egui::Color32,
    glyph: &'static str,
    current: bool,
    trailing: String,
    pr: Option<PrInfo>,
}

impl SporApp {
    pub(super) fn sidebar(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let Some(repo) = &self.repo else { return };

        ui.add_space(10.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.sidebar_filter)
                .hint_text(format!("{}  Filter", icon::MAGNIFYING_GLASS))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(8, 5)),
        );
        ui.add_space(6.0);

        let q = self.sidebar_filter.to_lowercase();
        let keep = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
        let selected_hash = match self.sel {
            super::Sel::Commit(i) => repo.rows.get(i).map(|r| r.commit.hash.clone()),
            super::Sel::Wip => None,
        };
        let tip_is_selected = |key: &str| {
            selected_hash.as_ref().is_some_and(|h| {
                repo.ref_rows
                    .get(key)
                    .and_then(|&i| repo.rows.get(i))
                    .is_some_and(|r| &r.commit.hash == h)
            })
        };

        let branch_color =
            |name: &str| adapt(rgb(color_for(branch_family(name, &repo.remotes), name)), &p);

        let mut local = Vec::new();
        let mut remote: Vec<(String, Vec<Item>)> = repo
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
                    color: branch_color(&b.name),
                    glyph: icon::GIT_BRANCH,
                    current: false,
                    trailing: String::new(),
                    pr: None,
                };
                match remote.iter_mut().find(|(r, _)| r == rem) {
                    Some((_, list)) => list.push(item),
                    None => remote.push((rem.to_string(), vec![item])),
                }
            } else {
                let trailing = if b.is_current {
                    let t = &repo.tracking;
                    let mut s = String::new();
                    if t.ahead > 0 {
                        s.push_str(&format!("{}{}", icon::ARROW_UP, t.ahead));
                    }
                    if t.behind > 0 {
                        s.push_str(&format!(" {}{}", icon::ARROW_DOWN, t.behind));
                    }
                    s.trim().to_string()
                } else {
                    String::new()
                };
                local.push(Item {
                    key: b.name.clone(),
                    label: b.name.clone(),
                    color: if b.is_current {
                        p.head
                    } else {
                        branch_color(&b.name)
                    },
                    glyph: if b.is_current {
                        icon::CHECK_CIRCLE
                    } else {
                        icon::GIT_BRANCH
                    },
                    current: b.is_current,
                    trailing,
                    pr: repo.prs.get(&b.name).cloned(),
                });
            }
        }
        // Tags, newest first (graph order).
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
                color: p.yellow,
                glyph: icon::TAG,
                current: false,
                trailing: String::new(),
                pr: None,
            })
            .collect();
        let stashes = repo.stashes;

        let mut action = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                group(
                    ui,
                    "Local",
                    icon::GIT_BRANCH,
                    &local,
                    &tip_is_selected,
                    &mut action,
                    &p,
                    true,
                );
                for (name, items) in &remote {
                    if !items.is_empty() {
                        group(
                            ui,
                            name,
                            icon::CLOUD,
                            items,
                            &tip_is_selected,
                            &mut action,
                            &p,
                            true,
                        );
                    }
                }
                if !tags.is_empty() {
                    group(
                        ui,
                        "Tags",
                        icon::TAG,
                        &tags,
                        &tip_is_selected,
                        &mut action,
                        &p,
                        false,
                    );
                }
                if stashes > 0 {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  {stashes} stash{}",
                            icon::STACK,
                            if stashes == 1 { "" } else { "es" }
                        ))
                        .color(p.muted),
                    );
                }
            });

        match action {
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
                            format!("{} {}", row.commit.short, key),
                        )
                    });
                if let Some((sha, label)) = target {
                    self.new_branch_dialog(sha, label);
                }
            }
            Some(Action::Copy(text)) => {
                ui.ctx().copy_text(text);
                self.info("Copied to clipboard");
            }
            None => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn group(
    ui: &mut Ui,
    title: &str,
    glyph: &str,
    items: &[Item],
    is_selected: &dyn Fn(&str) -> bool,
    action: &mut Option<Action>,
    p: &Palette,
    default_open: bool,
) {
    let id = ui.make_persistent_id(("sidebar-group", title));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open)
        .show_header(ui, |ui| {
            ui.label(
                egui::RichText::new(format!("{glyph}  {}", title.to_uppercase()))
                    .font(theme::semibold(10.5))
                    .color(p.faint),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(items.len().to_string())
                        .font(FontId::proportional(10.5))
                        .color(p.faint),
                );
            });
        })
        .body_unindented(|ui| {
            for item in items {
                let resp = list_row(ui, is_selected(&item.key), 26.0, |painter, rect, p| {
                    paint_item(painter, rect, item, p)
                });
                if resp.clicked() {
                    *action = Some(Action::Reveal(item.key.clone()));
                }
                let is_tag = item.key.starts_with("tag:");
                if resp.double_clicked() && !is_tag && !item.current {
                    *action = Some(Action::Checkout(item.key.clone()));
                }
                resp.context_menu(|ui| {
                    if !is_tag
                        && !item.current
                        && ui
                            .button(format!("{}  Check out", icon::ARROW_RIGHT))
                            .clicked()
                    {
                        *action = Some(Action::Checkout(item.key.clone()));
                    }
                    if ui
                        .button(format!("{}  New branch from here…", icon::GIT_BRANCH))
                        .clicked()
                    {
                        *action = Some(Action::NewBranch(item.key.clone()));
                    }
                    if ui.button(format!("{}  Copy name", icon::COPY)).clicked() {
                        *action = Some(Action::Copy(item.label.clone()));
                    }
                });
            }
            ui.add_space(6.0);
        });
}

fn paint_item(painter: &egui::Painter, rect: Rect, item: &Item, p: &Palette) {
    let mid = rect.center().y;
    painter.text(
        Pos2::new(rect.left() + 18.0, mid),
        Align2::CENTER_CENTER,
        item.glyph,
        FontId::proportional(14.0),
        item.color,
    );
    // Right side first so the name can be clipped against it.
    let mut right = rect.right() - 8.0;
    if let Some(pr) = &item.pr {
        let (glyph, color) = match pr.checks {
            ChecksState::Passing => (icon::CHECK_CIRCLE, p.green),
            ChecksState::Failing => (icon::X_CIRCLE, p.red),
            ChecksState::Pending => (icon::CIRCLE_DASHED, p.yellow),
            ChecksState::None => (icon::GIT_PULL_REQUEST, p.blue),
        };
        let r = painter.text(
            Pos2::new(right, mid),
            Align2::RIGHT_CENTER,
            format!("#{} {glyph}", pr.number),
            FontId::proportional(11.5),
            color,
        );
        right = r.left() - 6.0;
    }
    if !item.trailing.is_empty() {
        let r = painter.text(
            Pos2::new(right, mid),
            Align2::RIGHT_CENTER,
            &item.trailing,
            FontId::proportional(11.5),
            p.accent,
        );
        right = r.left() - 6.0;
    }
    let font = if item.current {
        theme::semibold(13.0)
    } else {
        FontId::proportional(13.0)
    };
    text_until(
        painter,
        rect.left() + 32.0,
        mid,
        right,
        &item.label,
        font,
        p.text,
    );
}
