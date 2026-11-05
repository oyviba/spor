//! The Tauri app's backend: spor's core (`spor::git`, `spor::tasks`,
//! `spor::agent`, `spor::github`, `spor::graph`, `spor::askpass`) exposed as
//! commands, one module per section of the app. Commands stay thin: the work is in the
//! core, and these only turn it into JSON and record what Undo needs.

mod agents;
mod changes;
mod github;
mod history;
mod undo;

use serde::Serialize;
use spor::git::Repo;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

#[derive(Default)]
pub(crate) struct App {
    repo: Mutex<Option<Repo>>,
    undo: undo::Stack,
    /// Credential prompts from git and ssh waiting for the user, by id.
    /// Dropping one answers Cancel, so they're kept until answered.
    asks: Mutex<(u64, std::collections::BTreeMap<u64, spor::askpass::Request>)>,
}

impl App {
    pub(crate) fn repo(&self) -> Result<Repo, String> {
        self.repo
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "Open a repository first".to_string())
    }
}

#[derive(Serialize)]
struct RepoInfo {
    root: String,
    name: String,
    branch: Option<String>,
    branches: Vec<String>,
    default_base: Option<String>,
    claude: bool,
}

fn repo_info(repo: &Repo) -> RepoInfo {
    let mut branches: Vec<String> = repo
        .list_branches()
        .unwrap_or_default()
        .into_iter()
        .filter(|b| !b.is_remote && !b.name.starts_with("agent/"))
        .map(|b| b.name)
        .collect();
    branches.sort();
    RepoInfo {
        root: repo.root().display().to_string(),
        name: repo
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        branch: repo.tracking().ok().and_then(|t| t.branch),
        branches,
        default_base: repo.default_base_branch(),
        claude: spor::agent::available("claude"),
    }
}

#[tauri::command(async)]
fn current_repo(app: State<App>) -> Option<RepoInfo> {
    app.repo.lock().unwrap().as_ref().map(repo_info)
}

#[tauri::command(async)]
fn open_repo(app: State<App>, path: String) -> Result<RepoInfo, String> {
    let path = expand_home(path.trim());
    let repo = Repo::discover(&path).map_err(|e| first_line(&e))?;
    let info = repo_info(&repo);
    *app.repo.lock().unwrap() = Some(repo);
    app.undo.clear();
    Ok(info)
}

/// What the window chrome shows about the repository, and fingerprints the
/// sections compare to know when to reload: `refs` changes when any branch,
/// tag or HEAD moves; `index` when anything is staged, committed or checked
/// out. Cheap enough to poll every couple of seconds.
#[derive(Serialize)]
struct RepoState {
    branch: Option<String>,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
    /// "merge", "rebase", "cherrypick" or "revert" while one is stopped.
    operation: Option<String>,
    conflicts: Vec<String>,
    refs: String,
    index: String,
    undo: Option<String>,
}

#[tauri::command(async)]
fn repo_state(app: State<App>) -> Result<RepoState, String> {
    let repo = app.repo()?;
    let tracking = repo.tracking()?;
    let operation = repo.operation();
    let git = |args: &[&str]| -> String {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo.root())
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let refs = fingerprint(&(
        git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
        git(&["rev-parse", "HEAD"]),
        tracking.branch.clone(),
    ));
    // The worktree's own git dir: a linked worktree keeps its index there.
    let git_dir = PathBuf::from(git(&["rev-parse", "--absolute-git-dir"]).trim());
    let index_mtime = std::fs::metadata(git_dir.join("index"))
        .and_then(|m| m.modified())
        .ok();
    Ok(RepoState {
        branch: tracking.branch,
        upstream: tracking.upstream,
        ahead: tracking.ahead,
        behind: tracking.behind,
        operation: operation.map(|o| format!("{o:?}").to_lowercase()),
        conflicts: if operation.is_some() {
            repo.conflicts().unwrap_or_default()
        } else {
            Vec::new()
        },
        index: fingerprint(&(index_mtime, &refs)),
        refs,
        undo: app.undo.peek(),
    })
}

#[tauri::command(async)]
fn undo(app: State<App>) -> Result<String, String> {
    let repo = app.repo()?;
    app.undo.pop(&repo)
}

/// Open a web page (a PR or a run on GitHub) in the default browser. Only
/// https links: the page can't use this to start other programs.
#[tauri::command(async)]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Only https links can be opened".into());
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't open the link: {e}"))
}

#[derive(Serialize)]
struct Credential {
    id: u64,
    prompt: String,
    /// Typed hidden (a passphrase or password, not a username).
    secret: bool,
    /// ssh asking yes/no whether to trust a host it hasn't seen.
    confirm: bool,
}

