//! A conflicted file as git leaves it — text, and regions between conflict
//! markers — and the file rebuilt from a choice per region.
//!
//! ```text
//! <<<<<<< HEAD
//! ours
//! ||||||| base        (only with merge.conflictStyle diff3 / zdiff3)
//! the common ancestor
//! =======
//! theirs
//! >>>>>>> feature
//! ```
//!
//! Lines keep their own endings, so a CRLF file stays CRLF.

/// One piece of a conflicted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Lines both sides agree on (or that only one side changed).
    Text(String),
    Conflict(Region),
}

/// The two (or three) versions of one conflicting stretch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// The checked-out side, and what git labelled it (`HEAD`, a commit…).
    pub ours: String,
    pub ours_label: String,
    /// The side being brought in.
    pub theirs: String,
    pub theirs_label: String,
    /// The common ancestor, when git wrote it (diff3 / zdiff3 style).
    pub base: Option<String>,
}

/// What to keep for one region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Ours,
    Theirs,
    /// Ours, then theirs.
    Both,
    /// Theirs, then ours.
    BothTheirsFirst,
}

/// A marker line: seven of `ch`, then the end of the line or a space and a
/// label. Returns the label.
fn marker(line: &str, ch: char) -> Option<String> {
    let body = line.trim_end_matches(['\n', '\r']);
    let rest = body.strip_prefix(&ch.to_string().repeat(7))?;
    if rest.is_empty() {
        Some(String::new())
    } else {
        rest.strip_prefix(' ').map(str::to_string)
    }
}

/// Split a file into text and conflict regions. A region that never closes
/// (a stray `<<<<<<<`) is kept as text.
pub fn parse(text: &str) -> Vec<Segment> {
    enum State {
        Text,
        Ours,
        Base,
        Theirs,
    }
    let mut out = Vec::new();
    let mut plain = String::new();
    // The raw lines of the region being read, to fall back to as text.
    let mut raw = String::new();
    let mut region = Region {
        ours: String::new(),
        ours_label: String::new(),
        theirs: String::new(),
        theirs_label: String::new(),
        base: None,
    };
    let mut state = State::Text;
    for line in text.split_inclusive('\n') {
        match state {
            State::Text => {
                if let Some(label) = marker(line, '<') {
                    region = Region {
                        ours: String::new(),
                        ours_label: label,
                        theirs: String::new(),
                        theirs_label: String::new(),
                        base: None,
                    };
                    raw = line.to_string();
                    state = State::Ours;
                } else {
                    plain.push_str(line);
                }
            }
            State::Ours | State::Base => {
                raw.push_str(line);
                if marker(line, '|').is_some() && matches!(state, State::Ours) {
                    region.base = Some(String::new());
                    state = State::Base;
                } else if marker(line, '=').is_some() {
                    state = State::Theirs;
                } else if matches!(state, State::Ours) {
                    region.ours.push_str(line);
                } else if let Some(base) = region.base.as_mut() {
                    base.push_str(line);
                }
            }
            State::Theirs => {
                raw.push_str(line);
                if let Some(label) = marker(line, '>') {
                    region.theirs_label = label;
                    if !plain.is_empty() {
                        out.push(Segment::Text(std::mem::take(&mut plain)));
                    }
                    out.push(Segment::Conflict(region.clone()));
                    raw.clear();
                    state = State::Text;
                } else {
                    region.theirs.push_str(line);
                }
            }
        }
    }
    // Unclosed region: it wasn't a conflict after all.
    plain.push_str(&raw);
    if !plain.is_empty() {
        out.push(Segment::Text(plain));
    }
    out
}

/// How many conflict regions there are.
pub fn count(segments: &[Segment]) -> usize {
    segments
        .iter()
        .filter(|s| matches!(s, Segment::Conflict(_)))
        .count()
}

