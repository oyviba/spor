use crate::color::{branch_family, color_for, Rgb};
use crate::git::{Branch, FileStatus, StatusEntry};
use crate::graph::GraphRow;
use crate::remote::{ChecksState, PrInfo, ReviewState};
use std::collections::HashMap;
use std::io::{self, Write};

// ANSI helpers. Direct CSI sequences keep us off ratatui.

pub fn clear() -> io::Result<()> {
    write!(io::stdout(), "\x1b[2J\x1b[H")?;
    Ok(())
}

pub fn move_to(row: u16, col: u16) -> io::Result<()> {
    // ANSI is 1-indexed.
    write!(io::stdout(), "\x1b[{};{}H", row + 1, col + 1)
}

pub fn fg(c: Rgb) -> io::Result<()> {
    write!(io::stdout(), "\x1b[38;2;{};{};{}m", c.0, c.1, c.2)
}

pub fn bg(c: Rgb) -> io::Result<()> {
    write!(io::stdout(), "\x1b[48;2;{};{};{}m", c.0, c.1, c.2)
}

pub fn reset() -> io::Result<()> {
    write!(io::stdout(), "\x1b[0m")
}

pub fn bold() -> io::Result<()> {
    write!(io::stdout(), "\x1b[1m")
}

pub fn hide_cursor() -> io::Result<()> {
    write!(io::stdout(), "\x1b[?25l")
}

pub fn show_cursor() -> io::Result<()> {
    write!(io::stdout(), "\x1b[?25h")
}

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    Graph,
    Status,
}

pub struct Layout {
    pub width: u16,
    pub height: u16,
    pub graph_width: u16,  // column split between graph and right pane
    pub diff_split_y: u16, // row inside the right pane where the diff starts
}

/// Below this we just paint a "terminal too small" message instead of trying
/// to lay anything out.
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 12;

impl Layout {
    pub fn new(w: u16, h: u16) -> Self {
        // Graph gets ~55% of width, but always leave at least 30 cols on the right
        // and at least 30 cols for the graph itself. On very narrow terminals,
        // `too_small()` short-circuits before this even matters.
        let target = (w as f32 * 0.55) as u16;
        let max_graph = w.saturating_sub(30);
        let graph_width = target.min(max_graph).max(30);

        // Status list takes up to ~⅓ of the height (cap at 10 rows), diff takes
        // the rest. This used to be a hardcoded 12 — bad on short terminals.
        let status_rows = ((h as f32 * 0.33) as u16).clamp(4, 10);
        let diff_split_y = status_rows + 2; // +2 for header and one blank line

        Self {
            width: w,
            height: h,
            graph_width,
            diff_split_y,
        }
    }

    pub fn too_small(&self) -> bool {
        self.width < MIN_WIDTH || self.height < MIN_HEIGHT
    }
}

/// Painted when the terminal is below MIN_WIDTH/MIN_HEIGHT. Rendering normal
/// content into a tiny window panics on out-of-range cursor moves and produces
/// garbage anyway.
pub fn draw_too_small(layout: &Layout) -> io::Result<()> {
    let mut out = io::stdout();
    clear()?;
    if layout.width == 0 || layout.height == 0 {
        return Ok(());
    }
    let msg = format!(
        "spor needs at least {}×{} (currently {}×{})",
        MIN_WIDTH, MIN_HEIGHT, layout.width, layout.height
    );
    let msg = truncate(&msg, layout.width as usize);
    let y = layout.height / 2;
    let x = (layout.width.saturating_sub(msg.chars().count() as u16)) / 2;
    move_to(y, x)?;
    fg((220, 180, 80))?;
    write!(out, "{msg}")?;
    reset()?;
    Ok(())
}

/// How old is a commit, compressed to something like "3d" or "2mo".
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

/// Repo-level context the graph rows need to label refs: remote names for
/// prefix-stripping, and open PRs keyed by head branch for the tip badges.
pub struct RepoMeta<'a> {
    pub remotes: &'a [String],
    pub prs: &'a HashMap<String, PrInfo>,
}

/// Find the open PR whose head is this ref, if the badge belongs on it.
/// A remote-tracking ref defers to its local twin on the same commit, so
/// `(feat/x #12✓) (origin/feat/x)` doesn't show the badge twice.
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
                return None; // local twin carries the badge
            }
            rest
        }
        _ => r,
    };
    prs.get(name)
}

