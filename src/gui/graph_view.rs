//! The commit list: one painted row per commit with graph lanes, ref pills,
//! subject, author, date and short SHA in aligned columns.

use super::theme::{self, adapt, icon, Palette};
use super::widgets::{paint_avatar, paint_pill, pill_width, relative_time, text_until, Seg};
use eframe::egui::{
    self, epaint::CubicBezierShape, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2,
};
use spor::color::{branch_family, color_for, Rgb};
use spor::graph::GraphRow;
use spor::remote::{ChecksState, PrInfo, ReviewState};
use std::collections::HashMap;

pub const ROW_HEIGHT: f32 = 30.0;
const LANE_WIDTH: f32 = 16.0;
const LEFT_PAD: f32 = 16.0;
const NODE_RADIUS: f32 = 5.0;
pub const MAX_LANES: usize = 14;

const AUTHOR_W: f32 = 140.0;
const DATE_W: f32 = 104.0;
const SHA_W: f32 = 64.0;

pub fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// Repo-level context needed to label refs.
pub struct RepoMeta<'a> {
    pub remotes: &'a [String],
    pub prs: &'a HashMap<String, PrInfo>,
}

/// Which optional columns fit at this width.
pub struct Columns {
    pub graph_w: f32,
    pub author: bool,
    pub date: bool,
    pub sha: bool,
}

impl Columns {
    pub fn new(width: f32, lanes: usize) -> Self {
        let graph_w = LEFT_PAD + lanes.clamp(1, MAX_LANES) as f32 * LANE_WIDTH;
        Self {
            graph_w,
            author: width > 500.0,
            date: width > 620.0,
            sha: width > 760.0,
        }
    }

    fn right_reserved(&self) -> f32 {
        let mut w = 12.0;
        if self.author {
            w += AUTHOR_W;
        }
        if self.date {
            w += DATE_W;
        }
        if self.sha {
            w += SHA_W;
        }
        w
    }

    /// Column captions, painted into the header strip above the list.
    pub fn paint_header(&self, painter: &egui::Painter, rect: Rect, p: &Palette) {
        let font = theme::semibold(10.5);
        let y = rect.center().y;
        let cap = |x: f32, text: &str| {
            painter.text(
                Pos2::new(x, y),
                Align2::LEFT_CENTER,
                text,
                font.clone(),
                p.faint,
            );
        };
        cap(rect.left() + 12.0, "GRAPH");
        cap(rect.left() + self.graph_w + 6.0, "DESCRIPTION");
        let mut x = rect.right() - 12.0;
        if self.sha {
            x -= SHA_W;
            cap(x, "SHA");
        }
        if self.date {
            x -= DATE_W;
            cap(x, "DATE");
        }
        if self.author {
            x -= AUTHOR_W;
            cap(x, "AUTHOR");
        }
    }
}

fn lane_x(rect: Rect, lane: usize) -> f32 {
    rect.left() + LEFT_PAD + lane as f32 * LANE_WIDTH
}

fn pr_glyph(pr: &PrInfo, p: &Palette) -> (Color32, &'static str, Color32) {
    let num = if pr.draft {
        p.faint
    } else {
        match pr.review {
            ReviewState::Approved => p.green,
            ReviewState::ChangesRequested => p.red,
            ReviewState::None => p.blue,
        }
    };
    let (glyph, color) = match pr.checks {
        ChecksState::Passing => (icon::CHECK_CIRCLE, p.green),
        ChecksState::Failing => (icon::X_CIRCLE, p.red),
        ChecksState::Pending => (icon::CIRCLE_DASHED, p.yellow),
        ChecksState::None => ("", Color32::TRANSPARENT),
    };
    (num, glyph, color)
}

/// One label to draw on a row.
struct Label {
    segs: Vec<Seg>,
    tint: Color32,
    solid: bool,
}

