//! Searching what was said (#108): prompts, the agent's words, follow-ups
//! and debriefs across every task of a repository, landed ones included
//! (their transcripts are kept while their `Spor-Task` trailer is in
//! history).

use crate::agent::{Entry, Runs};
use crate::tasks::Tasks;

/// Where a match was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    /// A run's prompt: the task's own, or a follow-up.
    Prompt,
    /// The agent's words.
    Agent,
    /// A message sent to it while it worked.
    Steer,
    /// A run's debrief.
    Debrief,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub task: String,
    /// The run it was in: the conversation opens there.
    pub run: usize,
    pub kind: Where,
    /// The matching text with some around it.
    pub snippet: String,
}

/// The most hits returned.
const MAX_HITS: usize = 60;
/// Characters kept either side of a match.
const AROUND: usize = 70;

/// Every place all the words of `query` appear (case ignored) in a task's
/// conversation, newest tasks first, one hit per entry.
pub fn transcripts(tasks: &Tasks, query: &str) -> Vec<Hit> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for task in tasks.list() {
        let runs = Runs::of(tasks, &task);
        // The run an entry belongs to: a prompt starts the next one.
        let mut run = None::<usize>;
        let mut next = 0;
        let mut seen_debrief = Vec::new();
        for entry in runs.transcript() {
            let (kind, text) = match entry {
                Entry::Prompt(p) => {
                    run = Some(next);
                    next += 1;
                    (Where::Prompt, p)
                }
                Entry::Text(t) => (Where::Agent, t),
                Entry::Steer(s) => (Where::Steer, s),
                _ => continue,
            };
            if let (Some(run), Some(snippet)) = (run, snippet(&text, &words)) {
                out.push(Hit {
                    task: task.id.clone(),
                    run,
                    kind,
                    snippet,
                });
            }
            if let Some(run) = run {
                if !seen_debrief.contains(&run) {
                    seen_debrief.push(run);
                    if let Some(d) = runs.debrief(run) {
                        let text = format!(
                            "{}\n{}\n{}",
                            d.changed,
                            d.checked,
                            d.open_questions.join("\n")
                        );
                        if let Some(snippet) = snippet(&text, &words) {
                            out.push(Hit {
                                task: task.id.clone(),
                                run,
                                kind: Where::Debrief,
                                snippet,
                            });
                        }
                    }
                }
            }
            if out.len() >= MAX_HITS {
                return out;
            }
        }
    }
    out
}

/// `text` around the first word's match, if every word is in it.
fn snippet(text: &str, words: &[String]) -> Option<String> {
    let lower = text.to_lowercase();
    if !words.iter().all(|w| lower.contains(w.as_str())) {
        return None;
    }
    // `to_lowercase` can change lengths; find the match in the original by chars.
    let at = lower.find(words[0].as_str())?;
    let chars_before = lower[..at].chars().count();
    let start = chars_before.saturating_sub(AROUND);
    let flat: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let body: String = flat
        .chars()
        .skip(start)
        .take(AROUND * 2 + words[0].chars().count())
        .collect();
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(body.trim());
    if flat.chars().count() > start + body.chars().count() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(q: &str) -> Vec<String> {
        q.split_whitespace().map(str::to_lowercase).collect()
    }

    #[test]
    fn every_word_must_match_and_the_snippet_centres_on_it() {
        let text = format!(
            "{} we decided to DROP the old endpoint {}",
            "x ".repeat(80),
            "y ".repeat(80)
        );
        let s = snippet(&text, &words("drop endpoint")).unwrap();
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert!(s.contains("DROP the old endpoint"), "{s}");
        assert!(snippet(&text, &words("drop pnpm")).is_none());
        assert_eq!(snippet("Drop it", &words("drop")).unwrap(), "Drop it");
    }

    #[test]
    fn a_snippet_survives_multibyte_text() {
        let text = format!("{}Ærlig talt: rydd opp i CSS", "æø ".repeat(60));
        let s = snippet(&text, &words("rydd")).unwrap();
        assert!(s.contains("rydd opp"), "{s}");
    }
}
