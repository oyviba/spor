use crate::color::{color_for, Rgb};
use crate::git::{Branch, FileStatus, StatusEntry};
use crate::graph::GraphRow;
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
    pub graph_width: u16,    // column split between graph and right pane
    pub diff_split_y: u16,   // row inside the right pane where the diff starts
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

/// Draw a single row of the graph pane.
/// Each lane takes 2 columns: the node/pipe glyph + a space.
pub fn draw_graph_row(
    row: &GraphRow,
    y: u16,
    selected: bool,
    focused: bool,
    max_lanes: usize,
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

    // Draw each active lane.
    let lane_count = row.lanes_before.len().max(row.lanes_after.len()).max(row.lane + 1);
    let lane_count = lane_count.min(max_lanes);

    for l in 0..lane_count {
        let col = (l * 2) as u16;
        move_to(y, col)?;

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
        write!(out, "{glyph}")?;
    }

    // Subject + refs.
    let text_col = (lane_count * 2 + 1) as u16;
    move_to(y, text_col)?;
    reset()?;
    if selected && focused {
        bg((40, 50, 70))?;
    } else if selected {
        bg((30, 30, 40))?;
    }

    // Refs as little labels before the subject.
    for r in &row.commit.refs {
        if r.starts_with("tag:") {
            fg((220, 180, 60))?;
            write!(out, "[{}] ", &r[4..])?;
        } else if row.commit.head_ref.as_deref() == Some(r.as_str()) {
            // Currently checked-out branch — bright gold + arrow marker
            fg((255, 210, 60))?;
            write!(out, "(▶{r}) ")?;
        } else {
            let fam = r.split('/').next().unwrap_or("_");
            fg(color_for(fam, r))?;
            write!(out, "({r}) ")?;
        }
    }

    fg((220, 220, 220))?;
    let subject = truncate(&row.commit.subject, 80);
    write!(out, "{subject}")?;

    reset()?;
    Ok(())
}