/// Turn a commit's refs into labels. A local branch and its remote twin on
/// the same commit collapse into one pill with a cloud mark, so `main` and
/// `origin/main` don't take up two slots.
fn labels(row: &GraphRow, meta: &RepoMeta, p: &Palette) -> Vec<Label> {
    let refs = &row.commit.refs;
    let split_remote = |r: &str| -> Option<(String, String)> {
        let (remote, rest) = r.split_once('/')?;
        meta.remotes
            .iter()
            .any(|x| x == remote)
            .then(|| (remote.to_string(), rest.to_string()))
    };
    let mut out = Vec::new();
    let mut tags = Vec::new();
    for r in refs {
        if let Some(tag) = r.strip_prefix("tag:") {
            tags.push(tag.trim().to_string());
            continue;
        }
        if let Some((_, rest)) = split_remote(r) {
            if rest == "HEAD" || refs.iter().any(|o| o == &rest) {
                continue; // drawn with its local twin, or a symbolic ref
            }
            let tint = adapt(rgb(color_for(branch_family(r, meta.remotes), r)), p);
            let mut segs = vec![Seg::new(format!("{} {r}", icon::CLOUD), tint)];
            add_pr(&mut segs, meta.prs.get(&rest), p);
            out.push(Label {
                segs,
                tint,
                solid: false,
            });
            continue;
        }
        let is_head = row.commit.head_ref.as_deref() == Some(r.as_str());
        let synced = refs
            .iter()
            .any(|o| split_remote(o).is_some_and(|(_, rest)| &rest == r));
        let tint = if is_head {
            p.head
        } else {
            adapt(rgb(color_for(branch_family(r, meta.remotes), r)), p)
        };
        let fg = if is_head {
            Color32::from_rgb(0x1d, 0x1e, 0x24)
        } else {
            tint
        };
        let mut text = format!("{} {r}", icon::GIT_BRANCH);
        if synced {
            text.push_str(&format!("  {}", icon::CLOUD));
        }
        let mut segs = vec![Seg::new(text, fg)];
        add_pr(&mut segs, meta.prs.get(r.as_str()), p);
        let label = Label {
            segs,
            tint,
            solid: is_head,
        };
        if is_head {
            out.insert(0, label);
        } else {
            out.push(label);
        }
    }
    for t in tags {
        out.push(Label {
            segs: vec![Seg::new(format!("{} {t}", icon::TAG), p.yellow)],
            tint: p.yellow,
            solid: false,
        });
    }
    out
}

fn add_pr(segs: &mut Vec<Seg>, pr: Option<&PrInfo>, p: &Palette) {
    let Some(pr) = pr else { return };
    let (num, glyph, glyph_color) = pr_glyph(pr, p);
    segs.push(Seg::new(format!("  #{}", pr.number), num));
    if !glyph.is_empty() {
        segs.push(Seg::new(format!(" {glyph}"), glyph_color));
    }
}

/// Paint a commit row's contents into `rect` (backgrounds are the caller's).
pub fn paint_row(
    painter: &egui::Painter,
    rect: Rect,
    row: &GraphRow,
    meta: &RepoMeta,
    cols: &Columns,
    p: &Palette,
    selected: bool,
) {
    paint_lanes(painter, rect, row, p, selected);

    let mid = rect.center().y;
    let mut x = rect.left() + cols.graph_w + 6.0;
    let text_right = rect.right() - cols.right_reserved();

    // Ref pills, as many as fit while leaving the subject some room.
    let all = labels(row, meta, p);
    let total = all.len();
    for (i, label) in all.into_iter().enumerate() {
        let w = pill_width(painter, &label.segs);
        if x + w > text_right - 80.0 {
            let more = total - i;
            let segs = [Seg::new(format!("+{more}"), p.muted)];
            if x + pill_width(painter, &segs) < text_right {
                x = paint_pill(painter, x, mid, &segs, p.muted, false) + 6.0;
            }
            break;
        }
        x = paint_pill(painter, x, mid, &label.segs, label.tint, label.solid) + 6.0;
    }

    text_until(
        painter,
        x + 2.0,
        mid,
        text_right - 12.0,
        &row.commit.subject,
        FontId::proportional(13.5),
        p.text,
    );

    // Right-hand columns.
    let mut cx = rect.right() - 12.0;
    let small = FontId::proportional(12.5);
    if cols.sha {
        cx -= SHA_W;
        painter.text(
            Pos2::new(cx, mid),
            Align2::LEFT_CENTER,
            &row.commit.short,
            FontId::monospace(12.0),
            p.faint,
        );
    }
    if cols.date {
        cx -= DATE_W;
        painter.text(
            Pos2::new(cx, mid),
            Align2::LEFT_CENTER,
            relative_time(row.commit.timestamp),
            small.clone(),
            p.muted,
        );
    }
    if cols.author {
        cx -= AUTHOR_W;
        paint_avatar(painter, Pos2::new(cx + 9.0, mid), &row.commit.author, 18.0);
        text_until(
            painter,
            cx + 24.0,
            mid,
            cx + AUTHOR_W - 12.0,
            &row.commit.author,
            small,
            p.muted,
        );
    }
}

