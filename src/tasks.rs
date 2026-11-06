//! Agent tasks: a prompt with its own branch and working tree, so an agent
//! (or several) can work without touching your checkout or each other.
//!
//! A task's branch is `agent/<id>`, checked out in `<repo>.spor/<id>` beside
//! the repository. Tasks are saved in the git directory every worktree
//! shares (`.git/spor/tasks/`), so they stay out of commits. Spor doesn't
//! run agents yet: you run one in the task's folder, and spor shows what it
//! did, lands it on the base branch, or throws it away.

use crate::git::{Outcome, Repo, ResetMode};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    /// Being worked on, or waiting to be reviewed.
    Open,
    /// Merged into its base as this commit; its branch and worktree are gone.
    Landed(String),
    /// Its pull request was closed without merging; its branch and worktree
    /// are gone.
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Short and readable, from the prompt: `fix-login-redirect`.
    pub id: String,
    pub prompt: String,
    /// The branch it started from and lands on.
    pub base: String,
    pub branch: String,
    pub worktree: PathBuf,
    /// Unix time it was created.
    pub created: i64,
    pub state: TaskState,
    /// The GitHub issue it was started from.
    pub issue: Option<u64>,
    /// The pull request its branch was published as.
    pub pull: Option<u64>,
}

impl Task {
    /// The task's working tree as a repository.
    pub fn repo(&self) -> Repo {
        Repo::new(&self.worktree)
    }

    /// The prompt's first line, for lists and commit subjects.
    pub fn title(&self) -> String {
        let line = self.prompt.trim().lines().next().unwrap_or("").trim();
        if line.chars().count() > 72 {
            line.chars().take(71).chain(['…']).collect()
        } else {
            line.to_string()
        }
    }

    /// The landing commit's message: the title, the prompt (an issue's
    /// whole text is left out: the issue has it), `Closes #n`, and the
    /// `Spor-Task` trailer.
    pub fn land_message(&self) -> String {
        let mut parts = vec![self.title()];
        match self.issue {
            Some(n) => parts.push(format!("Closes #{n}")),
            None if self.prompt.trim() != self.title() => {
                parts.push(self.prompt.trim().to_string())
            }
            None => {}
        }
        parts.push(format!("Spor-Task: {}", self.id));
        parts.join("\n\n")
    }

    /// The description of the pull request it's published as: `Closes #n`
    /// for an issue, then `description` (one written for the work), else
    /// the rest of the prompt (an issue's text is left out: the issue has
    /// it).
    pub fn pull_request_body(&self, description: Option<&str>) -> String {
        let mut parts = Vec::new();
        if let Some(n) = self.issue {
            parts.push(format!("Closes #{n}"));
        }
        match (description.map(str::trim), self.issue) {
            (Some(d), _) if !d.is_empty() => parts.push(d.to_string()),
            (_, Some(_)) => {}
            _ => {
                let rest = self
                    .prompt
                    .trim()
                    .lines()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join("\n");
                if !rest.trim().is_empty() {
                    parts.push(rest.trim().to_string());
                }
            }
        }
        parts.push(format!(
            "---\n_Made by an agent in spor task `{}`._",
            self.id
        ));
        parts.join("\n\n")
    }

    /// A shell command that starts Claude Code on the prompt in the task's
    /// folder, for pasting into a terminal.
    pub fn agent_command(&self) -> String {
        format!(
            "cd {} && claude {}",
            shell_quote(&self.worktree.to_string_lossy()),
            shell_quote(self.prompt.trim())
        )
    }
}

/// What a task has done so far, read from git.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStats {
    /// Commits on the task's branch since it left the base.
    pub commits: usize,
    /// Files changed in its working tree but not committed.
    pub uncommitted: usize,
    /// Across commits and uncommitted changes, against where it started.
    pub files: usize,
    pub additions: usize,
    pub deletions: usize,
    /// The base has commits the task doesn't, so landing rebases first.
    pub base_moved: bool,
    /// Unix time of its last commit.
    pub last_commit: i64,
    /// Its folder is gone (deleted by hand, say).
    pub missing: bool,
    /// Commits its published branch doesn't have yet (0 when unpublished).
    pub unpushed: usize,
}

impl TaskStats {
    pub fn is_empty(&self) -> bool {
        self.commits == 0 && self.uncommitted == 0
    }
}