/// The file with each region replaced by its choice, in order. `choices`
/// must have one entry per region.
pub fn resolve(segments: &[Segment], choices: &[Choice]) -> Result<String, String> {
    let n = count(segments);
    if choices.len() != n {
        return Err(format!(
            "Pick a side for every conflict ({} of {n} picked)",
            choices.len()
        ));
    }
    let mut out = String::new();
    let mut picks = choices.iter();
    for s in segments {
        match s {
            Segment::Text(t) => out.push_str(t),
            Segment::Conflict(r) => {
                let (first, second) = match picks.next().expect("counted above") {
                    Choice::Ours => (r.ours.as_str(), ""),
                    Choice::Theirs => (r.theirs.as_str(), ""),
                    Choice::Both => (r.ours.as_str(), r.theirs.as_str()),
                    Choice::BothTheirsFirst => (r.theirs.as_str(), r.ours.as_str()),
                };
                push_lines(&mut out, first);
                push_lines(&mut out, second);
            }
        }
    }
    Ok(out)
}

/// Append a block of lines, making sure the one before it ended its line
/// (a side whose last line had no newline at the end of the file).
fn push_lines(out: &mut String, lines: &str) {
    if lines.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(lines);
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "top\n<<<<<<< HEAD\nours 1\nours 2\n=======\ntheirs\n>>>>>>> feature\nmiddle\n<<<<<<< HEAD\na\n=======\nb\n>>>>>>> feature\nend\n";

    #[test]
    fn parses_text_and_regions() {
        let s = parse(FILE);
        assert_eq!(count(&s), 2);
        assert_eq!(s[0], Segment::Text("top\n".into()));
        let Segment::Conflict(r) = &s[1] else {
            panic!()
        };
        assert_eq!(r.ours, "ours 1\nours 2\n");
        assert_eq!(r.theirs, "theirs\n");
        assert_eq!(
            (r.ours_label.as_str(), r.theirs_label.as_str()),
            ("HEAD", "feature")
        );
        assert_eq!(r.base, None);
        assert_eq!(s[2], Segment::Text("middle\n".into()));
        assert_eq!(s.last(), Some(&Segment::Text("end\n".into())));
    }

    #[test]
    fn resolves_each_region_its_own_way() {
        let s = parse(FILE);
        assert_eq!(
            resolve(&s, &[Choice::Theirs, Choice::Both]).unwrap(),
            "top\ntheirs\nmiddle\na\nb\nend\n"
        );
        assert_eq!(
            resolve(&s, &[Choice::Ours, Choice::BothTheirsFirst]).unwrap(),
            "top\nours 1\nours 2\nmiddle\nb\na\nend\n"
        );
        assert!(resolve(&s, &[Choice::Ours]).is_err());
    }

    #[test]
    fn reads_the_diff3_base() {
        let s = parse("<<<<<<< ours\nx\n||||||| base\no\n=======\ny\n>>>>>>> theirs\n");
        let Segment::Conflict(r) = &s[0] else {
            panic!()
        };
        assert_eq!(r.base.as_deref(), Some("o\n"));
        assert_eq!((r.ours.as_str(), r.theirs.as_str()), ("x\n", "y\n"));
        assert_eq!(resolve(&s, &[Choice::Theirs]).unwrap(), "y\n");
    }

    #[test]
    fn keeps_crlf_and_a_missing_final_newline() {
        let s = parse("a\r\n<<<<<<< HEAD\r\nx\r\n=======\r\ny\r\n>>>>>>> b\r\n");
        assert_eq!(resolve(&s, &[Choice::Both]).unwrap(), "a\r\nx\r\ny\r\n");
        // Ours ends the file without a newline; Both still puts theirs on its own line.
        let s = parse("<<<<<<< HEAD\nx=======\ny\n>>>>>>> b");
        assert_eq!(count(&s), 0, "a marker must start its own line");
        let s = parse("<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> b");
        assert_eq!(resolve(&s, &[Choice::Ours]).unwrap(), "x\n");
    }

    #[test]
    fn an_unclosed_marker_is_text() {
        let text = "a\n<<<<<<< HEAD\nb\n";
        assert_eq!(parse(text), vec![Segment::Text(text.into())]);
        // Seven characters exactly, then a space or the end of the line.
        assert_eq!(count(&parse("<<<<<<<< x\n=======\n>>>>>>> y\n")), 0);
    }
}