/// The pseudo-row for uncommitted changes: a dashed node above the graph.
pub fn paint_wip_row(
    painter: &egui::Painter,
    rect: Rect,
    head_lane: usize,
    summary: &str,
    cols: &Columns,
    p: &Palette,
) {
    let mid = rect.center().y;
    let x = lane_x(rect, head_lane.min(MAX_LANES - 1));
    let c = Pos2::new(x, mid);
    // Dotted line down to the HEAD commit's row.
    let mut y = mid + NODE_RADIUS + 3.0;
    while y < rect.bottom() {
        painter.line_segment(
            [Pos2::new(x, y), Pos2::new(x, (y + 3.0).min(rect.bottom()))],
            Stroke::new(2.0, p.faint),
        );
        y += 6.0;
    }
    painter.circle_filled(c, NODE_RADIUS + 1.0, p.bg);
    let n = 12;
    for i in 0..n {
        let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = a0 + std::f32::consts::TAU / n as f32 * 0.55;
        painter.line_segment(
            [
                c + Vec2::angled(a0) * (NODE_RADIUS + 1.0),
                c + Vec2::angled(a1) * (NODE_RADIUS + 1.0),
            ],
            Stroke::new(1.6, p.muted),
        );
    }
    let tx = rect.left() + cols.graph_w + 6.0;
    let segs = [Seg::new(
        format!("{} Uncommitted changes", icon::PENCIL_SIMPLE),
        p.text,
    )];
    let end = paint_pill(painter, tx, mid, &segs, p.muted, false);
    painter.text(
        Pos2::new(end + 10.0, mid),
        Align2::LEFT_CENTER,
        summary,
        FontId::proportional(12.5),
        p.muted,
    );
}

fn paint_lanes(painter: &egui::Painter, rect: Rect, row: &GraphRow, p: &Palette, selected: bool) {
    let top = rect.top();
    let bottom = rect.bottom();
    let mid = rect.center().y;
    let hash = row.commit.hash.as_str();
    let name_hint = row
        .commit
        .refs
        .first()
        .cloned()
        .unwrap_or_else(|| row.commit.hash.clone());

    let lane_count = row
        .lanes_before
        .len()
        .max(row.lanes_after.len())
        .max(row.lane + 1)
        .min(MAX_LANES);
    let lane_color = |l: usize| {
        let family = if l == row.lane {
            row.branch_family.as_str()
        } else {
            row.lane_families
                .get(l)
                .and_then(|f| f.as_deref())
                .unwrap_or("_")
        };
        adapt(rgb(color_for(family, &name_hint)), p)
    };
    let node = Pos2::new(lane_x(rect, row.lane.min(MAX_LANES - 1)), mid);
    let width = 2.0;

    for l in 0..lane_count {
        let x = lane_x(rect, l);
        let before = row.lanes_before.get(l).and_then(|s| s.as_deref());
        let after = row.lanes_after.get(l).and_then(|s| s.as_deref());
        let stroke = Stroke::new(width, lane_color(l));

        if l == row.lane {
            if before == Some(hash) {
                painter.line_segment([Pos2::new(x, top), node], stroke);
            }
            if after.is_some() {
                painter.line_segment([node, Pos2::new(x, bottom)], stroke);
            }
            continue;
        }

        // Top half: a branch converging on this commit (its fork point)
        // curves in; anything else passing through comes straight down.
        let passing = before.is_some() && before == after;
        if before == Some(hash) {
            curve(painter, Pos2::new(x, top), node, stroke);
        } else if passing {
            painter.line_segment([Pos2::new(x, top), Pos2::new(x, bottom)], stroke);
        } else if before.is_some() {
            painter.line_segment([Pos2::new(x, top), Pos2::new(x, mid)], stroke);
        }

        // Bottom half: a merge parent this commit pulls in curves out of the
        // node; a lane that just opened otherwise starts mid-row.
        if let Some(a) = after.filter(|_| !passing) {
            if row.commit.parents.iter().skip(1).any(|p| p == a) {
                curve(painter, node, Pos2::new(x, bottom), stroke);
            } else {
                painter.line_segment([Pos2::new(x, mid), Pos2::new(x, bottom)], stroke);
            }
        }
    }

    // Node: ring for HEAD, hollow for merges, filled dot otherwise. A halo in
    // the row's background color separates it from lines passing behind.
    let color = lane_color(row.lane);
    let halo = if selected { p.selection } else { p.bg };
    painter.circle_filled(node, NODE_RADIUS + 2.0, halo);
    if row.commit.head_ref.is_some() {
        painter.circle_filled(node, NODE_RADIUS + 1.5, p.head);
        painter.circle_filled(node, NODE_RADIUS - 1.5, halo);
    } else if row.commit.parents.len() > 1 {
        painter.circle_stroke(node, NODE_RADIUS - 0.5, Stroke::new(2.0, color));
    } else {
        painter.circle_filled(node, NODE_RADIUS, color);
    }
}

fn curve(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    let dy = (to.y - from.y) * 0.6;
    painter.add(CubicBezierShape::from_points_stroke(
        [from, from + Vec2::new(0.0, dy), to - Vec2::new(0.0, dy), to],
        false,
        Color32::TRANSPARENT,
        stroke,
    ));
}