pub fn draw_status_pane(
    entries: &[StatusEntry],
    selected: usize,
    focused: bool,
    layout: &Layout,
) -> io::Result<()> {
    let mut out = io::stdout();
    let x0 = layout.graph_width + 1;
    // Don't write past the diff split or off the bottom of the screen.
    let max_rows = layout
        .diff_split_y
        .saturating_sub(2)
        .min(layout.height.saturating_sub(2));

    // Header
    move_to(0, x0)?;
    bold()?;
    fg((180, 200, 255))?;
    write!(out, " Working tree ")?;
    reset()?;

    if entries.is_empty() {
        move_to(2, x0)?;
        fg((120, 160, 120))?;
        write!(out, " clean")?;
        reset()?;
        return Ok(());
    }

    for (i, e) in entries.iter().take(max_rows as usize).enumerate() {
        let y = (i + 2) as u16;
        move_to(y, x0)?;
        if i == selected && focused {
            bg((40, 50, 70))?;
        }
        let (marker, color) = match e.status {
            FileStatus::Staged => ("+", (120, 200, 120)),
            FileStatus::Modified => ("~", (220, 180, 60)),
            FileStatus::Untracked => ("?", (180, 180, 180)),
            FileStatus::Deleted => ("-", (220, 100, 100)),
        };
        fg(color)?;
        write!(out, " {marker} ")?;
        fg((220, 220, 220))?;
        let remaining = layout.width.saturating_sub(x0 + 4) as usize;
        write!(out, "{}", truncate(&e.path, remaining))?;
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

    for (i, line) in diff.lines().skip(scroll).take(max_rows as usize).enumerate() {
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
    move_to(layout.height - 1, 0)?;

    // Build the branch segment text so we know its visible width.
    let branch_label = if tracking.detached {
        "(detached)".to_string()
    } else {
        tracking.branch.clone().unwrap_or_else(|| "?".to_string())
    };

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

    let seg_width = segment.chars().count();

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
    let remaining = layout.width as usize - seg_width;
    let rest = format!(" {msg}  │  {hint}");
    let rest = truncate(&rest, remaining);
    write!(out, "{:<width$}", rest, width = remaining)?;
    reset()?;
    Ok(())
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut r: String = s.chars().take(max.saturating_sub(1)).collect();
        r.push('…');
        r
    }
}

fn pad_right(s: &str, w: usize) -> String {
    let count = s.chars().count();
    if count >= w {
        s.chars().take(w).collect()
    } else {
        let mut r = s.to_string();
        r.push_str(&" ".repeat(w - count));
        r
    }
}

/// Centered modal overlay for branch selection.
pub fn draw_branch_picker(
    layout: &Layout,
    query: &str,
    matches: &[&Branch],
    sel: usize,
) -> io::Result<()> {
    let mut out = io::stdout();
    // Cap to screen, then floor at a usable minimum. If the screen is below
    // the floor, too_small() should have short-circuited render() — but be
    // defensive in case overlays get drawn from somewhere we didn't expect.
    let w = layout.width.min(60).max(layout.width.min(40));
    let h = layout.height.min(18).max(layout.height.min(8));
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
            let fam = b.name.split('/').next().unwrap_or("_");
            fg(color_for(fam, &b.name))?;
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

/// Password/credential modal used while a background git job is waiting on
/// `GIT_ASKPASS` / `SSH_ASKPASS`. `input_len` is the number of characters the
/// user has typed — the actual characters are never passed in so we can't
/// accidentally paint them to the screen.
pub fn draw_askpass(layout: &Layout, prompt: &str, input_len: usize) -> io::Result<()> {
    let mut out = io::stdout();

    // Size: narrow-ish, a few lines tall, centered.
    let w = layout.width.min(64).max(layout.width.min(48));
    let h: u16 = 7;
    if layout.width < 20 || layout.height < h + 2 {
        return Ok(());
    }
    let x = (layout.width.saturating_sub(w)) / 2;
    let y = (layout.height.saturating_sub(h)) / 2;
    let inner = (w - 2) as usize;

    // Top border with title.
    let title = " Credentials ";
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

    // Prompt text (trim trailing ":" / whitespace for display).
    let clean_prompt = prompt.trim().trim_end_matches(':').trim().to_string();
    let shown_prompt = truncate(&clean_prompt, inner.saturating_sub(2));
    move_to(y + 1, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "│ ")?;
    fg((220, 220, 220))?;
    write!(out, "{}", pad_right(&shown_prompt, inner.saturating_sub(2)))?;
    fg((180, 200, 255))?;
    write!(out, " │")?;

    // Blank line.
    move_to(y + 2, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "│{}│", " ".repeat(inner))?;

    // Input row — masked with '•', plus a trailing underscore cursor.
    move_to(y + 3, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "│ ")?;
    fg((120, 200, 120))?;
    write!(out, "> ")?;
    fg((230, 230, 230))?;
    let field_space = inner.saturating_sub(4);
    // Clip the dot count so it never overflows the field.
    let dots = "•".repeat(input_len.min(field_space.saturating_sub(1)));
    let display = format!("{dots}_");
    write!(out, "{}", pad_right(&display, field_space))?;
    fg((180, 200, 255))?;
    write!(out, " │")?;

    // Blank line.
    move_to(y + 4, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "│{}│", " ".repeat(inner))?;

    // Hint row.
    move_to(y + 5, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "│ ")?;
    fg((160, 160, 170))?;
    let hint = "⏎ submit   esc cancel";
    let hint = truncate(hint, inner.saturating_sub(2));
    write!(out, "{}", pad_right(&hint, inner.saturating_sub(2)))?;
    fg((180, 200, 255))?;
    write!(out, " │")?;

    // Bottom border.
    move_to(y + 6, x)?;
    bg((25, 25, 35))?;
    fg((180, 200, 255))?;
    write!(out, "└{}┘", "─".repeat(inner))?;
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
            HelpEntry { keys: "j  ↓",         desc: "move down" },
            HelpEntry { keys: "k  ↑",         desc: "move up" },
            HelpEntry { keys: "Tab",          desc: "switch focus (graph ↔ files)" },
            HelpEntry { keys: "J  K",         desc: "scroll diff" },
        ],
    },
    HelpSection {
        title: "Branches",
        entries: &[
            HelpEntry { keys: "Enter  o",     desc: "checkout branch at selected commit" },
            HelpEntry { keys: "n",            desc: "new branch from selected commit" },
            HelpEntry { keys: "b",            desc: "open branch picker" },
        ],
    },
    HelpSection {
        title: "Working tree (file pane)",
        entries: &[
            HelpEntry { keys: "Space",        desc: "stage / unstage" },
            HelpEntry { keys: "Backspace",    desc: "discard unstaged changes (confirm)" },
            HelpEntry { keys: "Shift-Bksp",   desc: "discard without confirm" },
            HelpEntry { keys: "c",            desc: "commit" },
        ],
    },
    HelpSection {
        title: "Remote",
        entries: &[
            HelpEntry { keys: "P",            desc: "pull (fast-forward only)" },
            HelpEntry { keys: "p",            desc: "push (warns if behind)" },
            HelpEntry { keys: "R",            desc: "open PR / MR for current branch" },
        ],
    },
    HelpSection {
        title: "General",
        entries: &[
            HelpEntry { keys: "r",            desc: "refresh" },
            HelpEntry { keys: "?",            desc: "this help" },
            HelpEntry { keys: "q  Ctrl-C",    desc: "quit" },
        ],
    },
];

pub fn draw_help(layout: &Layout) -> io::Result<()> {
    let mut out = io::stdout();

    // Compute total content height: 1 line per entry + 2 per section (header + blank).
    let total_entries: usize = HELP_SECTIONS.iter().map(|s| s.entries.len()).sum();
    let content_h = total_entries + HELP_SECTIONS.len() * 2;

    let w = layout.width.min(64).max(layout.width.min(48));
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