/// The ` #42✓` suffix for a branch label: PR number colored by review state,
/// check glyph colored by CI rollup.
fn pr_badge(pr: &PrInfo) -> (String, Rgb, &'static str, Rgb) {
    let num_color = if pr.draft {
        (110, 110, 125) // drafts stay dim regardless of review state
    } else {
        match pr.review {
            ReviewState::Approved => (120, 200, 120),
            ReviewState::ChangesRequested => (220, 100, 100),
            ReviewState::None => (180, 200, 255),
        }
    };
    let (glyph, glyph_color) = match pr.checks {
        ChecksState::Passing => ("✓", (120, 200, 120)),
        ChecksState::Failing => ("✗", (220, 100, 100)),
        ChecksState::Pending => ("●", (220, 180, 60)),
        ChecksState::None => ("", (0, 0, 0)),
    };
    (format!(" #{}", pr.number), num_color, glyph, glyph_color)
}

/// Draw a single row of the graph pane, clamped to `graph_width` columns.
/// Each lane takes 2 columns: the node/pipe glyph + a space.
pub fn draw_graph_row(
    row: &GraphRow,
    y: u16,
    selected: bool,
    focused: bool,
    max_lanes: usize,
    graph_width: u16,
    meta: &RepoMeta,
) -> io::Result<()> {
    let mut out = io::stdout();
    move_to(y, 0)?;
    if selected {
        if focused {
            bg((40, 50, 70))?;
        } else {
            bg((30, 30, 40))?;
        }
    }

    // Draw each active lane. Writing "glyph + space" sequentially (instead of
    // jumping the cursor per lane) keeps the selection background continuous.
    let lane_count = row
        .lanes_before
        .len()
        .max(row.lanes_after.len())
        .max(row.lane + 1);
    let lane_count = lane_count.min(max_lanes);

    for l in 0..lane_count {
        let family: &str = if l == row.lane {
            &row.branch_family
        } else {
            row.lane_families
                .get(l)
                .and_then(|f| f.as_deref())
                .unwrap_or("_")
        };
        let name_hint = row
            .commit
            .refs
            .first()
            .cloned()
            .unwrap_or_else(|| row.commit.hash.clone());

        let glyph = if l == row.lane {
            if row.commit.head_ref.is_some() {
                // HEAD commit — gold ring node
                fg((255, 210, 60))?;
                "◉"
            } else if row.commit.parents.len() > 1 {
                // Merge commit — diamond
                fg(color_for(family, &name_hint))?;
                "◆"
            } else {
                fg(color_for(family, &name_hint))?;
                "●"
            }
        } else if row.lanes_before.get(l).and_then(|x| x.as_ref()).is_some()
            && row.lanes_after.get(l).and_then(|x| x.as_ref()).is_some()
        {
            fg(color_for(family, &name_hint))?;
            "│"
        } else if row.lanes_before.get(l).and_then(|x| x.as_ref()).is_some() {
            fg(color_for(family, &name_hint))?;
            "╵"
        } else if row.lanes_after.get(l).and_then(|x| x.as_ref()).is_some() {
            fg(color_for(family, &name_hint))?;
            "╷"
        } else {
            " "
        };
        write!(out, "{glyph} ")?;
    }

    // Refs + subject + metadata, budgeted so nothing bleeds into the right pane.
    let text_col = lane_count * 2;
    let mut avail = (graph_width as usize).saturating_sub(text_col);

    // Refs as little labels before the subject.
    for r in &row.commit.refs {
        if let Some(tag) = r.strip_prefix("tag:") {
            let label = format!("[{tag}] ");
            if display_width(&label) + 2 > avail {
                break; // no room — drop remaining labels rather than overflow
            }
            fg((220, 180, 60))?;
            avail -= display_width(&label);
            write!(out, "{label}")?;
            continue;
        }

        let is_head = row.commit.head_ref.as_deref() == Some(r.as_str());
        let open = if is_head { "(▶" } else { "(" };
        let pr = pr_for_ref(r, &row.commit.refs, meta.remotes, meta.prs);
        let badge = pr.map(pr_badge);

        let badge_width = badge
            .as_ref()
            .map(|(num, _, glyph, _)| display_width(num) + display_width(glyph))
            .unwrap_or(0);
        let width = display_width(open) + display_width(r) + badge_width + 2; // ") "
        if width + 2 > avail {
            break; // no room — drop remaining labels rather than overflow
        }

        let ref_color = if is_head {
            // Currently checked-out branch — bright gold + arrow marker
            (255, 210, 60)
        } else {
            color_for(branch_family(r, meta.remotes), r)
        };
        fg(ref_color)?;
        write!(out, "{open}{r}")?;
        if let Some((num, num_color, glyph, glyph_color)) = badge {
            fg(num_color)?;
            write!(out, "{num}")?;
            if !glyph.is_empty() {
                fg(glyph_color)?;
                write!(out, "{glyph}")?;
            }
            fg(ref_color)?;
        }
        write!(out, ") ")?;
        avail -= width;
    }

    fg((220, 220, 220))?;
    let subject = truncate(&row.commit.subject, avail);
    avail -= display_width(&subject);
    write!(out, "{subject}")?;

    // If there's still room, tack on dim "hash author age" metadata.
    let meta = format!(
        "  {} {} {}",
        row.commit.short,
        row.commit.author,
        rel_time(row.commit.timestamp)
    );
    if display_width(&meta) <= avail {
        fg((110, 110, 125))?;
        avail -= display_width(&meta);
        write!(out, "{meta}")?;
    }

    // Extend the selection highlight across the whole pane.
    if selected && avail > 0 {
        write!(out, "{}", " ".repeat(avail))?;
    }

    reset()?;
    Ok(())
}

