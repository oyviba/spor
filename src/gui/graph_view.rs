//! Paints one row of the commit timeline: lane lines, the commit node, branch
//! labels (with PR badges) and the subject. Mirrors the terminal renderer in
//! `src/ui.rs` so both frontends read the same, just with real curves.

use eframe::egui::{self, epaint::CubicBezierShape, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use spor::color::{branch_family, color_for, Rgb};
use spor::graph::GraphRow;
use spor::remote::{ChecksState, PrInfo, ReviewState};
use std::collections::HashMap;

pub const ROW_HEIGHT: f32 = 24.0;
const LANE_WIDTH: f32 = 16.0;
const LEFT_PAD: f32 = 10.0;
const NODE_RADIUS: f32 = 4.5;

pub const HEAD_GOLD: Color32 = Color32::from_rgb(255, 210, 60);
pub const DIM: Color32 = Color32::from_rgb(120, 120, 135);

pub fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// Repo-level context needed to label refs.
pub struct RepoMeta<'a> {
    pub remotes: &'a [String],
    pub prs: &'a HashMap<String, PrInfo>,
}

fn lane_x(rect: Rect, lane: usize) -> f32 {
    rect.left() + LEFT_PAD + lane as f32 * LANE_WIDTH
}

/// Same rule as the terminal UI: a remote-tracking ref defers its PR badge to
/// a local twin on the same commit, so the badge shows once.
fn pr_for_ref<'a>(
    r: &str,
    all_refs: &[String],
    remotes: &[String],
    prs: &'a HashMap<String, PrInfo>,
) -> Option<&'a PrInfo> {
    if r.starts_with("tag:") {
        return None;
    }
    let name = match r.split_once('/') {
        Some((remote, rest)) if remotes.iter().any(|x| x == remote) => {
            if all_refs.iter().any(|other| other == rest) {
                return None;
            }
            rest
        }
        _ => r,
    };
    prs.get(name)
}

fn pr_colors(pr: &PrInfo) -> (Color32, &'static str, Color32) {
    let num = if pr.draft {
        Color32::from_rgb(110, 110, 125)
    } else {
        match pr.review {
            ReviewState::Approved => Color32::from_rgb(120, 200, 120),
            ReviewState::ChangesRequested => Color32::from_rgb(220, 100, 100),
            ReviewState::None => Color32::from_rgb(140, 170, 255),
        }
    };
    let (glyph, glyph_color) = match pr.checks {
        ChecksState::Passing => ("✔", Color32::from_rgb(120, 200, 120)),
        ChecksState::Failing => ("✖", Color32::from_rgb(220, 100, 100)),
        ChecksState::Pending => ("●", Color32::from_rgb(220, 180, 60)),
        ChecksState::None => ("", Color32::TRANSPARENT),
    };
    (num, glyph, glyph_color)
}

fn rel_time(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let dt = (now - timestamp).max(0);
    match dt {
        0..=59 => format!("{dt}s"),
        60..=3599 => format!("{}m", dt / 60),
        3600..=86_399 => format!("{}h", dt / 3600),
        86_400..=604_799 => format!("{}d", dt / 86_400),
        604_800..=2_591_999 => format!("{}w", dt / 604_800),
        2_592_000..=31_535_999 => format!("{}mo", dt / 2_592_000),
        _ => format!("{}y", dt / 31_536_000),
    }
}

