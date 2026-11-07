//! Rules worth writing down (#107): corrections people give agents over and
//! over ("use pnpm, not npm"), found across a repository's tasks, grouped by
//! Claude Code into candidate lines for `CLAUDE.md`.

use crate::agent::{ask_readonly_in, Entry, Runs};
use crate::json::Json;
use crate::tasks::Tasks;
use std::path::{Path, PathBuf};

/// Something a person told an agent to do differently, and in which task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    pub task: String,
    pub text: String,
}

/// A candidate rule and the tasks whose corrections it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// The line to add: "Use pnpm, not npm."
    pub rule: String,
    /// At least two different tasks.
    pub tasks: Vec<String>,
}

/// The most corrections sent to Claude, newest first kept.
const MAX_CORRECTIONS: usize = 120;
/// One correction, cut here.
const MAX_CHARS: usize = 400;

/// What people told the agents in every task of the repository: follow-up
/// messages (line comments go in them), steering, and a denial's reason.
/// A task's own first prompt isn't a correction.
pub fn collect(tasks: &Tasks) -> Vec<Correction> {
    let mut out = Vec::new();
    for task in tasks.list() {
        let runs = Runs::of(tasks, &task);
        let mut first = true;
        let mut last = String::new();
        for entry in runs.transcript() {
            let text = match entry {
                Entry::Prompt(_) if first => {
                    first = false;
                    continue;
                }
                Entry::Prompt(p) => feedback_of(&p),
                Entry::Steer(s) => s,
                Entry::Answer {
                    allow: false,
                    message,
                    ..
                } => message,
                _ => continue,
            };
            let text: String = text.trim().chars().take(MAX_CHARS).collect();
            if text.is_empty() || text == last {
                continue;
            }
            last.clone_from(&text);
            out.push(Correction {
                task: task.id.clone(),
                text,
            });
        }
    }
    let skip = out.len().saturating_sub(MAX_CORRECTIONS);
    out.split_off(skip)
}

/// A follow-up that had to restate the whole task carries the feedback
/// after this marker; the rest is the task's own prompt.
fn feedback_of(prompt: &str) -> String {
    match prompt.split_once("Feedback on the work so far:\n") {
        Some((_, feedback)) => feedback.to_string(),
        None => prompt.to_string(),
    }
}

/// What Claude is asked: group the corrections into rules.
pub fn prompt_for(corrections: &[Correction]) -> String {
    let mut out = String::from(
        "Below are messages people sent to coding agents working on this repository, \
         numbered, each from a task. Find instructions that were given again and again in \
         different tasks, and that belong in the repository's CLAUDE.md as a standing rule \
         (a convention, a tool to use or avoid, a place something goes, a thing not to touch). \
         Ignore one-off requests about a single piece of work.\n\n",
    );
    for (i, c) in corrections.iter().enumerate() {
        out.push_str(&format!(
            "{}. [{}] {}\n",
            i + 1,
            c.task,
            c.text.replace('\n', " ")
        ));
    }
    out.push_str(
        "\nReply with only JSON, no other text, in this shape:\n\
         {\"rules\": [{\"rule\": \"one short imperative sentence for CLAUDE.md\", \"from\": [message numbers]}]}\n\
         Only include a rule if at least two messages from different tasks say it. \
         Use an empty list if there is none. Don't change any files.",
    );
    out
}

/// Read Claude's reply: rules that really come from at least two tasks,
/// whatever the reply claims.
pub fn parse_reply(reply: &str, corrections: &[Correction]) -> Vec<Suggestion> {
    let text = reply.trim();
    let json = Json::parse(text).or_else(|| {
        let (start, end) = (text.find('{')?, text.rfind('}')?);
        Json::parse(text.get(start..=end)?)
    });
    let Some(json) = json else {
        return Vec::new();
    };
    let mut out: Vec<Suggestion> = Vec::new();
    for r in json.get("rules").array() {
        let rule = r.get("rule").str().unwrap_or_default().trim().to_string();
        if rule.is_empty() || out.iter().any(|s| same(&s.rule, &rule)) {
            continue;
        }
        let mut tasks: Vec<String> = Vec::new();
        for n in r.get("from").array() {
            let Some(c) = n
                .num()
                .and_then(|n| (n as usize).checked_sub(1))
                .and_then(|i| corrections.get(i))
            else {
                continue;
            };
            if !tasks.contains(&c.task) {
                tasks.push(c.task.clone());
            }
        }
        if tasks.len() >= 2 {
            out.push(Suggestion { rule, tasks });
        }
    }
    out
}