/// How many file rows fit in the status pane. Shared with the scroll logic in
/// main.rs so selection and rendering agree on visibility.
pub fn status_pane_rows(layout: &Layout) -> usize {
    layout
        .diff_split_y
        .saturating_sub(2)
        .min(layout.height.saturating_sub(2)) as usize
}

pub fn draw_status_pane(
    entries: &[StatusEntry],
    selected: usize,
    scroll: usize,
    focused: bool,
    layout: &Layout,
) -> io::Result<()> {
    let mut out = io::stdout();
    let x0 = layout.graph_width + 1;
    let max_rows = status_pane_rows(layout);

    // Header, with a position indicator when the list doesn't fit.
    move_to(0, x0)?;
    bold()?;
    fg((180, 200, 255))?;
    write!(out, " Working tree ")?;
    reset()?;
    if entries.len() > max_rows {
        fg((140, 140, 150))?;
        write!(out, "({}/{})", selected + 1, entries.len())?;
        reset()?;
    }

    if entries.is_empty() {
        move_to(2, x0)?;
        fg((120, 160, 120))?;
        write!(out, " clean")?;
        reset()?;
        return Ok(());
    }

    for (i, e) in entries.iter().skip(scroll).take(max_rows).enumerate() {
        let idx = scroll + i;
        let y = (i + 2) as u16;
        move_to(y, x0)?;
        if idx == selected && focused {
            bg((40, 50, 70))?;
        }
        let (marker, color) = match e.status {
            FileStatus::Staged => ("+", (120, 200, 120)),
            FileStatus::StagedDeleted => ("-", (120, 200, 120)),
            FileStatus::Modified => ("~", (220, 180, 60)),
            FileStatus::Untracked => ("?", (180, 180, 180)),
            FileStatus::Deleted => ("-", (220, 100, 100)),
        };
        fg(color)?;
        write!(out, " {marker} ")?;
        fg((220, 220, 220))?;
        let remaining = layout.width.saturating_sub(x0 + 4) as usize;
        let label = match &e.orig_path {
            Some(orig) => format!("{orig} → {}", e.path),
            None => e.path.clone(),
        };
        write!(out, "{}", truncate(&label, remaining))?;
        reset()?;
    }
    Ok(())
}

