//! Automations (#151): tasks that start themselves, from a repository's
//! `.spor/automations` (see `spor::automations`). The UI ticks this once a
//! minute with every repository it knows, so automations run while the app
//! is open and not otherwise. Each tick reads the file, asks `evaluate`
//! what to do, saves the state, then starts the task.

use crate::agents::mode_of;
use crate::{first_line, App};
use serde::Serialize;
use spor::agent::{self, AgentKind, Model, Permissions, Runs};
use spor::automations::{self as auto, Automation, Failure, State as AutoState, Trigger};
use spor::git::Repo;
use spor::github as gh;
use spor::tasks::{TaskState, Tasks};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Seconds east of UTC in the local time zone, from `date +%z` (`+0200`).
/// Falls back to UTC.
fn local_offset() -> i64 {
    let out = std::process::Command::new("date").arg("+%z").output().ok();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    text.as_deref().and_then(parse_offset).unwrap_or(0)
}

fn parse_offset(z: &str) -> Option<i64> {
    let sign = match z.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits = z.get(1..5)?;
    let (h, m): (i64, i64) = (
        digits.get(..2)?.parse().ok()?,
        digits.get(2..)?.parse().ok()?,
    );
    Some(sign * (h * 3600 + m * 60))
}

fn discover(root: &str) -> Option<Repo> {
    Repo::discover(&PathBuf::from(root)).ok()
}

/// The automations written in the checkout's `.spor/automations`.
fn read(repo: &Repo) -> auto::Parsed {
    match std::fs::read_to_string(repo.root().join(auto::FILE)) {
        Ok(text) => auto::parse(&text),
        Err(_) => auto::Parsed::default(),
    }
}

/// Failed runs of the repository, newest first. None when GitHub can't be
/// asked (`gh` missing or signed out, or not a GitHub repository).
fn failures(app: &App, repo: &Repo) -> Option<Vec<Failure>> {
    let runs = app.github.runs(repo.root()).ok()?;
    Some(
        runs.into_iter()
            .filter(|r| r.status == gh::Status::Failure)
            .map(|r| Failure {
                id: r.id,
                branch: r.branch,
                workflow: r.workflow,
                title: r.title,
                url: r.url,
            })
            .collect(),
    )
}

fn task_open(tasks: &Tasks, state: &AutoState) -> bool {
    state
        .task
        .as_deref()
        .and_then(|id| tasks.get(id))
        .is_some_and(|t| t.state == TaskState::Open)
}

/// Create the task for `a` and start its agent, returning the task's id.
fn start(repo: &Repo, a: &Automation, prompt: &str) -> Result<String, String> {
    let kind = match a.agent.as_deref() {
        Some("none") => None,
        _ => Some(AgentKind::Claude),
    };
    if let Some(e) = kind.as_ref().and_then(agent::missing) {
        return Err(e);
    }
    let mode = mode_of(a.mode.as_deref().unwrap_or("acceptEdits"))?;
    let model = Model::new(a.model.as_deref(), None)?;
    let base = a
        .base
        .clone()
        .or_else(|| repo.default_start())
        .or_else(|| repo.default_base_branch())
        .ok_or("No base branch to start from")?;
    // A remote branch starts from what the remote has now, as far as it can.
    if let (Some(branch), Some((remote, _))) = (repo.remote_branch(&base), base.split_once('/')) {
        let _ = spor::askpass::git_network_quiet(repo.root(), &["fetch", remote, &branch]);
    }
    let tasks = Tasks::open(repo)?;
    let task = tasks
        .create(prompt, &base)
        .map_err(|e| format!("Couldn't create the task: {}", first_line(&e)))?;
    if let Some(kind) = kind {
        match Runs::of(&tasks, &task).start_with(
            &task,
            &kind,
            &task.prompt,
            None,
            Permissions { mode },
            &model,
        ) {
            Ok(_) => {}
            Err(e) if e == agent::WAITING => {}
            // The task exists; it just has no run. Say so, but keep the id.
            Err(e) => eprintln!("automation {}: {}", a.name, first_line(&e)),
        }
    }
    Ok(task.id)
}

#[derive(Serialize)]
pub(crate) struct Started {
    repo: String,
    automation: String,
    task: String,
}

/// Look at every repository's automations and start what's due. Cheap when
/// there's nothing to do: a file read per repository, and GitHub only for
/// a `ci-failed` trigger (through the shared cache).
#[tauri::command(async)]
pub(crate) fn automations_tick(
    handle: AppHandle,
    app: State<App>,
    roots: Vec<String>,
) -> Vec<Started> {
    let offset = local_offset();
    let mut started = Vec::new();
    for root in roots {
        let Some(repo) = discover(&root) else {
            continue;
        };
        let parsed = read(&repo);
        if parsed.automations.is_empty() {
            continue;
        }
        let Ok(common) = repo.common_dir() else {
            continue;
        };
        let Ok(tasks) = Tasks::open(&repo) else {
            continue;
        };
        let wants_ci = parsed
            .automations
            .iter()
            .any(|a| matches!(a.trigger, Trigger::CiFailed { .. }));
        let fails = if wants_ci {
            failures(&app, &repo)
        } else {
            None
        };
        for a in &parsed.automations {
            let state = AutoState::load(&common, &a.name);
            let cx = auto::Context {
                now: now(),
                offset,
                task_open: task_open(&tasks, &state),
                failures: fails.as_deref(),
            };
            let out = auto::evaluate(a, &state, &cx);
            // Saved before the task starts: a crash in between loses a run
            // rather than starting it twice.
            if out.state != state && out.state.save(&common, &a.name).is_err() {
                continue;
            }
            let Some(go) = out.start else { continue };
            let mut next = out.state;
            match start(&repo, a, &go.prompt) {
                Ok(id) => {
                    next.task = Some(id.clone());
                    next.started = Some(now());
                    started.push(Started {
                        repo: root.clone(),
                        automation: a.name.clone(),
                        task: id,
                    });
                }
                Err(e) => next.note = Some(format!("couldn't start: {}", first_line(&e))),
            }
            let _ = next.save(&common, &a.name);
        }
    }
    if !started.is_empty() {
        let _ = handle.emit("work-changed", ());
    }
    started
}

