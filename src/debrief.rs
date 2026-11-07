//! A debrief at the end of a run (#104): the same five things every time,
//! so a reviewer doesn't depend on whatever the agent last felt like
//! saying. Like `describe_pull`, it asks a read-only fork of the run's
//! session, here for JSON in a fixed shape, and saves the reply beside the
//! run (`run-N.debrief`). It never replaces the transcript.

use crate::agent::{ask_readonly, Run, RunState, Runs};
use crate::json::Json;
use crate::tasks::Task;
use std::process::Command;

/// Something a reviewer should look at closely.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Risk {
    /// The file it is about, relative to the task's folder; empty if it
    /// isn't about one.
    pub path: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Debrief {
    /// What changed and why.
    pub changed: String,
    /// What it ran and what passed. Empty if nothing was run.
    pub checked: String,
    pub risks: Vec<Risk>,
    /// What it wasn't sure about, or assumed.
    pub open_questions: Vec<String>,
    /// Work it noticed but didn't do.
    pub follow_ups: Vec<String>,
}

impl Debrief {
    /// Read the JSON a debrief is saved as (and asked for). Tolerates a
    /// fence around it, or text before and after.
    pub fn parse(reply: &str) -> Option<Debrief> {
        let text = reply.trim();
        let json = Json::parse(text).or_else(|| {
            let (start, end) = (text.find('{')?, text.rfind('}')?);
            Json::parse(text.get(start..=end)?)
        })?;
        if !matches!(json, Json::Object(_)) {
            return None;
        }
        let strings = |key: &str| -> Vec<String> {
            json.get(key)
                .array()
                .iter()
                .filter_map(Json::str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        let text_of = |key: &str| match json.get(key) {
            Json::Array(_) => strings(key).join("\n"),
            other => other.str().unwrap_or("").trim().to_string(),
        };
        let risks = json
            .get("risks")
            .array()
            .iter()
            .filter_map(|r| match r {
                Json::String(s) => Some(Risk {
                    path: String::new(),
                    note: s.trim().to_string(),
                }),
                r => Some(Risk {
                    path: r.get("path").str().unwrap_or("").trim().to_string(),
                    note: r.get("note").str()?.trim().to_string(),
                }),
            })
            .filter(|r| !r.note.is_empty())
            .collect();
        let debrief = Debrief {
            changed: text_of("changed"),
            checked: text_of("checked"),
            risks,
            open_questions: strings("open_questions"),
            follow_ups: strings("follow_ups"),
        };
        (!debrief.changed.is_empty() || !debrief.risks.is_empty()).then_some(debrief)
    }

    /// As saved beside the run.
    pub fn to_json(&self) -> String {
        use std::collections::BTreeMap;
        let list = |v: &[String]| Json::Array(v.iter().cloned().map(Json::String).collect());
        let mut o = BTreeMap::new();
        o.insert("changed".into(), Json::String(self.changed.clone()));
        o.insert("checked".into(), Json::String(self.checked.clone()));
        o.insert(
            "risks".into(),
            Json::Array(
                self.risks
                    .iter()
                    .map(|r| {
                        let mut m = BTreeMap::new();
                        m.insert("path".into(), Json::String(r.path.clone()));
                        m.insert("note".into(), Json::String(r.note.clone()));
                        Json::Object(m)
                    })
                    .collect(),
            ),
        );
        o.insert("open_questions".into(), list(&self.open_questions));
        o.insert("follow_ups".into(), list(&self.follow_ups));
        Json::Object(o).to_json()
    }

    /// Why a reviewer might look twice, in a few words, for the Inbox:
    /// "2 open questions", "tests not run".
    pub fn reason(&self) -> Option<String> {
        let n = self.open_questions.len();
        if n > 0 {
            return Some(format!(
                "{n} open question{}",
                if n == 1 { "" } else { "s" }
            ));
        }
        let checked = self.checked.trim().to_lowercase();
        if checked.is_empty() || checked.starts_with("nothing") || checked.starts_with("not ") {
            return Some("tests not run".into());
        }
        None
    }

    /// The open questions as one follow-up to send the agent, to answer
    /// them in one go. `answers` is what the person wrote for each, in
    /// order; unanswered ones are left out.
    pub fn answers_message(&self, answers: &[String]) -> Option<String> {
        let lines: Vec<String> = self
            .open_questions
            .iter()
            .zip(answers)
            .filter(|(_, a)| !a.trim().is_empty())
            .map(|(q, a)| format!("- {q}\n  Answer: {}", a.trim()))
            .collect();
        (!lines.is_empty()).then(|| {
            format!(
                "Answers to your open questions:\n\n{}\n\nGo on with these in mind.",
                lines.join("\n")
            )
        })
    }
}

const PROMPT: &str = "The run is over. Write its debrief for a reviewer, as JSON only, \
no other text, in exactly this shape:\n\
{\"changed\": \"a few lines on what changed and why\", \
\"checked\": \"what you ran and what passed or failed; empty if you ran nothing\", \
\"risks\": [{\"path\": \"file/path\", \"note\": \"what to look at closely\"}], \
\"open_questions\": [\"what you weren't sure about, or assumed\"], \
\"follow_ups\": [\"work you noticed but didn't do\"]}\n\
Be honest and brief; use empty lists where there is nothing. Don't change any files.";

/// Is the debrief on for this task's repository? It costs a little, so
/// `git config spor.debrief false` turns it off.
pub fn enabled(task: &Task) -> bool {
    let out = Command::new("git")
        .current_dir(&task.worktree)
        .args(["config", "--type=bool", "--get", "spor.debrief"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim() != "false",
        _ => true,
    }
}

impl Runs {
    /// Run `index`'s saved debrief, if it has one.
    pub fn debrief(&self, index: usize) -> Option<Debrief> {
        Debrief::parse(&std::fs::read_to_string(self.file(index, "debrief")).ok()?)
    }

    /// The latest run's debrief.
    pub fn latest_debrief(&self) -> Option<Debrief> {
        self.debrief(self.latest()?.index)
    }

    /// Does `run` still need one: finished, resumable, none saved yet?
    pub fn wants_debrief(&self, run: &Run) -> bool {
        run.state == RunState::Finished
            && !run.undone
            && run.session.is_some()
            && !self.file(run.index, "debrief").exists()
    }

    /// Debrief the latest run if it wants one and the repository hasn't
    /// opted out: a short read-only fork of its session at a low effort,
    /// saved beside it. Returns the debrief, or `None` when there's
    /// nothing to do.
    pub fn debrief_latest(&self, task: &Task) -> Result<Option<Debrief>, String> {
        let Some(run) = self.latest() else {
            return Ok(None);
        };
        if !enabled(task) || !self.wants_debrief(&run) {
            return Ok(self.debrief(run.index));
        }
        let reply = ask_readonly(
            task,
            run.session.clone(),
            Some("low"),
            PROMPT,
            "write the debrief",
        )?;
        let debrief = Debrief::parse(&reply).ok_or("Claude's debrief wasn't in the right shape")?;
        std::fs::write(self.file(run.index, "debrief"), debrief.to_json())
            .map_err(|e| e.to_string())?;
        Ok(Some(debrief))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = r#"```json
{"changed":"Added the flag","checked":"cargo test passed","risks":[{"path":"src/a.rs","note":"new branch"},"loose"],
 "open_questions":["Keep v1?"],"follow_ups":["Docs"]}
```"#;

    #[test]
    fn parses_a_fenced_reply() {
        let d = Debrief::parse(REPLY).unwrap();
        assert_eq!(d.changed, "Added the flag");
        assert_eq!(d.risks.len(), 2);
        assert_eq!(d.risks[0].path, "src/a.rs");
        assert_eq!(d.risks[1].path, "");
        assert_eq!(d.open_questions, ["Keep v1?"]);
        assert_eq!(d.follow_ups, ["Docs"]);
    }

    #[test]
    fn round_trips_and_rejects_nonsense() {
        let d = Debrief::parse(REPLY).unwrap();
        assert_eq!(Debrief::parse(&d.to_json()), Some(d));
        assert_eq!(Debrief::parse("Sure! Here you go."), None);
        assert_eq!(Debrief::parse("{}"), None);
    }

    #[test]
    fn reasons_for_the_inbox() {
        let mut d = Debrief::parse(REPLY).unwrap();
        assert_eq!(d.reason().as_deref(), Some("1 open question"));
        d.open_questions.clear();
        assert_eq!(d.reason(), None);
        d.checked.clear();
        assert_eq!(d.reason().as_deref(), Some("tests not run"));
    }

    #[test]
    fn answers_become_one_message() {
        let mut d = Debrief::parse(REPLY).unwrap();
        d.open_questions.push("Rename?".into());
        let m = d.answers_message(&["Yes".into(), " ".into()]).unwrap();
        assert!(m.contains("- Keep v1?\n  Answer: Yes") && !m.contains("Rename?"));
        assert_eq!(d.answers_message(&[]), None);
    }
}