pub fn draw_diff_pane(diff: &str, layout: &Layout, scroll: usize) -> io::Result<()> {
    let mut out = io::stdout();
    let x0 = layout.graph_width + 1;
    let y0 = layout.diff_split_y;

    // Need at least the header row + 1 content row + status bar.
    if y0 + 2 >= layout.height {
        return Ok(());
    }

    move_to(y0.saturating_sub(1), x0)?;
    bold()?;
    fg((180, 200, 255))?;
    write!(out, " Diff ")?;
    reset()?;

    let max_rows = layout.height.saturating_sub(y0 + 1);
    let max_cols = layout.width.saturating_sub(x0 + 1) as usize;

    for (i, line) in diff
        .lines()
        .skip(scroll)
        .take(max_rows as usize)
        .enumerate()
    {
        let y = y0 + i as u16;
        move_to(y, x0)?;
        let color = if line.starts_with("+++") || line.starts_with("---") {
            (180, 180, 180)
        } else if line.starts_with('+') {
            (120, 200, 120)
        } else if line.starts_with('-') {
            (220, 100, 100)
        } else if line.starts_with("@@") {
            (180, 200, 255)
        } else {
            (180, 180, 180)
        };
        fg(color)?;
        write!(out, "{}", truncate(line, max_cols))?;
        reset()?;
    }
    Ok(())
}

pub fn draw_statusbar(
    layout: &Layout,
    tracking: &crate::git::TrackingInfo,
    msg: &str,
    hint: &str,
) -> io::Result<()> {
    let mut out = io::stdout();
    move_to(layout.height.saturating_sub(1), 0)?;

    // Build the branch segment text so we know its visible width. Long branch
    // names get truncated so the segment can never eat the whole bar (or
    // underflow the width math below).
    let branch_label = if tracking.detached {
        "(detached)".to_string()
    } else {
        tracking.branch.clone().unwrap_or_else(|| "?".to_string())
    };
    let branch_label = truncate(&branch_label, (layout.width / 2) as usize);

    // "⎇" is the branch symbol (U+2387), widely supported.
    let mut segment = format!(" ⎇ {branch_label} ");

    if tracking.upstream.is_some() {
        if tracking.ahead > 0 {
            segment.push_str(&format!("↑{} ", tracking.ahead));
        }
        if tracking.behind > 0 {
            segment.push_str(&format!("↓{} ", tracking.behind));
        }
        if tracking.ahead == 0 && tracking.behind == 0 {
            segment.push_str("= ");
        }
    } else if !tracking.detached {
        segment.push_str("(no upstream) ");
    }

    let seg_width = display_width(&segment);

    // Draw branch segment with a distinct background.
    bg((50, 50, 65))?;
    // Render it piece-by-piece for color. Simplest: write the whole segment with
    // inline color escapes, since escapes don't consume visible columns.
    write!(out, " ")?;
    fg((180, 200, 255))?;
    write!(out, "⎇ ")?;
    fg((220, 220, 220))?;
    write!(out, "{branch_label} ")?;

    if tracking.upstream.is_some() {
        if tracking.ahead > 0 {
            fg((120, 200, 120))?;
            write!(out, "↑{} ", tracking.ahead)?;
        }
        if tracking.behind > 0 {
            fg((220, 180, 80))?;
            write!(out, "↓{} ", tracking.behind)?;
        }
        if tracking.ahead == 0 && tracking.behind == 0 {
            fg((140, 140, 150))?;
            write!(out, "= ")?;
        }
    } else if !tracking.detached {
        fg((160, 120, 120))?;
        write!(out, "(no upstream) ")?;
    }

    // Rest of the bar.
    bg((30, 30, 40))?;
    fg((200, 200, 200))?;
    let remaining = (layout.width as usize).saturating_sub(seg_width);
    let rest = format!(" {msg}  │  {hint}");
    write!(out, "{}", pad_right(&rest, remaining))?;
    reset()?;
    Ok(())
}

/// Approximate terminal cell width of a char. Covers the common wide ranges
/// (CJK, Hangul, kana, fullwidth forms, emoji) and zero-width combining marks;
/// not a full Unicode east-asian-width implementation, but keeps columns from
/// drifting on the inputs a git TUI actually sees.
fn char_width(c: char) -> usize {
    let cp = c as u32;
    match cp {
        // Combining marks render as zero cells.
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F => 0,
        // Wide: CJK & friends, Hangul, kana, fullwidth forms, emoji.
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F680..=0x1F6FF
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x2FFFD => 2,
        _ => 1,
    }
}

pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Cut `s` to at most `max` terminal cells, appending `…` when it was longer.
pub fn truncate(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let budget = max - 1; // room for the ellipsis
    let mut used = 0;
    let mut r = String::new();
    for c in s.chars() {
        let w = char_width(c);
        if used + w > budget {
            break;
        }
        used += w;
        r.push(c);
    }
    r.push('…');
    r
}

