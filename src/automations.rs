//! Automations (#151): tasks that start themselves. A repository lists them
//! in `.spor/automations`, checked in so a team shares them; each names a
//! trigger and the prompt of the task to start.
//!
//! ```text
//! # Blocks are separated by `---`. `prompt:` is last: it and every line
//! # after it, up to the next `---`, is the prompt.
//! name: weekly-deps
//! on: schedule weekly mon 09:00
//! base: main
//! agent: claude
//! mode: acceptEdits
//! prompt: Update outdated dependencies, one commit each, and run the checks.
//! ---
//! name: fix-main
//! on: ci-failed main
//! prompt: CI failed on {branch} ({workflow}, {run_url}). Find out why and fix it.
//! ```
//!
//! Triggers: `schedule daily HH:MM`, `schedule weekdays HH:MM`,
//! `schedule weekly <day> HH:MM` (local time), and `ci-failed [branch]`
//! (a newly failed run of any workflow on that branch; `main` when none is
//! named). In a prompt, `{branch}`, `{workflow}`, `{run}`, `{run_url}` and
//! `{date}` are filled in.
//!
//! This module is pure: it parses the file and, given the time, the saved
//! [`State`] and what GitHub says, decides what to start ([`evaluate`]).
//! Starting the task and keeping the state is the app's job. Guards: an
//! automation never starts a second task while its last one is open, an
//! automation that has just been added doesn't fire for what already
//! happened, and a trigger that fires while it's guarded is used up rather
//! than saved for later. Tasks never land themselves.

use std::path::{Path, PathBuf};