/// How a task's work goes onto its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandMode {
    /// One commit with the task's prompt as its message.
    Squash,
    /// A merge commit, keeping the task's own commits.
    Merge,
}

/// Enough to take back a land or a discard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undoable {
    pub task: Task,
    /// Where the task's branch was.
    pub branch_sha: String,
    /// Where the base was before landing, for a land.
    pub base_before: Option<String>,
}

/// The tasks of one repository.
#[derive(Debug, Clone)]
pub struct Tasks {
    repo: Repo,
    dir: PathBuf,
}

impl Tasks {
    /// The tasks of the repository `repo` is a checkout of.
    pub fn open(repo: &Repo) -> Result<Self, String> {
        let dir = repo.common_dir()?.join("spor").join("tasks");
        Ok(Self {
            repo: repo.clone(),
            dir,
        })
    }

    /// Open tasks first (newest first), then landed and closed ones.
    pub fn list(&self) -> Vec<Task> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut tasks: Vec<Task> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "task"))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|text| parse_task(&text))
            .collect();
        tasks.sort_by(|a, b| {
            let settled = |t: &Task| t.state != TaskState::Open;
            settled(a)
                .cmp(&settled(b))
                .then(b.created.cmp(&a.created))
                .then(a.id.cmp(&b.id))
        });
        tasks
    }

    pub fn get(&self, id: &str) -> Option<Task> {
        let text = std::fs::read_to_string(self.file(id)).ok()?;
        parse_task(&text)
    }

    /// Start a task: a branch from `base` checked out in a folder of its own.
    pub fn create(&self, prompt: &str, base: &str) -> Result<Task, String> {
        self.create_with(prompt, base, &slug(prompt), None)
    }

    /// Start a task on a GitHub issue: its id (and branch) begin with the
    /// issue's number, and it remembers the issue so its pull request and
    /// landing commit close it.
    pub fn create_for_issue(
        &self,
        prompt: &str,
        base: &str,
        issue: u64,
        title: &str,
    ) -> Result<Task, String> {
        let id = format!("{issue}-{}", slug(title));
        let id = id.trim_end_matches("-task");
        self.create_with(prompt, base, id, Some(issue))
    }

    fn create_with(
        &self,
        prompt: &str,
        base: &str,
        id: &str,
        issue: Option<u64>,
    ) -> Result<Task, String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Describe the task first".into());
        }
        let base_sha = self.repo.resolve(base)?;
        let id = self.unique_id(id);
        let branch = format!("agent/{id}");
        let worktree = self.worktree_dir()?.join(&id);
        std::fs::create_dir_all(worktree.parent().unwrap_or(&worktree))
            .map_err(|e| format!("Couldn't create {}: {e}", worktree.display()))?;
        self.repo
            .add_worktree(&worktree, &branch, Some(&base_sha))?;
        self.copy_ignored(&worktree);
        let task = Task {
            id,
            prompt: prompt.to_string(),
            base: base.to_string(),
            branch,
            worktree,
            created: now(),
            state: TaskState::Open,
            issue,
            pull: None,
        };
        self.save(&task)?;
        Ok(task)
    }

    /// The main checkout.
    pub fn root(&self) -> &Path {
        self.repo.root()
    }

    /// Copy the files listed in the new folder's `.spor/copy` (one path per
    /// line, `#` for comments) from the main checkout: ignored files a
    /// checkout doesn't bring, like `.env` (#23). Missing ones are skipped.
    fn copy_ignored(&self, worktree: &Path) {
        let Ok(list) = std::fs::read_to_string(worktree.join(".spor").join("copy")) else {
            return;
        };
        for rel in list
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            // Only paths inside the checkout.
            if Path::new(rel).is_absolute() || rel.split('/').any(|c| c == "..") {
                continue;
            }
            let from = self.repo.root().join(rel);
            let to = worktree.join(rel);
            if from.is_file() {
                if let Some(dir) = to.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::copy(&from, &to);
            }
        }
    }

    pub fn save(&self, task: &Task) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        std::fs::write(self.file(&task.id), render_task(task)).map_err(|e| e.to_string())
    }

    /// Forget a task entirely (its branch and folder are left alone).
    pub fn forget(&self, id: &str) -> Result<(), String> {
        let _ = std::fs::remove_dir_all(self.run_dir(id));
        crate::checkpoint::delete_all(
            self.repo.root(),
            &format!("{}/", crate::agent::checkpoint_prefix(id)),
        );
        self.unlist(id)
    }

    /// Drop a task from the list, keeping its agents' transcripts so an
    /// undo brings them back too.
    fn unlist(&self, id: &str) -> Result<(), String> {
        match std::fs::remove_file(self.file(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    /// Where a task's agent runs keep their logs (see `agent`).
    pub fn run_dir(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.d"))
    }

    /// Where images attached to prompts are kept: `.git/spor/attachments/`,
    /// beside the tasks, so they stay out of every folder and commit.
    pub fn attachments_dir(&self) -> PathBuf {
        self.dir.with_file_name("attachments")
    }

    /// Keep an image attached to a prompt, for an agent to look at. It's
    /// named by its content, so attaching the same picture twice keeps it
    /// once. PNG, JPEG, GIF and WebP only: what Claude can read.
    pub fn attach(&self, data: &[u8]) -> Result<PathBuf, String> {
        let ext = image_kind(data).ok_or("Only PNG, JPEG, GIF and WebP images can be attached")?;
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data.hash(&mut hasher);
        let dir = self.attachments_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{:016x}.{ext}", hasher.finish()));
        if !path.exists() {
            std::fs::write(&path, data).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }

    pub fn stats(&self, task: &Task) -> TaskStats {
        if task.state != TaskState::Open {
            return TaskStats::default();
        }
        let wt = task.repo();
        if !task.worktree.exists() {
            return TaskStats {
                missing: true,
                ..TaskStats::default()
            };
        }
        let start = wt
            .merge_base(&task.base, "HEAD")
            .unwrap_or_else(|_| task.base.clone());
        let (files, additions, deletions) = wt.numstat_since(&start);
        let status = wt.status().unwrap_or_default();
        let mut uncommitted: Vec<&str> = status.iter().map(|e| e.path.as_str()).collect();
        uncommitted.dedup();
        let untracked: Vec<&str> = status
            .iter()
            .filter(|e| e.status == crate::git::FileStatus::Untracked)
            .map(|e| e.path.as_str())
            .collect();
        // numstat leaves out untracked files: count their lines as added
        // (skipping big ones, which are probably not text).
        let untracked_lines: usize = untracked
            .iter()
            .filter_map(|p| {
                let path = task.worktree.join(p);
                let meta = std::fs::metadata(&path).ok()?;
                (meta.len() < 1 << 20).then_some(path)
            })
            .filter_map(|path| std::fs::read(path).ok())
            .map(|bytes| bytecount_lines(&bytes))
            .sum();
        TaskStats {
            commits: wt.count_commits(&format!("{start}..HEAD")),
            uncommitted: uncommitted.len(),
            files: files + untracked.len(),
            additions: additions + untracked_lines,
            deletions,
            base_moved: wt.count_commits(&format!("HEAD..{}", task.base)) > 0,
            last_commit: wt.commit_time("HEAD"),
            missing: false,
            unpushed: wt.count_commits("@{upstream}..HEAD"),
        }
    }

    /// Everything the task changed since it left its base, uncommitted work
    /// and new files included.
    pub fn patch(&self, task: &Task) -> Result<String, String> {
        let wt = task.repo();
        let start = wt.merge_base(&task.base, "HEAD")?;
        wt.patch_since(&start)
    }

    /// Delete a task's folder and branch. Uncommitted work in the folder is
    /// lost; the branch can come back with [`Tasks::restore`].
    pub fn discard(&self, task: &Task) -> Result<Undoable, String> {
        let branch_sha = self.repo.resolve(&task.branch)?;
        if task.worktree.exists() {
            self.repo.remove_worktree(&task.worktree, true)?;
        }
        self.repo.delete_branch(&task.branch, true)?;
        self.unlist(&task.id)?;
        Ok(Undoable {
            task: task.clone(),
            branch_sha,
            base_before: None,
        })
    }

    /// Bring back a discarded or landed task: its branch, its folder, and
    /// for a land, the base as it was.
    pub fn restore(&self, undo: &Undoable) -> Result<(), String> {
        if let Some(before) = &undo.base_before {
            let current = self.repo.tracking()?.branch;
            if current.as_deref() != Some(undo.task.base.as_str()) {
                return Err(format!("Switch back to {} first", undo.task.base));
            }
            self.repo.reset(before, ResetMode::Keep)?;
        }
        self.repo
            .restore_branch(&undo.task.branch, &undo.branch_sha)?;
        self.repo
            .add_worktree(&undo.task.worktree, &undo.task.branch, None)?;
        let task = Task {
            state: TaskState::Open,
            ..undo.task.clone()
        };
        self.save(&task)
    }

    /// Put the task's work on its base, which must be checked out here.
    /// The task's branch is rebased onto the base first if the base has
    /// moved, so landing itself never conflicts; with `commit_uncommitted`
    /// uncommitted work in its folder is committed first, otherwise it's
    /// refused. Afterwards the folder and branch are removed.
    pub fn land(
        &self,
        task: &Task,
        mode: LandMode,
        commit_uncommitted: bool,
    ) -> Result<Undoable, String> {
        let here = self.repo.tracking()?;
        if here.branch.as_deref() != Some(task.base.as_str()) {
            return Err(format!("Check out {} to land {} on it", task.base, task.id));
        }
        if self.repo.status()?.iter().any(|e| e.status.is_staged()) {
            return Err(format!(
                "{} has staged changes — commit or unstage them first",
                task.base
            ));
        }
        let wt = task.repo();
        let stats = self.stats(task);
        if stats.missing {
            return Err(format!("{}'s folder is gone", task.id));
        }
        if stats.uncommitted > 0 {
            if !commit_uncommitted {
                return Err(format!("{} has uncommitted changes", task.id));
            }
            wt.stage_all()?;
            wt.commit(&format!("Uncommitted work from {}", task.id))?;
        }
        if self
            .repo
            .count_commits(&format!("{}..{}", task.base, task.branch))
            == 0
        {
            return Err(format!("{} has nothing to land", task.id));
        }
        if stats.base_moved {
            match wt.rebase(&task.base)? {
                Outcome::Done => {}
                Outcome::Conflicts(files) => {
                    wt.abort_operation()?;
                    return Err(format!(
                        "{} conflicts with {} in {} — resolve that in its folder (or ask the agent), then land",
                        task.id,
                        task.base,
                        files.join(", ")
                    ));
                }
            }
        }
        let base_before = self.repo.head_sha()?;
        let branch_sha = self.repo.resolve(&task.branch)?;
        let message = task.land_message();
        let outcome = match mode {
            LandMode::Squash => {
                let o = self.repo.squash_merge(&task.branch)?;
                if o == Outcome::Done {
                    self.repo.commit(&message)?;
                }
                o
            }
            LandMode::Merge => self.repo.merge_no_ff(&task.branch, &message)?,
        };
        if let Outcome::Conflicts(files) = outcome {
            // Can't happen after the rebase, but never leave a half-merge.
            let _ = self.repo.reset(&base_before, ResetMode::Hard);
            return Err(format!("Landing conflicted in {}", files.join(", ")));
        }
        let commit = self.repo.head_sha()?;
        self.repo.remove_worktree(&task.worktree, true)?;
        self.repo.delete_branch(&task.branch, true)?;
        self.save(&Task {
            state: TaskState::Landed(commit),
            ..task.clone()
        })?;
        Ok(Undoable {
            task: task.clone(),
            branch_sha,
            base_before: Some(base_before),
        })
    }

    /// Push the task's branch to `remote` and open a pull request for it
    /// on GitHub, into its base. With `commit_uncommitted`, uncommitted
    /// work in its folder is committed first; otherwise it's refused. With
    /// `describe`, Claude Code writes the PR's title and description from
    /// the work (and the agent's session); if it can't, the PR gets the
    /// prompt's. The task remembers the PR. Returns its number and URL.
    pub fn publish(
        &self,
        task: &Task,
        remote: &str,
        draft: bool,
        commit_uncommitted: bool,
        describe: bool,
    ) -> Result<(u64, String), String> {
        if task.state != TaskState::Open {
            return Err(format!("{} is already settled", task.id));
        }
        let stats = self.stats(task);
        if stats.missing {
            return Err(format!("{}'s folder is gone", task.id));
        }
        let wt = task.repo();
        if stats.uncommitted > 0 {
            if !commit_uncommitted {
                return Err(format!("{} has uncommitted changes", task.id));
            }
            wt.stage_all()?;
            wt.commit(&format!("Uncommitted work from {}", task.id))?;
        }
        if self
            .repo
            .count_commits(&format!("{}..{}", task.base, task.branch))
            == 0
        {
            return Err(format!("{} has no commits to publish", task.id));
        }
        self.push(task, remote)?;
        let described = describe
            .then(|| crate::agent::describe_pull(self, task).ok())
            .flatten();
        let (title, description) = match &described {
            Some((t, d)) => (t.clone(), Some(d.as_str())),
            None => (task.title(), None),
        };
        let (number, url) = crate::github::create_pull(
            &task.worktree,
            &task.branch,
            &task.base,
            &title,
            &task.pull_request_body(description),
            draft,
        )?;
        self.save(&Task {
            pull: Some(number),
            ..task.clone()
        })?;
        Ok((number, url))
    }

    /// Push the task's branch to `remote` (setting it as the upstream), so
    /// its pull request gets the agent's latest commits. A key passphrase
    /// or password comes up in the app, as for any other push.
    pub fn push(&self, task: &Task, remote: &str) -> Result<(), String> {
        if task.state != TaskState::Open || !task.worktree.exists() {
            return Err(format!("{} has no folder to push from", task.id));
        }
        crate::askpass::git_network(
            &task.worktree,
            &["push", "--set-upstream", remote, &task.branch],
        )
        .map(drop)
    }

    /// Remember the pull request a task's branch was published as (one
    /// opened outside spor, found by its branch).
    pub fn link_pull(&self, task: &Task, number: u64) -> Result<(), String> {
        if task.pull == Some(number) {
            return Ok(());
        }
        self.save(&Task {
            pull: Some(number),
            ..task.clone()
        })
    }

    /// The task's pull request was merged on the host as `commit`: mark it
    /// landed, and remove its folder and branch, which aren't needed any
    /// more. Refused while its folder has work the PR doesn't. Undoable
    /// with [`Tasks::restore`], like a discard.
    pub fn settle(&self, task: &Task, commit: &str) -> Result<Undoable, String> {
        self.settle_as(task, TaskState::Landed(commit.to_string()))
    }

    /// The task's pull request was closed without merging: mark it closed,
    /// and remove its folder and branch, as [`Tasks::settle`] does.
    pub fn close(&self, task: &Task) -> Result<Undoable, String> {
        self.settle_as(task, TaskState::Closed)
    }

    fn settle_as(&self, task: &Task, state: TaskState) -> Result<Undoable, String> {
        if task.state != TaskState::Open {
            return Err(format!("{} is already settled", task.id));
        }
        let branch_sha = self.repo.resolve(&task.branch)?;
        if task.worktree.exists() {
            if !task.repo().status()?.is_empty() {
                return Err(format!(
                    "{} has uncommitted changes the pull request doesn't have — commit or discard them first",
                    task.id
                ));
            }
            self.repo.remove_worktree(&task.worktree, false)?;
        }
        self.repo.delete_branch(&task.branch, true)?;
        self.save(&Task {
            state,
            ..task.clone()
        })?;
        Ok(Undoable {
            task: task.clone(),
            branch_sha,
            base_before: None,
        })
    }

    /// `<parent>/<repo>.spor/`: beside the repository, so editors and
    /// build tools treat each task's folder as an ordinary checkout.
    fn worktree_dir(&self) -> Result<PathBuf, String> {
        let common = self.repo.common_dir()?;
        let common = common.canonicalize().unwrap_or(common);
        // The main checkout is the common dir's parent (`repo/.git`).
        let main = common.parent().unwrap_or(&common).to_path_buf();
        let name = main
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".into());
        let parent = main.parent().unwrap_or(Path::new("/"));
        Ok(parent.join(format!("{name}.spor")))
    }

    fn unique_id(&self, base: &str) -> String {
        let taken = |id: &str| {
            self.file(id).exists()
                || self.repo.resolve(&format!("refs/heads/agent/{id}")).is_ok()
                || self.worktree_dir().is_ok_and(|d| d.join(id).exists())
        };
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| !taken(id))
            .unwrap_or_else(|| base.to_string())
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.task"))
    }
}

/// Lines in a file's contents, counting a last line without a newline.
fn bytecount_lines(bytes: &[u8]) -> usize {
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    newlines + usize::from(bytes.last().is_some_and(|&b| b != b'\n'))
}

/// "Fix the login redirect on Safari!" → `fix-login-redirect-safari`: the
/// first few meaningful words, lowercase, ASCII letters and digits only.
pub fn slug(prompt: &str) -> String {
    const SKIP: &[&str] = &[
        "the", "a", "an", "to", "on", "in", "of", "for", "and", "so", "it",
    ];
    let words: Vec<String> = prompt
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|w| !w.is_empty() && !SKIP.contains(&w.as_str()))
        .take(4)
        .collect();
    let mut slug = words.join("-");
    slug.truncate(32);
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "task".into()
    } else {
        slug
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// One `key<TAB>value` line per field; values escape `\`, tab and newline.
fn render_task(t: &Task) -> String {
    let state = match &t.state {
        TaskState::Open => "open".to_string(),
        TaskState::Landed(sha) => format!("landed {sha}"),
        TaskState::Closed => "closed".to_string(),
    };
    let fields = [
        ("id", t.id.clone()),
        ("prompt", t.prompt.clone()),
        ("base", t.base.clone()),
        ("branch", t.branch.clone()),
        ("worktree", t.worktree.to_string_lossy().into_owned()),
        ("created", t.created.to_string()),
        ("state", state),
    ];
    let optional = [("issue", t.issue), ("pull", t.pull)];
    fields
        .iter()
        .map(|(k, v)| format!("{k}\t{}\n", escape(v)))
        .chain(
            optional
                .iter()
                .filter_map(|(k, v)| v.map(|n| format!("{k}\t{n}\n"))),
        )
        .collect()
}

fn parse_task(text: &str) -> Option<Task> {
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('\t'))
            .map(unescape)
    };
    let state = get("state")?;
    let state = match state.split_once(' ') {
        Some(("landed", sha)) => TaskState::Landed(sha.to_string()),
        None if state == "closed" => TaskState::Closed,
        _ => TaskState::Open,
    };
    Some(Task {
        id: get("id")?,
        prompt: get("prompt")?,
        base: get("base")?,
        branch: get("branch")?,
        worktree: PathBuf::from(get("worktree")?),
        created: get("created")?.parse().unwrap_or(0),
        state,
        issue: get("issue").and_then(|n| n.parse().ok()),
        pull: get("pull").and_then(|n| n.parse().ok()),
    })
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// An image's extension, from its first bytes.
fn image_kind(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("gif")
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attached_images_are_known_by_their_bytes() {
        assert_eq!(image_kind(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(image_kind(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(image_kind(b"GIF89a.."), Some("gif"));
        assert_eq!(image_kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_kind(b"<svg>"), None);
    }

    #[test]
    fn slugs() {
        assert_eq!(
            slug("Fix the login redirect on Safari!"),
            "fix-login-redirect-safari"
        );
        assert_eq!(slug("   "), "task");
        assert_eq!(slug("Ærlig talt: rydd opp i CSS"), "rlig-talt-rydd-opp");
        assert!(slug(&"word ".repeat(40)).len() <= 32);
    }

    #[test]
    fn task_file_round_trips() {
        let task = Task {
            id: "x".into(),
            prompt: "Line one\n\twith a tab \\ and a backslash".into(),
            base: "main".into(),
            branch: "agent/x".into(),
            worktree: PathBuf::from("/tmp/r.spor/x"),
            created: 42,
            state: TaskState::Landed("abc".into()),
            issue: Some(7),
            pull: Some(12),
        };
        assert_eq!(parse_task(&render_task(&task)), Some(task.clone()));
        let closed = Task {
            state: TaskState::Closed,
            ..task
        };
        assert_eq!(parse_task(&render_task(&closed)), Some(closed));
    }

    #[test]
    fn agent_command_quotes() {
        let task = Task {
            id: "x".into(),
            prompt: "Don't break it".into(),
            base: "main".into(),
            branch: "agent/x".into(),
            worktree: PathBuf::from("/tmp/my repo.spor/x"),
            created: 0,
            state: TaskState::Open,
            issue: None,
            pull: None,
        };
        assert_eq!(
            task.agent_command(),
            r"cd '/tmp/my repo.spor/x' && claude 'Don'\''t break it'"
        );
    }
}