fn pad_right(s: &str, w: usize) -> String {
    let width = display_width(s);
    if width > w {
        truncate(s, w)
    } else {
        let mut r = s.to_string();
        r.push_str(&" ".repeat(w - width));
        r
    }
}

/// Centered modal overlay for branch selection.
pub fn draw_branch_picker(
    layout: &Layout,
    query: &str,
    matches: &[&Branch],
    sel: usize,
    remotes: &[String],
) -> io::Result<()> {
    let mut out = io::stdout();
    // Cap to the screen; too_small() has already guaranteed a usable minimum.
    let w = layout.width.min(60);
    let h = layout.height.min(18);
    let x = (layout.width.saturating_sub(w)) / 2;
    let y = (layout.height.saturating_sub(h)) / 2;
    let inner = (w - 2) as usize;

    // Top border with title
    let title = " Switch branch ";
    let title_len = title.chars().count();
    let dash_count = inner.saturating_sub(title_len);
    let left_dash = dash_count / 2;
    let right_dash = dash_count - left_dash;
    move_to(y, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "┌{}", "─".repeat(left_dash))?;
    bold()?;
    write!(out, "{title}")?;
    reset()?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "{}┐", "─".repeat(right_dash))?;

    // Query row
    move_to(y + 1, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "│ ")?;
    fg((120, 200, 120))?;
    write!(out, "> ")?;
    fg((230, 230, 230))?;
    let q_display = format!("{query}_");
    let q_space = inner.saturating_sub(3);
    write!(out, "{}", pad_right(&q_display, q_space))?;
    fg((180, 200, 255))?;
    write!(out, "│")?;

    // Separator
    move_to(y + 2, x)?;
    fg((180, 200, 255))?;
    write!(out, "├{}┤", "─".repeat(inner))?;

    // Match rows
    let list_rows = (h as usize).saturating_sub(4);
    for i in 0..list_rows {
        move_to(y + 3 + i as u16, x)?;
        bg((25, 25, 35))?;
        fg((180, 200, 255))?;
        write!(out, "│")?;

        if let Some(b) = matches.get(i) {
            let is_selected = i == sel;
            if is_selected {
                bg((40, 50, 70))?;
            }
            // Selection cursor
            fg((180, 200, 255))?;
            write!(out, "{}", if is_selected { "▎" } else { " " })?;
            // Current-branch marker
            fg((120, 200, 120))?;
            write!(out, "{} ", if b.is_current { "●" } else { " " })?;
            // Branch name, colored by family
            fg(color_for(branch_family(&b.name, remotes), &b.name))?;
            let content_width = inner.saturating_sub(4);
            let label = if b.is_remote {
                format!("{} (remote)", b.name)
            } else {
                b.name.clone()
            };
            write!(out, "{}", pad_right(&label, content_width))?;
        } else {
            write!(out, "{}", " ".repeat(inner))?;
        }

        bg((25, 25, 35))?;
        fg((180, 200, 255))?;
        write!(out, "│")?;
    }

    // Bottom border + hint
    move_to(y + h - 1, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    let hint = " ↑↓ select  ⏎ checkout  esc cancel ";
    let hint_len = hint.chars().count();
    let pad_total = inner.saturating_sub(hint_len);
    let pad_left = pad_total / 2;
    let pad_right_n = pad_total - pad_left;
    write!(
        out,
        "└{}{}{}┘",
        "─".repeat(pad_left),
        hint,
        "─".repeat(pad_right_n)
    )?;
    reset()?;
    Ok(())
}

/// Help overlay — keep this in sync with the actual key handlers.
/// Sections appear with bold headers; keys are right-padded to a column
/// so the descriptions line up.
struct HelpEntry {
    keys: &'static str,
    desc: &'static str,
}

struct HelpSection {
    title: &'static str,
    entries: &'static [HelpEntry],
}