/// The oldest credential prompt from a fetch, pull or push that's waiting
/// for the user, if any. The window polls this.
#[tauri::command(async)]
fn askpass_pending(app: State<App>) -> Option<Credential> {
    let mut asks = app.asks.lock().unwrap();
    while let Some(request) = spor::askpass::next_request() {
        asks.0 += 1;
        let id = asks.0;
        asks.1.insert(id, request);
    }
    asks.1.iter().next().map(|(id, r)| Credential {
        id: *id,
        prompt: r.prompt.clone(),
        secret: r.is_secret(),
        confirm: r.is_confirmation(),
    })
}

/// Answer a credential prompt; `None` cancels it.
#[tauri::command(async)]
fn askpass_answer(app: State<App>, id: u64, answer: Option<String>) {
    if let Some(request) = app.asks.lock().unwrap().1.remove(&id) {
        request.answer(answer);
    }
}

/// The repositories the old native app (GPUI) knew, from its workspaces
/// file, so switching apps doesn't lose the list. Existing folders only.
#[tauri::command(async)]
fn legacy_repos() -> Vec<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
    };
    let Some(file) = base.map(|b| b.join("spor").join("workspaces")) else {
        return Vec::new();
    };
    let mut repos: Vec<String> = Vec::new();
    for line in std::fs::read_to_string(file).unwrap_or_default().lines() {
        if let Some(path) = line.strip_prefix("repo\t") {
            if std::path::Path::new(path).is_dir() && !repos.iter().any(|r| r == path) {
                repos.push(path.to_string());
            }
        }
    }
    repos
}

fn fingerprint(value: &impl Hash) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    format!("{:x}", h.finish())
}

pub(crate) fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .trim_start_matches("error: ")
        .trim_start_matches("fatal: ")
        .to_string()
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest.trim_start_matches('/')),
        _ => PathBuf::from(path),
    }
}

/// An enum's variant name as the UI spells it: `ChangesRequested` →
/// `changesrequested`.
pub(crate) fn name_of(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}").to_lowercase()
}

pub fn run() {
    // Claude Code runs under this same executable as its host, so agents
    // can ask before acting and be steered (spor::host, #20).
    if let Ok(me) = std::env::current_exe() {
        spor::host::set_program(me);
    }
    let app = App::default();
    // The repository to open: the first argument, else $SPOR_REPO, else the
    // current directory if it's in one.
    let start = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("SPOR_REPO").ok())
        .map(|p| expand_home(&p))
        .or_else(|| std::env::current_dir().ok());
    if let Some(repo) = start.and_then(|p| Repo::discover(&p).ok()) {
        *app.repo.lock().unwrap() = Some(repo);
    }
    tauri::Builder::default()
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            current_repo,
            open_repo,
            repo_state,
            undo,
            open_url,
            askpass_pending,
            askpass_answer,
            legacy_repos,
            agents::list_tasks,
            agents::task_detail,
            agents::task_patch,
            agents::create_task,
            agents::send_to_agent,
            agents::stop_agent,
            agents::land_task,
            agents::discard_task,
            agents::open_folder,
            agents::run_patch,
            agents::undo_run,
            agents::answer_ask,
            agents::set_mode,
            agents::steer,
            agents::skip_setup,
            agents::create_issue_task,
            agents::publish_task,
            agents::push_task,
            agents::task_pulls,
            agents::review_feedback,
            agents::settle_task,
            history::graph,
            history::search_commits,
            history::commit_detail,
            history::branches,
            history::checkout,
            history::create_branch,
            history::delete_branch,
            history::cherry_pick,
            history::revert,
            history::merge_branch,
            history::rename_branch,
            history::delete_remote_branch,
            history::rebase_onto,
            history::reset_to,
            history::checkout_commit,
            history::tags,
            history::create_tag,
            history::delete_tag,
            history::push_tag,
            history::worktrees,
            history::remove_worktree,
            history::rewrite_plan,
            history::rewrite,
            history::pr_badges,
            history::pull_request_url,
            history::blame,
            changes::status,
            changes::file_diff,
            changes::stage,
            changes::unstage,
            changes::stage_lines,
            changes::discard,
            changes::commit,
            changes::last_message,
            changes::sync,
            changes::stashes,
            changes::stash,
            changes::stash_pop,
            changes::stash_apply,
            changes::stash_drop,
            changes::skip_operation,
            changes::conflict_regions,
            changes::resolve_regions,
            changes::auto_fetch,
            changes::resolve,
            changes::continue_operation,
            changes::abort_operation,
            github::pulls,
            github::pull_detail,
            github::pull_diff,
            github::pull_action,
            github::runs,
            github::workflows,
            github::run_detail,
            github::run_action,
            github::job_log,
            github::issues,
            github::issue_detail,
            github::issue_action,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Spor");
}