#[derive(Serialize)]
pub(crate) struct AutomationInfo {
    name: String,
    /// The trigger as written after `on:`.
    on: String,
    prompt: String,
    enabled: bool,
    /// When it last started a task (seconds), and that task.
    started: Option<i64>,
    task: Option<String>,
    /// Whether that task is still open (so the next start is held back).
    task_open: bool,
    /// Why it last held back or failed.
    note: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Automations {
    items: Vec<AutomationInfo>,
    /// Blocks of the file that couldn't be read, one line each.
    errors: Vec<String>,
}

#[tauri::command(async)]
pub(crate) fn automations(root: String) -> Result<Automations, String> {
    let repo = discover(&root).ok_or("Not a repository")?;
    let parsed = read(&repo);
    let common = repo.common_dir()?;
    let tasks = Tasks::open(&repo)?;
    let items = parsed
        .automations
        .iter()
        .map(|a| {
            let state = AutoState::load(&common, &a.name);
            AutomationInfo {
                name: a.name.clone(),
                on: a.trigger.describe(),
                prompt: a.prompt.clone(),
                enabled: !state.disabled,
                task_open: task_open(&tasks, &state),
                started: state.started,
                task: state.task,
                note: state.note,
            }
        })
        .collect();
    Ok(Automations {
        items,
        errors: parsed.errors,
    })
}

#[tauri::command(async)]
pub(crate) fn automation_enable(root: String, name: String, enabled: bool) -> Result<(), String> {
    let repo = discover(&root).ok_or("Not a repository")?;
    let common = repo.common_dir()?;
    if !read(&repo).automations.iter().any(|a| a.name == name) {
        return Err(format!("No automation {name}"));
    }
    let mut state = AutoState::load(&common, &name);
    state.disabled = !enabled;
    state.save(&common, &name)
}

/// Start an automation's task now, whatever its trigger. Refused while its
/// last task is still open, like a trigger would be. Triggers that depend
/// on a run have nothing to fill in, so their `{…}` stay as written.
#[tauri::command(async)]
pub(crate) fn automation_run(
    handle: AppHandle,
    root: String,
    name: String,
) -> Result<Started, String> {
    let repo = discover(&root).ok_or("Not a repository")?;
    let common = repo.common_dir()?;
    let parsed = read(&repo);
    let a = parsed
        .automations
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| format!("No automation {name}"))?;
    let mut state = AutoState::load(&common, &name);
    if task_open(&Tasks::open(&repo)?, &state) {
        return Err(format!("{name}'s last task is still open"));
    }
    let prompt = auto::fill(&a.prompt, None, now(), local_offset());
    let id = start(&repo, a, &prompt)?;
    state.task = Some(id.clone());
    state.started = Some(now());
    state.note = None;
    state.save(&common, &name)?;
    let _ = handle.emit("work-changed", ());
    Ok(Started {
        repo: root,
        automation: name,
        task: id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    }

    /// A due automation with `agent: none` makes a task, remembers it, and
    /// is then held back while that task is open.
    #[test]
    fn starts_a_task_once_and_waits_for_it() {
        let dir = std::env::temp_dir().join(format!("spor-auto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".spor")).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "t@example.com"]);
        git(&dir, &["config", "user.name", "T"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        std::fs::write(
            dir.join(auto::FILE),
            "name: daily\non: schedule daily 09:00\nagent: none\nprompt: Tidy up on {date}.\n",
        )
        .unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "first"]);

        let repo = Repo::discover(&dir).unwrap();
        let common = repo.common_dir().unwrap();
        let tasks = Tasks::open(&repo).unwrap();
        let a = read(&repo).automations.remove(0);
        let at = |now: i64, state: &AutoState| {
            let open = task_open(&tasks, state);
            auto::evaluate(
                &a,
                state,
                &auto::Context {
                    now,
                    offset: 0,
                    task_open: open,
                    failures: None,
                },
            )
        };

        let day = 1_794_657_600; // 2026-11-14 12:00 UTC
        let armed = at(day, &AutoState::default()).state;
        assert!(at(day + 60, &armed).start.is_none());

        let next = at(day + 86_400, &armed);
        let go = next.start.expect("due the next morning");
        assert_eq!(go.prompt, "Tidy up on 2026-11-15.");
        let id = start(&repo, &a, &go.prompt).unwrap();
        let mut state = next.state;
        state.task = Some(id.clone());
        state.save(&common, "daily").unwrap();
        let state = AutoState::load(&common, "daily");
        assert_eq!(tasks.get(&id).unwrap().prompt, "Tidy up on 2026-11-15.");

        // The morning after, its task is still open: held back.
        let held = at(day + 2 * 86_400, &state);
        assert!(held.start.is_none());
        assert!(held.state.note.unwrap().contains("still open"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(dir.with_extension("spor"));
    }

    #[test]
    fn reads_zone_offsets() {
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0330"), Some(-12_600));
        assert_eq!(parse_offset("+0000"), Some(0));
        assert_eq!(parse_offset(""), None);
        assert_eq!(parse_offset("UTC"), None);
    }
}