/// The same rule, give or take case, spacing and a final full stop.
fn same(a: &str, b: &str) -> bool {
    normal(a) == normal(b)
}

fn normal(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches('.')
        .to_lowercase()
}

/// Ask Claude (read-only, low effort) for rules worth writing down, leaving
/// out any that were added or turned down before.
pub fn suggest(tasks: &Tasks) -> Result<Vec<Suggestion>, String> {
    let corrections = collect(tasks);
    if corrections.len() < 2 {
        return Ok(Vec::new());
    }
    let reply = ask_readonly_in(
        tasks.root(),
        None,
        Some("low"),
        &prompt_for(&corrections),
        "look for repeated rules",
    )?;
    Ok(without_known(tasks, parse_reply(&reply, &corrections)))
}

/// `found`, less the rules that were added or turned down before.
pub fn without_known(tasks: &Tasks, found: Vec<Suggestion>) -> Vec<Suggestion> {
    let known = known(tasks);
    found
        .into_iter()
        .filter(|s| !known.iter().any(|k| same(k, &s.rule)))
        .collect()
}

fn memory(tasks: &Tasks) -> PathBuf {
    tasks.attachments_dir().with_file_name("rules.known")
}

/// Rules already added or turned down.
fn known(tasks: &Tasks) -> Vec<String> {
    std::fs::read_to_string(memory(tasks))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .filter(|l| !l.is_empty())
        .collect()
}

fn remember(tasks: &Tasks, rule: &str) -> Result<(), String> {
    use std::io::Write as _;
    let path = memory(tasks);
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{}", rule.replace('\n', " ")).map_err(|e| e.to_string())
}

/// Turn a suggestion down: it isn't suggested again.
pub fn decline(tasks: &Tasks, rule: &str) -> Result<(), String> {
    remember(tasks, rule)
}

/// The file rules go in: `CLAUDE.md`, or `AGENTS.md` if that is what the
/// repository has.
pub fn rules_file(root: &Path) -> PathBuf {
    let claude = root.join("CLAUDE.md");
    let agents = root.join("AGENTS.md");
    if !claude.exists() && agents.exists() {
        agents
    } else {
        claude
    }
}

/// Add `rule` as a bullet at the end of the rules file in the main checkout,
/// as an uncommitted change that goes through Changes like any edit. It
/// isn't suggested again.
pub fn add(tasks: &Tasks, rule: &str) -> Result<PathBuf, String> {
    let rule = rule.trim().replace('\n', " ");
    if rule.is_empty() {
        return Err("Write the rule first".into());
    }
    let path = rules_file(tasks.root());
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("- {rule}\n"));
    std::fs::write(&path, text).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    remember(tasks, &rule)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(task: &str, text: &str) -> Correction {
        Correction {
            task: task.into(),
            text: text.into(),
        }
    }

    #[test]
    fn a_rule_needs_two_tasks_whatever_claude_says() {
        let list = [
            c("a", "use pnpm"),
            c("a", "pnpm, not npm"),
            c("b", "we use pnpm here"),
            c("c", "rename it"),
        ];
        let reply = r#"```json
{"rules":[{"rule":"Use pnpm, not npm.","from":[1,3]},
          {"rule":"Only one task says this.","from":[1,2]},
          {"rule":"use pnpm, not npm","from":[3,1]},
          {"rule":"Out of range.","from":[9,10]}]}
```"#;
        let found = parse_reply(reply, &list);
        assert_eq!(
            found,
            vec![Suggestion {
                rule: "Use pnpm, not npm.".into(),
                tasks: vec!["a".into(), "b".into()]
            }]
        );
        assert!(parse_reply("no json", &list).is_empty());
    }

    #[test]
    fn the_prompt_numbers_the_corrections_with_their_tasks() {
        let p = prompt_for(&[c("a", "use pnpm\nplease"), c("b", "tests go beside")]);
        assert!(p.contains("1. [a] use pnpm please"), "{p}");
        assert!(p.contains("2. [b] tests go beside"), "{p}");
        assert!(p.contains("at least two messages from different tasks"));
    }

    #[test]
    fn feedback_is_what_comes_after_the_restated_task() {
        assert_eq!(
            feedback_of("Do it\n\nFeedback on the work so far:\nuse pnpm"),
            "use pnpm"
        );
        assert_eq!(feedback_of("use pnpm"), "use pnpm");
    }
}