/// Where the file is, relative to the repository's root.
pub const FILE: &str = ".spor/automations";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Days {
    Every,
    Weekdays,
    /// 0 = Monday … 6 = Sunday.
    On(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub days: Days,
    /// Minutes after local midnight.
    pub minute: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    Schedule(Schedule),
    /// A newly failed run on this branch.
    CiFailed {
        branch: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Automation {
    pub name: String,
    pub trigger: Trigger,
    pub prompt: String,
    /// Where the task starts; the repository's default start when none.
    pub base: Option<String>,
    /// `claude` (the default) or `none`, to only create the task.
    pub agent: Option<String>,
    pub model: Option<String>,
    /// A permission mode's name; `acceptEdits` when none.
    pub mode: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub automations: Vec<Automation>,
    /// One line each: what's wrong and in which block. A bad block doesn't
    /// stop the others.
    pub errors: Vec<String>,
}

/// The automations in `text`.
pub fn parse(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut block: Vec<&str> = Vec::new();
    let mut index = 0;
    let mut flush = |block: &mut Vec<&str>, out: &mut Parsed| {
        if block
            .iter()
            .all(|l| l.trim().is_empty() || l.trim().starts_with('#'))
        {
            block.clear();
            return;
        }
        index += 1;
        match parse_block(block) {
            Ok(a) if out.automations.iter().any(|b| b.name == a.name) => out
                .errors
                .push(format!("{}: the name is used twice", a.name)),
            Ok(a) => out.automations.push(a),
            Err(e) => out.errors.push(format!("block {index}: {e}")),
        }
        block.clear();
    };
    for line in text.lines() {
        if line.trim() == "---" {
            flush(&mut block, &mut out);
        } else {
            block.push(line);
        }
    }
    flush(&mut block, &mut out);
    out
}

fn parse_block(lines: &[&str]) -> Result<Automation, String> {
    let (mut name, mut on, mut base, mut agent, mut model, mut mode) =
        (None, None, None, None, None, None);
    let mut prompt: Option<Vec<&str>> = None;
    for line in lines {
        if let Some(p) = prompt.as_mut() {
            p.push(line);
            continue;
        }
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let (key, value) = t
            .split_once(':')
            .ok_or_else(|| format!("not `key: value`: {t}"))?;
        let value = value.trim();
        match key.trim() {
            "name" => name = Some(value.to_string()),
            "on" => on = Some(value.to_string()),
            "base" => base = Some(value.to_string()),
            "agent" => agent = Some(value.to_string()),
            "model" => model = Some(value.to_string()),
            "mode" => mode = Some(value.to_string()),
            "prompt" => prompt = Some(vec![value]),
            other => return Err(format!("unknown key `{other}`")),
        }
    }
    let name = name.filter(|n| !n.is_empty()).ok_or("no name")?;
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("{name}: a name is letters, digits, - and _"));
    }
    let on = on.ok_or_else(|| format!("{name}: no `on:`"))?;
    let trigger = parse_trigger(&on).map_err(|e| format!("{name}: {e}"))?;
    let prompt = prompt
        .map(|p| p.join("\n").trim().to_string())
        .unwrap_or_default();
    if prompt.is_empty() {
        return Err(format!("{name}: no prompt"));
    }
    let agent = agent.filter(|a| !a.is_empty());
    if let Some(a) = &agent {
        if a != "claude" && a != "none" {
            return Err(format!("{name}: agent is `claude` or `none`, not `{a}`"));
        }
    }
    Ok(Automation {
        name,
        trigger,
        prompt,
        base: base.filter(|b| !b.is_empty()),
        agent,
        model: model.filter(|m| !m.is_empty()),
        mode: mode.filter(|m| !m.is_empty()),
    })
}

fn parse_trigger(on: &str) -> Result<Trigger, String> {
    let words: Vec<&str> = on.split_whitespace().collect();
    match words.as_slice() {
        ["schedule", "daily", t] => schedule(Days::Every, t),
        ["schedule", "weekdays", t] => schedule(Days::Weekdays, t),
        ["schedule", "weekly", day, t] => {
            let day = DAYS
                .iter()
                .position(|d| day.to_ascii_lowercase().starts_with(d))
                .ok_or_else(|| format!("`{day}` isn't a day (mon, tue, …)"))?;
            schedule(Days::On(day as u8), t)
        }
        ["ci-failed"] => Ok(Trigger::CiFailed {
            branch: "main".into(),
        }),
        ["ci-failed", branch] => Ok(Trigger::CiFailed {
            branch: branch.to_string(),
        }),
        _ => Err(format!(
            "can't read `on: {on}` (schedule daily|weekdays HH:MM, schedule weekly mon HH:MM, ci-failed [branch])"
        )),
    }
}

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

impl Trigger {
    /// The trigger as it's written after `on:`.
    pub fn describe(&self) -> String {
        match self {
            Trigger::Schedule(s) => {
                let time = format!("{:02}:{:02}", s.minute / 60, s.minute % 60);
                match s.days {
                    Days::Every => format!("schedule daily {time}"),
                    Days::Weekdays => format!("schedule weekdays {time}"),
                    Days::On(d) => format!("schedule weekly {} {time}", DAYS[d as usize % 7]),
                }
            }
            Trigger::CiFailed { branch } => format!("ci-failed {branch}"),
        }
    }
}

fn schedule(days: Days, time: &str) -> Result<Trigger, String> {
    let (h, m) = time
        .split_once(':')
        .ok_or_else(|| format!("`{time}` isn't a time (HH:MM)"))?;
    let (h, m): (u32, u32) = match (h.parse(), m.parse()) {
        (Ok(h), Ok(m)) if h < 24 && m < 60 => (h, m),
        _ => return Err(format!("`{time}` isn't a time (HH:MM)")),
    };
    Ok(Trigger::Schedule(Schedule {
        days,
        minute: h * 60 + m,
    }))
}

impl Schedule {
    /// The latest time this schedule was due, at or before `now` (seconds
    /// since the epoch), in the time zone `offset` seconds east of UTC.
    pub fn latest(&self, now: i64, offset: i64) -> i64 {
        let today = (now + offset).div_euclid(86_400);
        for back in 0..=7 {
            let day = today - back;
            // 1970-01-01 was a Thursday: Monday is 0.
            let weekday = (day + 3).rem_euclid(7) as u8;
            let counts = match self.days {
                Days::Every => true,
                Days::Weekdays => weekday < 5,
                Days::On(d) => weekday == d,
            };
            let at = day * 86_400 + i64::from(self.minute) * 60 - offset;
            if counts && at <= now {
                return at;
            }
        }
        unreachable!("every schedule is due within a week")
    }
}

/// What spor keeps about an automation between ticks, in
/// `<git dir>/spor/automations/<name>`: `key: value` lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// Turned off in the app (on when never saved).
    pub disabled: bool,
    /// The slot or run it has dealt with up to: a schedule's last due time
    /// (seconds), or a CI run's id. None until the automation is first seen.
    pub seen: Option<i64>,
    /// The id of the task it last started.
    pub task: Option<String>,
    /// When it last started one (seconds).
    pub started: Option<i64>,
    /// Why it last didn't start when it was due.
    pub note: Option<String>,
}