const HELP_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Navigation",
        entries: &[
            HelpEntry {
                keys: "j  ↓",
                desc: "move down",
            },
            HelpEntry {
                keys: "k  ↑",
                desc: "move up",
            },
            HelpEntry {
                keys: "Tab",
                desc: "switch focus (graph ↔ files)",
            },
            HelpEntry {
                keys: "J  K",
                desc: "scroll diff",
            },
        ],
    },
    HelpSection {
        title: "Branches",
        entries: &[
            HelpEntry {
                keys: "Enter  o",
                desc: "checkout branch at selected commit",
            },
            HelpEntry {
                keys: "n",
                desc: "new branch from selected commit",
            },
            HelpEntry {
                keys: "b",
                desc: "open branch picker",
            },
        ],
    },
    HelpSection {
        title: "Working tree (file pane)",
        entries: &[
            HelpEntry {
                keys: "Space",
                desc: "stage / unstage",
            },
            HelpEntry {
                keys: "Backspace",
                desc: "discard unstaged changes (confirm)",
            },
            HelpEntry {
                keys: "Shift-Bksp",
                desc: "discard without confirm (if terminal sends it)",
            },
            HelpEntry {
                keys: "c",
                desc: "commit",
            },
        ],
    },
    HelpSection {
        title: "Remote",
        entries: &[
            HelpEntry {
                keys: "P",
                desc: "pull (fast-forward only)",
            },
            HelpEntry {
                keys: "p",
                desc: "push (confirms if behind)",
            },
            HelpEntry {
                keys: "R",
                desc: "open PR / MR for current branch",
            },
        ],
    },
    HelpSection {
        title: "General",
        entries: &[
            HelpEntry {
                keys: "r",
                desc: "refresh",
            },
            HelpEntry {
                keys: "?",
                desc: "this help",
            },
            HelpEntry {
                keys: "q  Ctrl-C",
                desc: "quit",
            },
        ],
    },
];

