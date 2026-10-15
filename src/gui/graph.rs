//! Paints one row of the commit graph: lane lines, curved connectors where a
//! branch forks or merges, and the commit node. Called from a `canvas` inside
//! each History row, so only visible rows are painted.

use gpui_kit::{fill, point, px, size, Bounds, Hsla, PathBuilder, Pixels, Point, Rgba, Window};
use spor::color::color_for;
use spor::graph::GraphRow;

pub const LANE_WIDTH: f32 = 12.0;
const LEFT_PAD: f32 = 12.0;
const NODE_RADIUS: f32 = 3.5;
pub const MAX_LANES: usize = 12;

pub fn width(lanes: usize) -> Pixels {
    px(LEFT_PAD + lanes.clamp(1, MAX_LANES) as f32 * LANE_WIDTH)
}

/// Colors the painter needs from the theme.
#[derive(Clone, Copy)]
pub struct GraphColors {
    /// Row background, for the halo that separates a node from lines
    /// passing behind it.
    pub background: Hsla,
    pub head: Hsla,
    pub dark: bool,
}

fn lane_color(family: &str, name_hint: &str, colors: &GraphColors) -> Hsla {
    let (r, g, b) = color_for(family, name_hint);
    let mut c: Hsla = Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
    .into();
    // The palette is tuned for dark backgrounds; deepen it for light ones.
    if !colors.dark {
        c.l *= 0.8;
    }
    c
}

pub fn paint_row(
    bounds: Bounds<Pixels>,
    row: &GraphRow,
    colors: &GraphColors,
    window: &mut Window,
) {
    let top = bounds.top();
    let bottom = bounds.bottom();
    let mid = bounds.center().y;
    let x = |lane: usize| bounds.left() + px(LEFT_PAD + lane as f32 * LANE_WIDTH);
    let hash = row.commit.hash.as_str();
    let name_hint = row
        .commit
        .refs
        .first()
        .cloned()
        .unwrap_or_else(|| row.commit.hash.clone());
    let color = |lane: usize| {
        let family = if lane == row.lane {
            row.branch_family.as_str()
        } else {
            row.lane_families
                .get(lane)
                .and_then(|f| f.as_deref())
                .unwrap_or("_")
        };
        lane_color(family, &name_hint, colors)
    };

    let lane_count = row
        .lanes_before
        .len()
        .max(row.lanes_after.len())
        .max(row.lane + 1)
        .min(MAX_LANES);
    let node = point(x(row.lane.min(MAX_LANES - 1)), mid);

    for lane in 0..lane_count {
        let lx = x(lane);
        let before = row.lanes_before.get(lane).and_then(|s| s.as_deref());
        let after = row.lanes_after.get(lane).and_then(|s| s.as_deref());
        let c = color(lane);

        if lane == row.lane {
            if before == Some(hash) {
                line(window, point(lx, top), node, c);
            }
            if after.is_some() {
                line(window, node, point(lx, bottom), c);
            }
            continue;
        }

        // Top half: a branch converging on this commit (its fork point)
        // curves in; anything else passing through comes straight down.
        let passing = before.is_some() && before == after;
        if before == Some(hash) {
            curve(window, point(lx, top), node, c);
        } else if passing {
            line(window, point(lx, top), point(lx, bottom), c);
        } else if before.is_some() {
            line(window, point(lx, top), point(lx, mid), c);
        }

        // Bottom half: a merge parent this commit pulls in curves out of the
        // node; a lane that just opened otherwise starts mid-row.
        if let Some(a) = after.filter(|_| !passing) {
            if row.commit.parents.iter().skip(1).any(|p| p == a) {
                curve(window, node, point(lx, bottom), c);
            } else {
                line(window, point(lx, mid), point(lx, bottom), c);
            }
        }
    }

    // Node: a ring for HEAD, hollow for merges, a dot otherwise.
    let c = color(row.lane);
    circle(window, node, NODE_RADIUS + 2.0, colors.background);
    if row.commit.head_ref.is_some() {
        circle(window, node, NODE_RADIUS + 1.0, colors.head);
        circle(window, node, NODE_RADIUS - 1.0, colors.background);
    } else if row.commit.parents.len() > 1 {
        circle(window, node, NODE_RADIUS, c);
        circle(window, node, NODE_RADIUS - 1.5, colors.background);
    } else {
        circle(window, node, NODE_RADIUS, c);
    }
}

fn line(window: &mut Window, from: Point<Pixels>, to: Point<Pixels>, color: Hsla) {
    let mut path = PathBuilder::stroke(px(1.5));
    path.move_to(from);
    path.line_to(to);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn curve(window: &mut Window, from: Point<Pixels>, to: Point<Pixels>, color: Hsla) {
    let dy = (to.y - from.y) * 0.6;
    let mut path = PathBuilder::stroke(px(1.5));
    path.move_to(from);
    path.cubic_bezier_to(to, point(from.x, from.y + dy), point(to.x, to.y - dy));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn circle(window: &mut Window, center: Point<Pixels>, radius: f32, color: Hsla) {
    let r = px(radius);
    window.paint_quad(
        fill(Bounds::centered_at(center, size(r * 2.0, r * 2.0)), color).corner_radii(r),
    );
}
