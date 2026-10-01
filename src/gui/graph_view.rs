//! History rows, Mail-style: a thin graph on the left, then two lines —
//! subject and time, author and ref tags. Color lives in the graph lines;
//! everything else stays neutral.

use super::theme::{adapt, icon, Palette};
use super::widgets::{relative_time, tag, tag_width, text_until};
use eframe::egui::{self, epaint::CubicBezierShape, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use spor::color::{color_for, Rgb};
use spor::graph::GraphRow;
use spor::remote::{ChecksState, PrInfo};
use std::collections::HashMap;

pub const ROW_HEIGHT: f32 = 50.0;
const LANE_WIDTH: f32 = 13.0;
const LEFT_PAD: f32 = 16.0;
const NODE_RADIUS: f32 = 4.0;
pub const MAX_LANES: usize = 10;

pub fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// Repo-level context needed to label refs.
pub struct RepoMeta<'a> {
    pub remotes: &'a [String],
    pub prs: &'a HashMap<String, PrInfo>,
}

pub fn graph_width(lanes: usize) -> f32 {
    LEFT_PAD + lanes.clamp(1, MAX_LANES) as f32 * LANE_WIDTH + 2.0
}

fn lane_x(rect: Rect, lane: usize) -> f32 {
    rect.left() + LEFT_PAD + lane as f32 * LANE_WIDTH
}

/// Short labels for a commit's refs: local branches (merged with their
/// remote twin), remote-only branches, tags. The checked-out branch first.
fn ref_labels(row: &GraphRow, meta: &RepoMeta) -> Vec<(String, bool)> {
    let refs = &row.commit.refs;
    let remote_rest = |r: &str| -> Option<String> {
        let (remote, rest) = r.split_once('/')?;
        meta.remotes
            .iter()
            .any(|x| x == remote)
            .then(|| rest.to_string())
    };
    let mut out: Vec<(String, bool)> = Vec::new();
    for r in refs {
        if let Some(t) = r.strip_prefix("tag:") {
            out.push((format!("{} {}", icon::TAG, t.trim()), false));
            continue;
        }
        if let Some(rest) = remote_rest(r) {
            if rest == "HEAD" || refs.iter().any(|o| o == &rest) {
                continue;
            }
        }
        let mut label = r.clone();
        if let Some(pr) = meta.prs.get(remote_rest(r).as_deref().unwrap_or(r)) {
            let mark = match pr.checks {
                ChecksState::Passing => format!(" {}", icon::CHECK),
                ChecksState::Failing => format!(" {}", icon::X),
                ChecksState::Pending => format!(" {}", icon::CIRCLE_DASHED),
                ChecksState::None => String::new(),
            };
            label.push_str(&format!("  #{}{mark}", pr.number));
        }
        let head = row.commit.head_ref.as_deref() == Some(r.as_str());
        if head {
            out.insert(0, (label, true));
        } else {
            out.push((label, false));
        }
    }
    out
}

/// Paint one History row. `selected` rows are drawn on the accent color.
pub fn paint_row(
    painter: &egui::Painter,
    rect: Rect,
    row: &GraphRow,
    meta: &RepoMeta,
    graph_w: f32,
    p: &Palette,
    selected: bool,
) {
    let halo = if selected { p.selection } else { p.bg };
    paint_lanes(painter, rect, row, p, halo);

    let (text, muted) = if selected {
        (Color32::WHITE, Color32::from_white_alpha(200))
    } else {
        (p.text, p.muted)
    };
    let x = rect.left() + graph_w + 4.0;
    let right = rect.right() - 14.0;
    let line1 = rect.top() + 17.0;
    let line2 = rect.top() + 35.0;

    // Line 1: subject … time.
    let time = relative_time(row.commit.timestamp);
    let tg = painter.layout_no_wrap(time, FontId::proportional(11.5), muted);
    let tw = tg.size().x;
    let th = tg.size().y;
    painter.galley(Pos2::new(right - tw, line1 - th / 2.0), tg, muted);
    text_until(
        painter,
        x,
        line1,
        right - tw - 12.0,
        &row.commit.subject,
        FontId::proportional(13.0),
        text,
    );

    // Line 2: author, then ref tags.
    let mut cx = text_until(
        painter,
        x,
        line2,
        right - 60.0,
        &row.commit.author,
        FontId::proportional(11.5),
        muted,
    ) + 8.0;
    let labels = ref_labels(row, meta);
    let n = labels.len();
    for (i, (label, head)) in labels.into_iter().enumerate() {
        let w = tag_width(painter, &label);
        if cx + w > right {
            let more = format!("+{}", n - i);
            if cx + tag_width(painter, &more) <= right {
                tag(
                    painter,
                    cx,
                    line2,
                    &more,
                    muted,
                    tag_fill(p, selected, false),
                );
            }
            break;
        }
        let fg = match (selected, head) {
            (true, _) => Color32::WHITE,
            (false, true) => p.accent,
            (false, false) => p.muted,
        };
        cx = tag(painter, cx, line2, &label, fg, tag_fill(p, selected, head)) + 5.0;
    }
}

fn tag_fill(p: &Palette, selected: bool, head: bool) -> Color32 {
    match (selected, head) {
        (true, _) => Color32::from_white_alpha(46),
        (false, true) => p.accent.gamma_multiply(0.16),
        (false, false) => {
            if p.dark {
                Color32::from_white_alpha(18)
            } else {
                Color32::from_black_alpha(14)
            }
        }
    }
}

fn paint_lanes(painter: &egui::Painter, rect: Rect, row: &GraphRow, p: &Palette, halo: Color32) {
    let top = rect.top();
    let bottom = rect.bottom();
    let mid = rect.top() + 17.0;
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
    // Lane colors keep their hue but are toned down toward the label gray,
    // so the graph reads as structure, not decoration.
    let lane_color = |l: usize| {
        let family = if l == row.lane {
            row.branch_family.as_str()
        } else {
            row.lane_families
                .get(l)
                .and_then(|f| f.as_deref())
                .unwrap_or("_")
        };
        adapt(rgb(color_for(family, &name_hint)), p).lerp_to_gamma(p.muted, 0.3)
    };
    let node = Pos2::new(lane_x(rect, row.lane.min(MAX_LANES - 1)), mid);
    let width = 1.6;

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
        // node; a lane that just opened otherwise starts at the node's height.
        if let Some(a) = after.filter(|_| !passing) {
            if row.commit.parents.iter().skip(1).any(|p| p == a) {
                curve(painter, node, Pos2::new(x, bottom), stroke);
            } else {
                painter.line_segment([Pos2::new(x, mid), Pos2::new(x, bottom)], stroke);
            }
        }
    }

    let color = lane_color(row.lane);
    painter.circle_filled(node, NODE_RADIUS + 2.0, halo);
    if row.commit.head_ref.is_some() {
        // HEAD: an accent ring.
        painter.circle_filled(node, NODE_RADIUS + 1.0, p.accent);
        painter.circle_filled(node, NODE_RADIUS - 1.5, halo);
    } else if row.commit.parents.len() > 1 {
        painter.circle_stroke(node, NODE_RADIUS - 0.5, Stroke::new(1.6, color));
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