/// Paint a graph row into `rect`. Selection/hover backgrounds are the
/// caller's job; this only draws content.
pub fn paint_row(
    painter: &egui::Painter,
    rect: Rect,
    row: &GraphRow,
    meta: &RepoMeta,
    text: Color32,
) {
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
        .max(row.lane + 1);
    let lane_color = |l: usize| {
        let family = if l == row.lane {
            row.branch_family.as_str()
        } else {
            row.lane_families
                .get(l)
                .and_then(|f| f.as_deref())
                .unwrap_or("_")
        };
        rgb(color_for(family, &name_hint))
    };
    let node = Pos2::new(lane_x(rect, row.lane), mid);
    let width = 2.0;

    for l in 0..lane_count {
        let x = lane_x(rect, l);
        let before = row.lanes_before.get(l).and_then(|s| s.as_deref());
        let after = row.lanes_after.get(l).and_then(|s| s.as_deref());
        let stroke = Stroke::new(width, lane_color(l));

        if l == row.lane {
            // Something above was waiting for this commit → line in from the top.
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

    // The commit node: gold ring for HEAD, hollow for merges, dot otherwise.
    let color = lane_color(row.lane);
    if row.commit.head_ref.is_some() {
        painter.circle_filled(node, NODE_RADIUS + 1.5, HEAD_GOLD);
        painter.circle_filled(node, NODE_RADIUS - 1.5, painter_bg(painter));
    } else if row.commit.parents.len() > 1 {
        painter.circle_filled(node, NODE_RADIUS, painter_bg(painter));
        painter.circle_stroke(node, NODE_RADIUS, Stroke::new(2.0, color));
    } else {
        painter.circle_filled(node, NODE_RADIUS, color);
    }

    // Labels + subject + metadata, clipped to the row.
    let font = FontId::proportional(13.0);
    let small = FontId::proportional(12.0);
    let mut x = lane_x(rect, lane_count) + 2.0;
    let right = rect.right() - 8.0;

    for r in &row.commit.refs {
        if x > right - 60.0 {
            break;
        }
        if let Some(tag) = r.strip_prefix("tag:") {
            x = pill(
                painter,
                x,
                mid,
                &format!("🏷 {}", tag.trim()),
                &[],
                rgb((220, 180, 60)),
                &small,
            ) + 4.0;
            continue;
        }
        let is_head = row.commit.head_ref.as_deref() == Some(r.as_str());
        let color = if is_head {
            HEAD_GOLD
        } else {
            rgb(color_for(branch_family(r, meta.remotes), r))
        };
        let label = if is_head {
            format!("▶ {r}")
        } else {
            r.clone()
        };
        let mut extra = Vec::new();
        if let Some(pr) = pr_for_ref(r, &row.commit.refs, meta.remotes, meta.prs) {
            let (num_color, glyph, glyph_color) = pr_colors(pr);
            extra.push((format!(" #{}", pr.number), num_color));
            if !glyph.is_empty() {
                extra.push((format!(" {glyph}"), glyph_color));
            }
        }
        x = pill(painter, x, mid, &label, &extra, color, &small) + 4.0;
    }

    let meta_text = format!(
        "{}  {}  {}",
        row.commit.short,
        row.commit.author,
        rel_time(row.commit.timestamp)
    );
    let meta_galley = painter.layout_no_wrap(meta_text, small.clone(), DIM);
    let meta_w = meta_galley.size().x;
    let subject_room = (right - x - meta_w - 16.0).max(0.0);

    let subject = painter.layout(row.commit.subject.clone(), font, text, f32::INFINITY);
    let clip = Rect::from_min_max(Pos2::new(x, top), Pos2::new(x + subject_room, bottom));
    painter
        .with_clip_rect(clip.intersect(painter.clip_rect()))
        .galley(
            Pos2::new(x + 4.0, mid - subject.size().y / 2.0),
            subject,
            text,
        );
    if right - meta_w > x + 40.0 {
        painter.galley(
            Pos2::new(right - meta_w, mid - meta_galley.size().y / 2.0),
            meta_galley,
            DIM,
        );
    }
}

fn painter_bg(painter: &egui::Painter) -> Color32 {
    painter.ctx().global_style().visuals.panel_fill
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

/// A rounded ref label. Returns the x where it ends.
fn pill(
    painter: &egui::Painter,
    x: f32,
    mid: f32,
    label: &str,
    extra: &[(String, Color32)],
    color: Color32,
    font: &FontId,
) -> f32 {
    let main = painter.layout_no_wrap(label.to_string(), font.clone(), color);
    let extras: Vec<_> = extra
        .iter()
        .map(|(t, c)| painter.layout_no_wrap(t.clone(), font.clone(), *c))
        .collect();
    let w = main.size().x + extras.iter().map(|g| g.size().x).sum::<f32>();
    let h = main.size().y;
    let rect = Rect::from_min_size(
        Pos2::new(x, mid - h / 2.0 - 2.0),
        Vec2::new(w + 12.0, h + 4.0),
    );
    painter.rect_filled(rect, 5.0, color.gamma_multiply(0.18));
    painter.rect_stroke(
        rect,
        5.0,
        Stroke::new(1.0, color.gamma_multiply(0.6)),
        egui::StrokeKind::Inside,
    );
    let mut tx = x + 6.0;
    let ty = mid - h / 2.0;
    tx += main.size().x;
    painter.galley(Pos2::new(x + 6.0, ty), main, color);
    for g in extras {
        let gw = g.size().x;
        painter.galley(Pos2::new(tx, ty), g, color);
        tx += gw;
    }
    rect.right()
}
