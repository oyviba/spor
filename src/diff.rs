//! Unified-diff parsing: turns `git diff` / `git show` patch text into files,
//! hunks and numbered lines, so a frontend can draw per-file sections with
//! line-number gutters instead of a wall of raw patch text.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    /// `\ No newline at end of file`
    NoNewline,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    /// Line content without the leading `+`/`-`/space marker.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@` range part.
    pub range: String,
    /// Whatever git put after the range — usually the enclosing function.
    pub section: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileDiff {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub change: ChangeKind,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    pub additions: usize,
    pub deletions: usize,
}

impl FileDiff {
    /// The path to show: the new path, or the old one for deletions.
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or("")
    }

    fn new() -> Self {
        Self {
            old_path: None,
            new_path: None,
            change: ChangeKind::Modified,
            binary: false,
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
        }
    }
}

/// Strip git's `a/` / `b/` prefix and C-style quoting (paths with unusual
/// characters arrive as `"a/odd name"`). `/dev/null` means "no file".
fn clean_path(raw: &str, prefix: &str) -> Option<String> {
    let raw = raw.trim_end_matches('\t').trim();
    if raw == "/dev/null" {
        return None;
    }
    let unquoted = if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        unquote(&raw[1..raw.len() - 1])
    } else {
        raw.to_string()
    };
    Some(
        unquoted
            .strip_prefix(prefix)
            .map(str::to_string)
            .unwrap_or(unquoted),
    )
}

