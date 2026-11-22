//! Any command as the agent: `{prompt}` is replaced with the prompt,
//! shell-quoted, and `$SPOR_PROMPT` holds it too. spor knows nothing of
//! its output, so it's shown as it printed it.

use super::adapter::{Adapter, Launch};
use super::{shell_quote, spor_event, Entry};
use crate::json::Json;

/// The most of a command's output a transcript shows: its end.
const MAX_OUTPUT_LINES: usize = 2000;

pub struct Custom<'a>(pub &'a str);

impl Adapter for Custom<'_> {
    fn id(&self) -> &'static str {
        "command"
    }

    fn label(&self) -> &'static str {
        "Custom command"
    }

    /// The template's command, run by `sh`. What spor would add (the
    /// context, the MCP server) has no place in an arbitrary command; the
    /// context is in `$SPOR_CONTEXT`.
    fn command(&self, launch: &Launch, _extra: Vec<String>) -> (String, Vec<String>) {
        let command = self.0.replace("{prompt}", &shell_quote(launch.prompt));
        ("sh".into(), vec!["-c".into(), command])
    }

    fn parse(&self, log: &str) -> Vec<Entry> {
        let mut out = Vec::new();
        let lines: Vec<&str> = log.lines().collect();
        let skip = lines.len().saturating_sub(MAX_OUTPUT_LINES);
        if skip > 0 {
            out.push(Entry::Output(format!("… {skip} earlier lines")));
        }
        // spor's own marker lines (setup) are events; the rest is the
        // command's output, as it printed it.
        out.extend(lines[skip..].iter().map(|l| {
            if l.starts_with(r#"{"type":"spor_"#) {
                if let Some(e) = Json::parse(l).and_then(|e| spor_event(&e)) {
                    return e;
                }
            }
            Entry::Output(l.to_string())
        }));
        out
    }

    /// Nothing it prints says what it's doing.
    fn progress(&self, _log: &str) -> super::Progress {
        super::Progress::default()
    }
}