impl State {
    pub fn parse(text: &str) -> State {
        let mut s = State::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "disabled" => s.disabled = v == "true",
                "seen" => s.seen = v.parse().ok(),
                "task" if !v.is_empty() => s.task = Some(v.to_string()),
                "started" => s.started = v.parse().ok(),
                "note" if !v.is_empty() => s.note = Some(v.to_string()),
                _ => {}
            }
        }
        s
    }

    pub fn render(&self) -> String {
        let mut out = format!("disabled: {}\n", self.disabled);
        if let Some(v) = self.seen {
            out.push_str(&format!("seen: {v}\n"));
        }
        if let Some(v) = &self.task {
            out.push_str(&format!("task: {v}\n"));
        }
        if let Some(v) = self.started {
            out.push_str(&format!("started: {v}\n"));
        }
        if let Some(v) = &self.note {
            out.push_str(&format!("note: {}\n", v.replace('\n', " ")));
        }
        out
    }

    pub fn path(common_dir: &Path, name: &str) -> PathBuf {
        common_dir.join("spor").join("automations").join(name)
    }

    pub fn load(common_dir: &Path, name: &str) -> State {
        std::fs::read_to_string(Self::path(common_dir, name))
            .map(|t| State::parse(&t))
            .unwrap_or_default()
    }

    pub fn save(&self, common_dir: &Path, name: &str) -> Result<(), String> {
        let path = Self::path(common_dir, name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Written beside and renamed, so a reader never sees half of it.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.render()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }
}

/// A failed run on GitHub, as far as a trigger needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub id: u64,
    pub branch: String,
    pub workflow: String,
    pub title: String,
    pub url: String,
}

/// What [`evaluate`] is told about the world.
pub struct Context<'a> {
    /// Seconds since the epoch.
    pub now: i64,
    /// The local time zone, seconds east of UTC.
    pub offset: i64,
    /// Whether the task the automation last started is still open.
    pub task_open: bool,
    /// Runs that ended in failure, newest first. `None` when GitHub
    /// couldn't be asked (the trigger waits).
    pub failures: Option<&'a [Failure]>,
}

/// A task to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// The prompt with its `{…}` filled in.
    pub prompt: String,
    /// The run that set it off, for a CI trigger.
    pub failure: Option<Failure>,
}

/// What a tick decided: the new state to save, and a task to start if any.
/// The caller saves `state` first, then starts the task and records its id,
/// so a crash in between can't start it twice.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub state: State,
    pub start: Option<Start>,
}

/// Decide what `auto` should do now. See the module's notes for the guards.
pub fn evaluate(auto: &Automation, state: &State, cx: &Context) -> Outcome {
    let mut next = state.clone();
    let none = |state: State| Outcome { state, start: None };
    if state.disabled {
        return none(next);
    }
    let (due, seen, failure) = match &auto.trigger {
        Trigger::Schedule(s) => {
            let slot = s.latest(cx.now, cx.offset);
            match state.seen {
                // Just added: nothing is owed for what's already gone by.
                None => {
                    next.seen = Some(slot);
                    return none(next);
                }
                Some(seen) => (slot > seen, slot, None),
            }
        }
        Trigger::CiFailed { branch } => {
            let Some(all) = cx.failures else {
                return none(next);
            };
            let newest = all.iter().find(|f| &f.branch == branch);
            let top = all
                .iter()
                .filter(|f| &f.branch == branch)
                .map(|f| f.id as i64)
                .max();
            match (state.seen, top) {
                (None, top) => {
                    next.seen = Some(top.unwrap_or(0));
                    return none(next);
                }
                (Some(seen), Some(top)) if top > seen => (true, top, newest.cloned()),
                (Some(seen), _) => (false, seen, None),
            }
        }
    };
    if !due {
        return none(next);
    }
    next.seen = Some(seen);
    if cx.task_open {
        next.note = Some("skipped: its last task is still open".into());
        return none(next);
    }
    next.note = None;
    Outcome {
        state: next,
        start: Some(Start {
            prompt: fill(&auto.prompt, failure.as_ref(), cx.now, cx.offset),
            failure,
        }),
    }
}