/// Undo git's C-style path quoting: `\"`, `\\`, `\t`, `\n` and octal byte
/// escapes (how non-ASCII bytes are written unless core.quotePath is off).
fn unquote(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut it = s.bytes().peekable();
    while let Some(b) = it.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match it.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(d @ b'0'..=b'7') => {
                let mut v = (d - b'0') as u32;
                for _ in 0..2 {
                    match it.peek() {
                        Some(&n @ b'0'..=b'7') => {
                            v = v * 8 + (n - b'0') as u32;
                            it.next();
                        }
                        _ => break,
                    }
                }
                bytes.push(v as u8);
            }
            Some(other) => bytes.push(other),
            None => bytes.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Parse `-a,b +c,d` into the two starting line numbers.
fn hunk_starts(range: &str) -> (u32, u32) {
    let mut old = 0;
    let mut new = 0;
    for part in range.split_whitespace() {
        let num = |p: &str| {
            p[1..]
                .split(',')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        };
        if part.starts_with('-') {
            old = num(part);
        } else if part.starts_with('+') {
            new = num(part);
        }
    }
    (old, new)
}

/// Parse a patch made of one or more `diff --git` sections. Anything before
/// the first section (a commit header, `--stat` output) is ignored.
pub fn parse(patch: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut in_hunk = false;
    let (mut old_no, mut new_no) = (0u32, 0u32);

    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let mut f = FileDiff::new();
            // Best-effort paths from the header; ---/+++ or rename lines
            // below override them. Ambiguous only when paths contain " b/".
            if let Some((a, b)) = rest.split_once(" b/") {
                f.old_path = clean_path(a, "a/");
                f.new_path = clean_path(b, "");
            }
            files.push(f);
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };

        if let Some(rest) = line.strip_prefix("@@ ") {
            let (range, section) = match rest.split_once(" @@") {
                Some((r, s)) => (r, s.trim()),
                None => (rest, ""),
            };
            (old_no, new_no) = hunk_starts(range);
            file.hunks.push(Hunk {
                range: format!("@@ {range} @@"),
                section: section.to_string(),
                lines: Vec::new(),
            });
            in_hunk = true;
            continue;
        }

        if in_hunk {
            let hunk = file.hunks.last_mut().expect("in_hunk implies a hunk");
            let (kind, text) = match line.as_bytes().first() {
                Some(b'+') => (LineKind::Added, &line[1..]),
                Some(b'-') => (LineKind::Removed, &line[1..]),
                Some(b'\\') => (LineKind::NoNewline, line),
                Some(b' ') => (LineKind::Context, &line[1..]),
                None => (LineKind::Context, ""),
                // Anything else ends the hunk (e.g. the next file's header
                // without a `diff --git` line); fall through to headers.
                Some(_) => {
                    in_hunk = false;
                    (LineKind::Context, "")
                }
            };
            if in_hunk {
                let (o, n) = match kind {
                    LineKind::Added => {
                        file.additions += 1;
                        new_no += 1;
                        (None, Some(new_no - 1))
                    }
                    LineKind::Removed => {
                        file.deletions += 1;
                        old_no += 1;
                        (Some(old_no - 1), None)
                    }
                    LineKind::Context => {
                        old_no += 1;
                        new_no += 1;
                        (Some(old_no - 1), Some(new_no - 1))
                    }
                    LineKind::NoNewline => (None, None),
                };
                hunk.lines.push(DiffLine {
                    kind,
                    old_no: o,
                    new_no: n,
                    text: text.to_string(),
                });
                continue;
            }
        }

        if line.starts_with("new file mode") {
            file.change = ChangeKind::Added;
        } else if line.starts_with("deleted file mode") {
            file.change = ChangeKind::Deleted;
        } else if let Some(p) = line.strip_prefix("rename from ") {
            file.change = ChangeKind::Renamed;
            file.old_path = clean_path(p, "");
        } else if let Some(p) = line.strip_prefix("rename to ") {
            file.change = ChangeKind::Renamed;
            file.new_path = clean_path(p, "");
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            file.binary = true;
        } else if let Some(p) = line.strip_prefix("--- ") {
            file.old_path = clean_path(p, "a/");
            if file.old_path.is_none() {
                file.change = ChangeKind::Added;
            }
        } else if let Some(p) = line.strip_prefix("+++ ") {
            file.new_path = clean_path(p, "b/");
            if file.new_path.is_none() {
                file.change = ChangeKind::Deleted;
            }
        }
    }

    // Deletions/additions told by the header but with paths from `diff --git`
    // keep both paths; drop the side that doesn't exist.
    for f in &mut files {
        match f.change {
            ChangeKind::Added => f.old_path = None,
            ChangeKind::Deleted => f.new_path = None,
            _ => {}
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODIFY: &str = "\
commit abc
Author: x

diff --git a/src/main.rs b/src/main.rs
index 111..222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,4 +10,5 @@ fn main() {
     let a = 1;
-    let b = 2;
+    let b = 3;
+    let c = 4;
     done();
\\ No newline at end of file
";

    #[test]
    fn parses_modified_file_with_numbers() {
        let files = parse(MODIFY);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.path(), "src/main.rs");
        assert_eq!(f.change, ChangeKind::Modified);
        assert_eq!((f.additions, f.deletions), (2, 1));
        let h = &f.hunks[0];
        assert_eq!(h.range, "@@ -10,4 +10,5 @@");
        assert_eq!(h.section, "fn main() {");
        let nums: Vec<_> = h.lines.iter().map(|l| (l.old_no, l.new_no)).collect();
        assert_eq!(
            nums,
            vec![
                (Some(10), Some(10)),
                (Some(11), None),
                (None, Some(11)),
                (None, Some(12)),
                (Some(12), Some(13)),
                (None, None),
            ]
        );
        assert_eq!(h.lines[1].text, "    let b = 2;");
        assert_eq!(h.lines[5].kind, LineKind::NoNewline);
    }

    #[test]
    fn parses_added_deleted_renamed_and_binary() {
        let patch = "\
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..e69de29
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hello
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
diff --git a/a.rs b/b.rs
similarity index 90%
rename from a.rs
rename to b.rs
diff --git a/logo.png b/logo.png
Binary files a/logo.png and b/logo.png differ
";
        let files = parse(patch);
        assert_eq!(files.len(), 4);
        assert_eq!(files[0].change, ChangeKind::Added);
        assert_eq!(files[0].old_path, None);
        assert_eq!(files[0].hunks[0].lines[0].new_no, Some(1));
        assert_eq!(files[1].change, ChangeKind::Deleted);
        assert_eq!(files[1].path(), "old.txt");
        assert_eq!(files[2].change, ChangeKind::Renamed);
        assert_eq!(files[2].old_path.as_deref(), Some("a.rs"));
        assert_eq!(files[2].path(), "b.rs");
        assert!(files[3].binary);
    }

    #[test]
    fn unquotes_paths() {
        let patch = "\
diff --git \"a/odd name.txt\" \"b/odd name.txt\"
--- \"a/odd name.txt\"
+++ \"b/odd name.txt\"
@@ -1 +1 @@
-x
+y
diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"
--- \"a/caf\\303\\251.txt\"
+++ \"b/caf\\303\\251.txt\"
";
        let files = parse(patch);
        assert_eq!(files[0].path(), "odd name.txt");
        assert_eq!(files[1].path(), "café.txt");
    }

    #[test]
    fn empty_patch_has_no_files() {
        assert!(parse("").is_empty());
        assert!(parse("commit abc\n\n    message\n").is_empty());
    }
}