pub fn draw_help(layout: &Layout) -> io::Result<()> {
    let mut out = io::stdout();

    // Compute total content height: 1 line per entry + 2 per section (header + blank).
    let total_entries: usize = HELP_SECTIONS.iter().map(|s| s.entries.len()).sum();
    let content_h = total_entries + HELP_SECTIONS.len() * 2;

    let w = layout.width.min(64);
    let h = ((content_h + 4) as u16).min(layout.height.saturating_sub(2));
    let x = (layout.width.saturating_sub(w)) / 2;
    let y = (layout.height.saturating_sub(h)) / 2;
    let inner = (w - 2) as usize;
    let key_col = 16; // width reserved for the key column

    // Title bar
    let title = " Keyboard shortcuts ";
    let title_len = title.chars().count();
    let dash_count = inner.saturating_sub(title_len);
    let left_dash = dash_count / 2;
    let right_dash = dash_count - left_dash;
    move_to(y, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "┌{}", "─".repeat(left_dash))?;
    bold()?;
    write!(out, "{title}")?;
    reset()?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "{}┐", "─".repeat(right_dash))?;

    // Body
    let mut row = 1u16;
    for section in HELP_SECTIONS {
        if row + 1 >= h {
            break;
        }
        // Section header
        move_to(y + row, x)?;
        bg((25, 25, 35))?;
        fg((180, 200, 255))?;
        write!(out, "│ ")?;
        bold()?;
        fg((140, 200, 255))?;
        let header = section.title;
        let header_len = header.chars().count();
        write!(out, "{header}")?;
        reset()?;
        bg((25, 25, 35))?;
        // Pad to inner width minus the leading "│ "
        let pad = inner.saturating_sub(header_len + 1);
        write!(out, "{}", " ".repeat(pad))?;
        fg((180, 200, 255))?;
        write!(out, "│")?;
        row += 1;

        // Entries
        for entry in section.entries {
            if row + 1 >= h {
                break;
            }
            move_to(y + row, x)?;
            bg((25, 25, 35))?;
            fg((180, 200, 255))?;
            write!(out, "│ ")?;
            // Key column (cyan-ish, dim bold)
            fg((180, 220, 180))?;
            let keys = pad_right(entry.keys, key_col);
            write!(out, "{keys}")?;
            // Description
            fg((220, 220, 220))?;
            let desc_space = inner.saturating_sub(key_col + 2);
            let desc = pad_right(entry.desc, desc_space);
            write!(out, "{desc}")?;
            fg((180, 200, 255))?;
            write!(out, "│")?;
            row += 1;
        }

        // Blank line between sections
        if row + 1 < h {
            move_to(y + row, x)?;
            bg((25, 25, 35))?;
            fg((180, 200, 255))?;
            write!(out, "│{}│", " ".repeat(inner))?;
            row += 1;
        }
    }

    // Fill any remaining rows so the box is rectangular even if content is short.
    while row + 1 < h {
        move_to(y + row, x)?;
        bg((25, 25, 35))?;
        fg((180, 200, 255))?;
        write!(out, "│{}│", " ".repeat(inner))?;
        row += 1;
    }

    // Bottom border with hint
    move_to(y + h - 1, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    let hint = " any key to dismiss ";
    let hint_len = hint.chars().count();
    let pad_total = inner.saturating_sub(hint_len);
    let pad_left = pad_total / 2;
    let pad_right_n = pad_total - pad_left;
    write!(
        out,
        "└{}{}{}┘",
        "─".repeat(pad_left),
        hint,
        "─".repeat(pad_right_n)
    )?;
    reset()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_adds_ellipsis() {
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(truncate("hi", 0), "");
    }

    #[test]
    fn truncate_counts_wide_chars_as_two_cells() {
        // Each CJK char is 2 cells; "日本語" is 6 cells wide.
        assert_eq!(display_width("日本語"), 6);
        assert_eq!(truncate("日本語", 6), "日本語");
        // Budget 5 → 4 cells of content (2 chars) + ellipsis.
        assert_eq!(truncate("日本語", 5), "日本…");
    }

    #[test]
    fn combining_marks_are_zero_width() {
        assert_eq!(display_width("e\u{0301}"), 1); // e + combining acute
    }

    #[test]
    fn pad_right_pads_to_display_width() {
        assert_eq!(pad_right("ab", 4), "ab  ");
        assert_eq!(pad_right("日本", 5), "日本 ");
        assert_eq!(pad_right("too long here", 7), "too lo…");
    }

    fn pr_map(branches: &[&str]) -> HashMap<String, PrInfo> {
        branches
            .iter()
            .enumerate()
            .map(|(i, b)| {
                (
                    b.to_string(),
                    PrInfo {
                        number: i as u64 + 1,
                        head_branch: b.to_string(),
                        draft: false,
                        review: ReviewState::None,
                        checks: ChecksState::None,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn pr_badge_on_local_ref() {
        let prs = pr_map(&["feat/x"]);
        let refs = vec!["feat/x".to_string()];
        let remotes = vec!["origin".to_string()];
        assert!(pr_for_ref("feat/x", &refs, &remotes, &prs).is_some());
        assert!(pr_for_ref("feat/other", &refs, &remotes, &prs).is_none());
    }

    #[test]
    fn pr_badge_on_remote_ref_when_no_local_twin() {
        let prs = pr_map(&["feat/x"]);
        let refs = vec!["origin/feat/x".to_string()];
        let remotes = vec!["origin".to_string()];
        assert!(pr_for_ref("origin/feat/x", &refs, &remotes, &prs).is_some());
    }

    #[test]
    fn remote_ref_defers_to_local_twin_on_same_commit() {
        let prs = pr_map(&["feat/x"]);
        let refs = vec!["feat/x".to_string(), "origin/feat/x".to_string()];
        let remotes = vec!["origin".to_string()];
        assert!(pr_for_ref("feat/x", &refs, &remotes, &prs).is_some());
        assert!(pr_for_ref("origin/feat/x", &refs, &remotes, &prs).is_none());
    }

    #[test]
    fn slashed_branch_without_remote_prefix_matches() {
        // "feat/x" starts with "feat/", but "feat" isn't a remote — the whole
        // name must be used for the PR lookup even with no local twin check.
        let prs = pr_map(&["feat/x"]);
        let refs = vec!["feat/x".to_string()];
        assert!(pr_for_ref("feat/x", &refs, &[], &prs).is_some());
    }

    #[test]
    fn tags_never_get_badges() {
        let prs = pr_map(&["v1"]);
        let refs = vec!["tag:v1".to_string()];
        assert!(pr_for_ref("tag:v1", &refs, &[], &prs).is_none());
    }

    #[test]
    fn rel_time_buckets() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert_eq!(rel_time(now - 30), "30s");
        assert_eq!(rel_time(now - 120), "2m");
        assert_eq!(rel_time(now - 7200), "2h");
        assert_eq!(rel_time(now - 3 * 86_400), "3d");
    }
}
