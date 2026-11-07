//! Claude Code hooks for spor's runs (#103).
//!
//! Spor hands hooks to Claude Code with `--settings` (a JSON file in the
//! run directory), so nothing is written into the repository or the
//! user's own settings. Each hook runs the app's executable with [`ARG`]
//! as its first argument, like the agent host and the MCP server.
//!
//! * `stop`: when the agent tries to finish, run the repository's
//!   `.spor/checks` (one command per line) in the task's folder. On
//!   failure the stop is blocked with the failing output, trimmed, and
//!   the agent keeps working. After `max` tries it is let go, and the
//!   task shows as failing. Every run is logged as a `spor_check` line.
//! * `format`: after an edit, run `.spor/format <file>` on the edited file.

use crate::host;
use crate::json::Json;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// The first argument that turns an app's executable into a hook.
pub const ARG: &str = "__spor-hook";

/// How many times a failing run is sent back before it may finish.
pub const DEFAULT_TRIES: usize = 2;

/// The most output sent back to the agent.
const MAX_OUTPUT: usize = 4000;

/// The commands in a checks file: one per line, blanks and `#` skipped.
pub fn commands(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// `s` in single quotes, for a shell.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The settings for a run: the hooks to pass with `--settings`. None when
/// there is nothing to hook.
pub fn settings(
    program: &Path,
    worktree: &Path,
    log: &Path,
    state: &Path,
    checks: bool,
    format: bool,
    tries: usize,
) -> Option<String> {
    let base = |kind: &str, rest: &[&str]| {
        let mut parts = vec![
            sh_quote(&program.to_string_lossy()),
            ARG.to_string(),
            kind.to_string(),
            sh_quote(&worktree.to_string_lossy()),
            sh_quote(&log.to_string_lossy()),
        ];
        parts.extend(rest.iter().map(|r| sh_quote(r)));
        host::quote(&parts.join(" "))
    };
    let mut hooks = Vec::new();
    if checks {
        let rest = [state.to_string_lossy().to_string(), tries.to_string()];
        let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
        hooks.push(format!(
            r#""Stop":[{{"hooks":[{{"type":"command","command":{}}}]}}]"#,
            base("stop", &rest)
        ));
    }
    if format {
        hooks.push(format!(
            r#""PostToolUse":[{{"matcher":"Edit|Write|MultiEdit","hooks":[{{"type":"command","command":{}}}]}}]"#,
            base("format", &[])
        ));
    }
    if hooks.is_empty() {
        return None;
    }
    Some(format!(r#"{{"hooks":{{{}}}}}"#, hooks.join(",")))
}

/// `ARG stop <worktree> <log> <state> <tries>` or `ARG format <worktree>
/// <log>`: the hook, with Claude Code's event on stdin.
pub fn main(args: &[String]) -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    match args {
        [kind, worktree, log, state, tries] if kind == "stop" => {
            let tries = tries.parse().unwrap_or(DEFAULT_TRIES);
            if let Some(reply) = stop(Path::new(worktree), Path::new(log), Path::new(state), tries)
            {
                println!("{reply}");
            }
            0
        }
        [kind, worktree, log] if kind == "format" => {
            format(Path::new(worktree), Path::new(log), &input);
            0
        }
        _ => {
            eprintln!(
                "usage: {ARG} stop <worktree> <log> <state> <tries> | format <worktree> <log>"
            );
            2
        }
    }
}

/// Append a `spor_check` line to the run's log.
fn log_line(log: &Path, fields: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(log)
    {
        let _ = writeln!(f, r#"{{"type":"spor_check",{fields}}}"#);
    }
}

/// Run one shell command in `dir`: whether it passed, and its output.
fn run(dir: &Path, command: &str) -> (bool, String) {
    let out = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            (o.status.success(), text)
        }
        Err(e) => (false, format!("couldn't run: {e}")),
    }
}

/// The end of `text`, which is where a failure usually says why.
fn trim_output(text: &str) -> String {
    let text = text.trim();
    if text.len() <= MAX_OUTPUT {
        return text.to_string();
    }
    let mut start = text.len() - MAX_OUTPUT;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

/// The `Stop` hook: Some(reply) blocks the stop and says why.
pub fn stop(worktree: &Path, log: &Path, state: &Path, tries: usize) -> Option<String> {
    let file = std::fs::read_to_string(worktree.join(".spor").join("checks")).ok()?;
    let mut failures = Vec::new();
    for command in commands(&file) {
        let (ok, output) = run(worktree, command);
        if !ok {
            failures.push((command, output));
        }
    }
    if failures.is_empty() {
        let _ = std::fs::remove_file(state);
        log_line(log, r#""state":"passed""#);
        return None;
    }
    let used: usize = std::fs::read_to_string(state)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let failed = failures
        .iter()
        .map(|(c, _)| host::quote(c))
        .collect::<Vec<_>>()
        .join(",");
    if used >= tries {
        log_line(
            log,
            &format!(r#""state":"failing","kept_going":false,"tries":{used},"failed":[{failed}]"#),
        );
        return None;
    }
    let _ = std::fs::write(state, (used + 1).to_string());
    log_line(
        log,
        &format!(
            r#""state":"failing","kept_going":true,"tries":{},"failed":[{failed}]"#,
            used + 1
        ),
    );
    let mut reason = String::from("The repository's checks failed. Fix them before finishing.\n");
    for (command, output) in &failures {
        reason.push_str(&format!("\n$ {command}\n{}\n", trim_output(output)));
    }
    Some(format!(
        r#"{{"decision":"block","reason":{}}}"#,
        host::quote(reason.trim_end())
    ))
}

/// The `PostToolUse` hook: format the file the tool edited.
pub fn format(worktree: &Path, log: &Path, event: &str) {
    let script = worktree.join(".spor").join("format");
    if !script.is_file() {
        return;
    }
    let Some(path) = Json::parse(event).and_then(|e| {
        e.get("tool_input")
            .get("file_path")
            .str()
            .map(str::to_string)
    }) else {
        return;
    };
    let (ok, output) = Command::new("sh")
        .arg(&script)
        .arg(&path)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .output()
        .map(|o| {
            let mut t = String::from_utf8_lossy(&o.stdout).into_owned();
            t.push_str(&String::from_utf8_lossy(&o.stderr));
            (o.status.success(), t)
        })
        .unwrap_or((false, String::new()));
    if !ok {
        log_line(
            log,
            &format!(
                r#""state":"format_failed","file":{},"output":{}"#,
                host::quote(&path),
                host::quote(&trim_output(&output))
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_skip_blanks_and_comments() {
        assert_eq!(
            commands("# tests\ncargo test\n\n  cargo clippy  \n"),
            ["cargo test", "cargo clippy"]
        );
    }

    #[test]
    fn quoting_survives_a_quote() {
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn settings_hook_only_what_is_asked() {
        let p = Path::new("/bin/spor");
        let w = Path::new("/w");
        assert!(settings(p, w, w, w, false, false, 2).is_none());
        let s = settings(p, w, w, w, true, true, 2).unwrap();
        let json = Json::parse(&s).expect("valid json");
        assert!(json.get("hooks").get("Stop") != &Json::Null);
        assert!(json.get("hooks").get("PostToolUse") != &Json::Null);
        assert!(s.contains("__spor-hook"));
    }
}