/// `prompt` with `{branch}`, `{workflow}`, `{run}`, `{run_url}` and
/// `{date}` replaced. Names it has no value for are left as written.
pub fn fill(prompt: &str, failure: Option<&Failure>, now: i64, offset: i64) -> String {
    let mut out = prompt.replace("{date}", &date(now + offset));
    if let Some(f) = failure {
        out = out
            .replace("{branch}", &f.branch)
            .replace("{workflow}", &f.workflow)
            .replace("{run}", &f.title)
            .replace("{run_url}", &f.url);
    }
    out
}

/// `2026-11-14` for a local time in seconds since the epoch (the civil
/// calendar, from Howard Hinnant's days-to-date).
fn date(local: i64) -> String {
    let z = local.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE_TEXT: &str = "\
# deps
name: weekly-deps
on: schedule weekly mon 09:00
base: main
mode: acceptEdits
prompt: Update dependencies.
Run the checks.
---
name: fix-main
on: ci-failed
prompt: {workflow} failed on {branch}: {run_url}
---
name: bad
on: whenever
prompt: x
";

    fn weekly() -> Automation {
        parse(FILE_TEXT).automations.remove(0)
    }

    #[test]
    fn parses_blocks_and_keeps_going_past_a_bad_one() {
        let p = parse(FILE_TEXT);
        assert_eq!(p.automations.len(), 2);
        let a = &p.automations[0];
        assert_eq!(a.name, "weekly-deps");
        assert_eq!(
            a.trigger,
            Trigger::Schedule(Schedule {
                days: Days::On(0),
                minute: 540
            })
        );
        assert_eq!(a.prompt, "Update dependencies.\nRun the checks.");
        assert_eq!(a.base.as_deref(), Some("main"));
        assert_eq!(
            p.automations[1].trigger,
            Trigger::CiFailed {
                branch: "main".into()
            }
        );
        assert_eq!(a.trigger.describe(), "schedule weekly mon 09:00");
        assert_eq!(p.automations[1].trigger.describe(), "ci-failed main");
        assert_eq!(p.errors.len(), 1);
        assert!(p.errors[0].contains("bad") && p.errors[0].contains("whenever"));
    }

    #[test]
    fn rejects_duplicates_missing_prompts_and_unknown_keys() {
        let p = parse("name: a\non: ci-failed\nprompt: x\n---\nname: a\non: ci-failed\nprompt: y");
        assert_eq!((p.automations.len(), p.errors.len()), (1, 1));
        assert!(parse("name: a\non: ci-failed").errors[0].contains("no prompt"));
        assert!(parse("name: a\ncolour: red\nprompt: x").errors[0].contains("unknown key"));
        assert!(parse("name: a b\non: ci-failed\nprompt: x").errors[0].contains("letters"));
        assert!(parse("name: a\non: schedule daily 25:00\nprompt: x").errors[0].contains("time"));
        assert!(parse("").automations.is_empty());
    }

    // 2026-11-14 is a Saturday. 12:00 UTC that day:
    const SAT_NOON: i64 = 1_794_657_600;

    #[test]
    fn a_schedule_is_due_at_its_latest_slot() {
        assert_eq!(date(SAT_NOON), "2026-11-14");
        let daily = Schedule {
            days: Days::Every,
            minute: 9 * 60,
        };
        assert_eq!(daily.latest(SAT_NOON, 0), SAT_NOON - 3 * 3600);
        // Before today's slot: yesterday's.
        assert_eq!(daily.latest(SAT_NOON - 4 * 3600, 0), SAT_NOON - 27 * 3600);
        // Weekdays skip the weekend: Friday's 09:00.
        let weekdays = Schedule {
            days: Days::Weekdays,
            minute: 9 * 60,
        };
        assert_eq!(weekdays.latest(SAT_NOON, 0), SAT_NOON - 27 * 3600);
        // Monday morning, a week back from Sunday.
        let monday = Schedule {
            days: Days::On(0),
            minute: 9 * 60,
        };
        assert_eq!(monday.latest(SAT_NOON, 0), SAT_NOON - (5 * 24 + 3) * 3600);
        // A zone east of UTC: 09:00 at +02:00 is 07:00 UTC.
        assert_eq!(daily.latest(SAT_NOON, 7200), SAT_NOON - 5 * 3600);
    }

    fn cx(now: i64, task_open: bool, failures: Option<&[Failure]>) -> Context<'_> {
        Context {
            now,
            offset: 0,
            task_open,
            failures,
        }
    }

    #[test]
    fn a_new_schedule_arms_itself_then_fires_once_per_slot() {
        let a = weekly();
        let first = evaluate(&a, &State::default(), &cx(SAT_NOON, false, None));
        assert!(first.start.is_none());
        let armed = first.state;
        assert!(armed.seen.is_some());
        // Nothing until next Monday 09:00.
        let still = evaluate(&a, &armed, &cx(SAT_NOON + 86_400, false, None));
        assert!(still.start.is_none());
        let monday = SAT_NOON + 2 * 86_400 - 3 * 3600 + 3600; // Mon 10:00
        let fired = evaluate(&a, &armed, &cx(monday, false, None));
        assert_eq!(
            fired.start.as_ref().unwrap().prompt,
            "Update dependencies.\nRun the checks."
        );
        // And not again in the same slot.
        let again = evaluate(&a, &fired.state, &cx(monday + 60, false, None));
        assert!(again.start.is_none());
    }

    #[test]
    fn an_open_task_guards_and_uses_up_the_slot() {
        let a = weekly();
        let armed = evaluate(&a, &State::default(), &cx(SAT_NOON, false, None)).state;
        let monday = SAT_NOON + 2 * 86_400 - 2 * 3600;
        let held = evaluate(&a, &armed, &cx(monday, true, None));
        assert!(held.start.is_none());
        assert!(held.state.note.as_deref().unwrap().contains("still open"));
        // Once the task is done, the same slot doesn't start another.
        let later = evaluate(&a, &held.state, &cx(monday + 600, false, None));
        assert!(later.start.is_none());
    }

    #[test]
    fn disabled_automations_do_nothing() {
        let a = weekly();
        let s = State {
            disabled: true,
            ..State::default()
        };
        let out = evaluate(&a, &s, &cx(SAT_NOON, false, None));
        assert_eq!((out.state, out.start), (s, None));
    }

    fn failure(id: u64, branch: &str) -> Failure {
        Failure {
            id,
            branch: branch.into(),
            workflow: "CI".into(),
            title: "Fix it".into(),
            url: format!("https://github.com/o/r/actions/runs/{id}"),
        }
    }

    #[test]
    fn ci_failed_fires_for_a_new_failure_on_its_branch_only() {
        let a = parse("name: f\non: ci-failed main\nprompt: {workflow} on {branch}: {run_url}")
            .automations
            .remove(0);
        let old = [failure(5, "main"), failure(9, "other")];
        let armed = evaluate(&a, &State::default(), &cx(0, false, Some(&old))).state;
        assert_eq!(armed.seen, Some(5));
        // The same failures: nothing.
        assert!(evaluate(&a, &armed, &cx(0, false, Some(&old)))
            .start
            .is_none());
        // GitHub unreachable: waits, state untouched.
        let out = evaluate(&a, &armed, &cx(0, false, None));
        assert_eq!((out.start, out.state), (None, armed.clone()));
        // A failure on another branch: nothing.
        let other = [failure(12, "dev"), failure(5, "main")];
        assert!(evaluate(&a, &armed, &cx(0, false, Some(&other)))
            .start
            .is_none());
        // A new one on main.
        let new = [failure(12, "main"), failure(5, "main")];
        let out = evaluate(&a, &armed, &cx(0, false, Some(&new)));
        let start = out.start.unwrap();
        assert_eq!(
            start.prompt,
            "CI on main: https://github.com/o/r/actions/runs/12"
        );
        assert_eq!(out.state.seen, Some(12));
    }

    #[test]
    fn state_round_trips() {
        let s = State {
            disabled: true,
            seen: Some(-3),
            task: Some("brave-otter-jumps".into()),
            started: Some(99),
            note: Some("skipped: a\nb".into()),
        };
        let back = State::parse(&s.render());
        assert_eq!(back.note.as_deref(), Some("skipped: a b"));
        assert_eq!(State { note: None, ..back }, State { note: None, ..s });
        assert_eq!(State::parse(""), State::default());
    }

    #[test]
    fn dates_are_civil() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(date(-86_400), "1969-12-31");
    }
}
