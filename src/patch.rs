//! Unified-diff patches cut down to some of their lines, so part of a file
//! can be staged, unstaged or discarded with `git apply`.
//!
//! The patch comes from git itself (`git diff -- path`). A selection is a
//! range of source lines on either side of it, the way a diff view reports
//! one; [`FilePatch::select`] keeps the changed lines inside it and turns the
//! rest into context (or drops them), so the result still applies cleanly.

/// Which version a line number refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Before the change: the `-` side.
    Old,
    /// After the change: the `+` side.
    New,
}

/// A one-based line on one side of the patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinePos {
    pub side: Side,
    pub line: usize,
}

impl LinePos {
    pub fn new(side: Side, line: usize) -> Self {
        Self { side, line }
    }
}

/// One `@@` hunk: where it starts on each side, and its lines with their
/// ` `, `+`, `-` or `\` prefix.
#[derive(Clone, Debug, PartialEq)]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<String>,
}

/// One file's part of a patch: the `diff --git` header lines and its hunks.
#[derive(Clone, Debug, PartialEq)]
pub struct FilePatch {
    pub header: Vec<String>,
    pub hunks: Vec<Hunk>,
}

/// Split a patch into files and hunks. Anything before the first
/// `diff --git` line is ignored; binary files come out with no hunks.
pub fn parse(text: &str) -> Vec<FilePatch> {
    let mut files: Vec<FilePatch> = Vec::new();
    for line in text.lines() {
        if line.starts_with("diff --git ") {
            files.push(FilePatch {
                header: vec![line.to_string()],
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if line.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_header(line) {
                file.hunks.push(Hunk {
                    old_start,
                    new_start,
                    lines: Vec::new(),
                });
                continue;
            }
        }
        match file.hunks.last_mut() {
            Some(hunk) if matches!(line.chars().next(), Some(' ' | '+' | '-' | '\\') | None) => {
                // An empty line inside a hunk is an empty context line whose
                // leading space was stripped along the way.
                hunk.lines.push(if line.is_empty() {
                    " ".to_string()
                } else {
                    line.to_string()
                });
            }
            Some(_) => {}
            None => file.header.push(line.to_string()),
        }
    }
    files
}

/// `@@ -12,3 +12,4 @@ fn x()` → (12, 12). A missing count means 1.
fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let mut parts = line.split_whitespace().skip(1);
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let start = |s: &str| s.split(',').next()?.parse::<usize>().ok();
    Some((start(old)?, start(new)?))
}

impl FilePatch {
    /// The file's path after the change, from the `+++ b/…` header (or the
    /// `--- a/…` one for a deletion).
    pub fn path(&self) -> Option<&str> {
        let new = self.header.iter().find_map(|l| l.strip_prefix("+++ b/"));
        new.or_else(|| self.header.iter().find_map(|l| l.strip_prefix("--- a/")))
    }

    /// The whole file's patch, as text `git apply` accepts.
    pub fn to_text(&self) -> String {
        let hunks: Vec<(usize, usize, Vec<String>)> = self
            .hunks
            .iter()
            .map(|h| (h.old_start, h.new_start, h.lines.clone()))
            .collect();
        render(&self.header, &hunks)
    }

    /// A patch holding only the changed lines between `from` and `to`
    /// (inclusive, in patch order). Lines outside the selection are left as
    /// they are on the side the patch applies to: when applying forwards an
    /// unselected `-` line becomes context and an unselected `+` line is
    /// dropped; `reverse` (for `git apply -R`) swaps the two. `None` when
    /// no changed line is selected.
    pub fn select(&self, from: LinePos, to: LinePos, reverse: bool) -> Option<String> {
        let numbered = self.numbered();
        let start = numbered.iter().position(|n| n.at(from))?;
        let end = numbered.iter().rposition(|n| n.at(to))?;
        let (start, end) = (start.min(end), start.max(end));
        Some(self.keep(reverse, |ix| (start..=end).contains(&ix))).filter(|p| !p.is_empty())
    }

    /// A patch holding only hunk `ix`.
    pub fn hunk(&self, ix: usize) -> Option<String> {
        let mut seen = 0;
        let range = self.hunks.iter().enumerate().find_map(|(i, h)| {
            let r = seen..seen + h.lines.len();
            seen += h.lines.len();
            (i == ix).then_some(r)
        })?;
        Some(self.keep(false, |i| range.contains(&i))).filter(|p| !p.is_empty())
    }

    /// Every line of every hunk with its line number on each side.
    fn numbered(&self) -> Vec<Numbered> {
        let mut out = Vec::new();
        for hunk in &self.hunks {
            let (mut old, mut new) = (hunk.old_start, hunk.new_start);
            for line in &hunk.lines {
                let n = match line.as_bytes().first() {
                    Some(b'-') => {
                        old += 1;
                        Numbered {
                            old: Some(old - 1),
                            new: None,
                        }
                    }
                    Some(b'+') => {
                        new += 1;
                        Numbered {
                            old: None,
                            new: Some(new - 1),
                        }
                    }
                    Some(b'\\') => Numbered {
                        old: None,
                        new: None,
                    },
                    _ => {
                        old += 1;
                        new += 1;
                        Numbered {
                            old: Some(old - 1),
                            new: Some(new - 1),
                        }
                    }
                };
                out.push(n);
            }
        }
        out
    }

    /// Rebuild the patch keeping the changed lines `selected` says yes to.
    fn keep(&self, reverse: bool, selected: impl Fn(usize) -> bool) -> String {
        let mut ix = 0;
        let mut hunks = Vec::new();
        for hunk in &self.hunks {
            let mut lines = Vec::new();
            let mut changed = false;
            // Whether the line before a `\ No newline` marker survived.
            let mut kept_previous = false;
            for line in &hunk.lines {
                let take = selected(ix);
                ix += 1;
                let (kind, rest) = line.split_at(1.min(line.len()));
                let out = match (kind, take, reverse) {
                    ("+" | "-", true, _) => {
                        changed = true;
                        Some(line.clone())
                    }
                    ("-", false, false) | ("+", false, true) => Some(format!(" {rest}")),
                    ("-", false, true) | ("+", false, false) => None,
                    ("\\", _, _) => kept_previous.then(|| line.clone()),
                    _ => Some(line.clone()),
                };
                if kind != "\\" {
                    kept_previous = out.is_some();
                }
                lines.extend(out);
            }
            if changed {
                hunks.push((hunk.old_start, hunk.new_start, lines));
            }
        }
        if hunks.is_empty() {
            return String::new();
        }
        render(&self.header, &hunks)
    }
}

struct Numbered {
    old: Option<usize>,
    new: Option<usize>,
}

impl Numbered {
    fn at(&self, pos: LinePos) -> bool {
        match pos.side {
            Side::Old => self.old == Some(pos.line),
            Side::New => self.new == Some(pos.line),
        }
    }
}

/// Header plus hunks, with each hunk's counts recomputed from its lines.
fn render(header: &[String], hunks: &[(usize, usize, Vec<String>)]) -> String {
    let mut out = String::new();
    for line in header {
        out.push_str(line);
        out.push('\n');
    }
    for (old_start, new_start, lines) in hunks {
        let count = |c: u8| {
            lines
                .iter()
                .filter(|l| matches!(l.as_bytes().first(), Some(&b) if b == c || b == b' '))
                .count()
        };
        let (old_count, new_count) = (count(b'-'), count(b'+'));
        out.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "\
diff --git a/f.txt b/f.txt
index 111..222 100644
--- a/f.txt
+++ b/f.txt
@@ -1,4 +1,4 @@
 one
-two
+TWO
 three
-four
+FOUR
@@ -10,2 +10,3 @@ ctx
 ten
+ten and a half
 eleven
";

    #[test]
    fn parses_files_and_hunks() {
        let files = parse(PATCH);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path(), Some("f.txt"));
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!(files[0].hunks[1].old_start, 10);
        assert_eq!(files[0].hunks[0].lines.len(), 6);
    }

    #[test]
    fn round_trips() {
        let file = &parse(PATCH)[0];
        assert_eq!(file.to_text(), PATCH.replace(" @@ ctx", " @@"));
    }

    #[test]
    fn select_new_side_keeps_only_those_lines() {
        let file = &parse(PATCH)[0];
        // Only the addition of "TWO": the deletion of "two" stays context.
        let p = file
            .select(
                LinePos::new(Side::New, 2),
                LinePos::new(Side::New, 2),
                false,
            )
            .unwrap();
        assert_eq!(
            p,
            "diff --git a/f.txt b/f.txt\nindex 111..222 100644\n--- a/f.txt\n+++ b/f.txt\n\
             @@ -1,4 +1,5 @@\n one\n two\n+TWO\n three\n four\n"
        );
    }

    #[test]
    fn select_spanning_sides_takes_the_pair() {
        let file = &parse(PATCH)[0];
        let p = file
            .select(
                LinePos::new(Side::Old, 2),
                LinePos::new(Side::New, 2),
                false,
            )
            .unwrap();
        assert!(p.contains("-two\n+TWO\n three\n four\n"));
        assert!(!p.contains("ten and a half"));
    }

    #[test]
    fn select_reverse_keeps_new_side_intact() {
        let file = &parse(PATCH)[0];
        let p = file
            .select(LinePos::new(Side::Old, 4), LinePos::new(Side::New, 4), true)
            .unwrap();
        // Unselected "-two" dropped, unselected "+TWO" kept as context.
        assert!(p.contains("@@ -1,4 +1,4 @@\n one\n TWO\n three\n-four\n+FOUR\n"));
    }

    #[test]
    fn select_nothing_changed_is_none() {
        let file = &parse(PATCH)[0];
        assert_eq!(
            file.select(
                LinePos::new(Side::New, 1),
                LinePos::new(Side::New, 1),
                false
            ),
            None
        );
    }

    #[test]
    fn hunk_by_index() {
        let file = &parse(PATCH)[0];
        let p = file.hunk(1).unwrap();
        assert!(p.contains("@@ -10,2 +10,3 @@\n ten\n+ten and a half\n eleven\n"));
        assert!(!p.contains("TWO"));
    }

    #[test]
    fn no_newline_marker_follows_its_line() {
        let patch = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
        let file = &parse(patch)[0];
        let p = file
            .select(
                LinePos::new(Side::Old, 1),
                LinePos::new(Side::Old, 1),
                false,
            )
            .unwrap();
        // "+new" is dropped, and its marker with it.
        assert!(p.ends_with("@@ -1,1 +1,0 @@\n-old\n\\ No newline at end of file\n"));
    }
}
