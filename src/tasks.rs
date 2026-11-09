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
    /// The branch it lands on (and its pull request goes into).
    pub base: String,
    /// Where it started when that isn't `base` itself: `origin/main`, so a
    /// task starts from what the remote has, not a local copy that may be
    /// behind. Its work is what it has on top of this.
    pub from: Option<String>,
    pub branch: String,
    pub worktree: PathBuf,
    /// Unix time it was created.
    pub created: i64,
    pub state: TaskState,
    /// The GitHub issue it was started from.
    pub issue: Option<u64>,
    /// The pull request its branch was published as.
    pub pull: Option<u64>,
    /// Not started yet (#110): `branch` and `worktree` are where the task
    /// will work, but neither exists until an agent first wants to write
    /// ([`Tasks::start_branch`]). A task that only reads never gets them.
    pub pending: bool,
}

impl Task {
    /// Whether the task made its branch (`agent/<id>`), as opposed to
    /// adopting one of yours ([`Tasks::adopt`]). Only a branch the task
    /// made goes when the task does; yours stays.
    pub fn owns_branch(&self) -> bool {
        self.branch.starts_with("agent/")
    }

    /// The task's working tree as a repository. For a pending task that's
    /// a folder that doesn't exist yet.
    pub fn repo(&self) -> Repo {
        Repo::new(&self.worktree)
    }

    /// The ref its work is measured from: `from`, or else `base`.
    pub fn start(&self) -> &str {
        self.from.as_deref().unwrap_or(&self.base)
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

    /// The id in a commit message's `Spor-Task: <id>` trailer, if it has one.
    pub fn trailer_id(message: &str) -> Option<&str> {
        message
            .lines()
            .rev()
            .find_map(|l| l.strip_prefix("Spor-Task:"))
            .map(str::trim)
            .filter(|id| !id.is_empty())
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
    /// `base` may be a remote-tracking branch (`origin/main`): the task
    /// starts there and lands on the local branch of that name.
    pub fn create(&self, prompt: &str, base: &str) -> Result<Task, String> {
        self.create_with(prompt, base, &self.fresh_name(""), None)
    }

    /// Start a task on a GitHub issue: its id (and branch) begin with the
    /// issue's number, and it remembers the issue so its pull request and
    /// landing commit close it. The rest of the name is random, like any
    /// task's, so it doesn't depend on the issue's text.
    pub fn create_for_issue(
        &self,
        prompt: &str,
        base: &str,
        issue: u64,
        _title: &str,
    ) -> Result<Task, String> {
        let id = self.fresh_name(&format!("{issue}-"));
        self.create_with(prompt, base, &id, Some(issue))
    }

    /// A name no task, branch or folder has: `prefix` and three random
    /// words (`brave-otter-jumps`), tried again if it collides; only if
    /// many in a row do, a number is added.
    fn fresh_name(&self, prefix: &str) -> String {
        let taken = |id: &str| self.unique_id(id) != id;
        let mut name = format!("{prefix}{}", random_name());
        for _ in 0..20 {
            if !taken(&name) {
                return name;
            }
            name = format!("{prefix}{}", random_name());
        }
        self.unique_id(&name)
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
        self.repo.resolve(base)?;
        let (base, from) = match self.repo.remote_branch(base) {
            Some(local) => (local, Some(base.to_string())),
            None => (base.to_string(), None),
        };
        let id = self.unique_id(id);
        // Only named: the branch and folder come with the first write.
        let branch = format!("agent/{id}");
        let worktree = self.worktree_dir()?.join(&id);
        let task = Task {
            id,
            prompt: prompt.to_string(),
            base,
            from,
            branch,
            worktree,
            created: now(),
            state: TaskState::Open,
            issue,
            pull: None,
            pending: true,
        };
        self.save(&task)?;
        Ok(task)
    }

    /// Give a pending task its branch and folder, from where its base is
    /// now (#110). Called when an agent first wants to write; a task that
    /// has them already is returned as it is.
    pub fn start_branch(&self, task: &Task) -> Result<Task, String> {
        if !task.pending || task.state != TaskState::Open {
            return Ok(task.clone());
        }
        let sha = self.repo.resolve(task.start())?;
        std::fs::create_dir_all(task.worktree.parent().unwrap_or(&task.worktree))
            .map_err(|e| format!("Couldn't create {}: {e}", task.worktree.display()))?;
        self.repo
            .add_worktree(&task.worktree, &task.branch, Some(&sha))?;
        self.copy_ignored(&task.worktree);
        let started = Task {
            pending: false,
            ..task.clone()
        };
        self.save(&started)?;
        Ok(started)
    }

    /// Hand one of your branches to an agent (#80, #82): a task whose folder
    /// has `branch` checked out, landing on `base`. If the branch is checked
    /// out in the main checkout, its uncommitted work (untracked files too)
    /// moves into the folder, and the main checkout switches to `base`.
    /// Returns the task and whether it moved the branch out of the main
    /// checkout, which [`Tasks::give_back`] puts back.
    pub fn adopt(&self, branch: &str, prompt: &str, base: &str) -> Result<(Task, bool), String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Say what the agent should do first".into());
        }
        if branch.starts_with("agent/") {
            return Err(format!("{branch} is already an agent's branch"));
        }
        self.repo
            .resolve(&format!("refs/heads/{branch}"))
            .map_err(|_| format!("There's no local branch {branch}"))?;
        if self
            .list()
            .iter()
            .any(|t| t.branch == branch && t.state == TaskState::Open)
        {
            return Err(format!("{branch} is already a task"));
        }
        let (base, from) = match self.repo.remote_branch(base) {
            Some(local) => (local, Some(base.to_string())),
            None => (base.to_string(), None),
        };
        if base == branch {
            return Err(format!(
                "{branch} is the base: start a branch from it first"
            ));
        }
        let here = self.repo.tracking()?.branch;
        let moving = here.as_deref() == Some(branch);
        if let Some(w) = self
            .repo
            .worktrees()?
            .into_iter()
            .find(|w| !w.is_main && w.branch.as_deref() == Some(branch))
        {
            return Err(format!(
                "{branch} is already checked out in {}",
                w.path.display()
            ));
        }

        // Out of the main checkout: its uncommitted work comes along.
        let mut stashed = false;
        if moving {
            if !self.repo.status()?.is_empty() {
                self.repo
                    .stash_save(Some(&format!("spor: hand {branch} to an agent")), true)?;
                stashed = true;
            }
            if let Err(e) = self.repo.checkout_branch(&base) {
                if stashed {
                    let _ = self.repo.stash_pop_index();
                }
                return Err(e);
            }
        }
        let put_back = |e: String| {
            if moving {
                let _ = self.repo.checkout_branch(branch);
                if stashed {
                    let _ = self.repo.stash_pop_index();
                }
            }
            e
        };

        let leaf = branch.rsplit('/').next().unwrap_or(branch);
        let id = self.unique_id(&slug(leaf));
        let worktree = self.worktree_dir().map_err(put_back)?.join(&id);
        std::fs::create_dir_all(worktree.parent().unwrap_or(&worktree))
            .map_err(|e| put_back(format!("Couldn't create {}: {e}", worktree.display())))?;
        self.repo
            .add_worktree(&worktree, branch, None)
            .map_err(put_back)?;
        self.copy_ignored(&worktree);
        if stashed {
            // The stash is shared by every checkout of the repository.
            Repo::new(&worktree).stash_pop_index()?;
        }
        let task = Task {
            id,
            prompt: prompt.to_string(),
            base,
            from,
            branch: branch.to_string(),
            worktree,
            created: now(),
            state: TaskState::Open,
            issue: None,
            pull: None,
            pending: false,
        };
        self.save(&task)?;
        Ok((task, moving))
    }

    /// Take back a branch handed to an agent with [`Tasks::adopt`]: the
    /// folder goes, its uncommitted work comes along, and the task is
    /// forgotten. With `checkout`, the branch is checked out in the main
    /// checkout again, where it was; otherwise the folder's uncommitted
    /// work is kept as a stash.
    pub fn give_back(&self, task: &Task, checkout: bool) -> Result<(), String> {
        if task.owns_branch() {
            return Err(format!("{} was never yours to take back", task.branch));
        }
        let mut stashed = false;
        if task.worktree.exists() {
            let folder = task.repo();
            if !folder.status()?.is_empty() {
                folder.stash_save(Some(&format!("spor: take back {}", task.branch)), true)?;
                stashed = true;
            }
            self.repo.remove_worktree(&task.worktree, true)?;
        }
        self.forget(&task.id)?;
        if checkout {
            self.repo.checkout_branch(&task.branch)?;
            if stashed {
                self.repo.stash_pop_index()?;
            }
        }
        Ok(())
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

    /// Clean up after tasks that are done (landed, closed or discarded,
    /// #41): their run checkpoints (`refs/spor/tasks/<id>/…`), and a
    /// discarded task's transcripts. Undo can't bring those back
    /// afterwards, so the app does this when nothing is left to undo (when
    /// it opens a repository) or when asked. Returns how many tasks it
    /// cleaned up after.
    pub fn clean_up(&self) -> usize {
        let listed = self.list();
        let open: std::collections::HashSet<&str> = listed
            .iter()
            .filter(|t| t.state == TaskState::Open)
            .map(|t| t.id.as_str())
            .collect();
        let known: std::collections::HashSet<&str> = listed.iter().map(|t| t.id.as_str()).collect();
        let mut ids = std::collections::BTreeSet::new();
        // Tasks with checkpoints.
        let prefix = crate::agent::checkpoint_prefix("");
        ids.extend(
            crate::checkpoint::list(self.repo.root(), &prefix)
                .iter()
                .filter_map(|r| r.strip_prefix(&prefix))
                .filter_map(|r| r.split('/').next())
                .filter(|id| !open.contains(id))
                .map(String::from),
        );
        // Discarded tasks' transcripts.
        let mut gone_dirs = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(id) = name.strip_suffix(".d") {
                    if !known.contains(id) {
                        ids.insert(id.to_string());
                        gone_dirs.push(e.path());
                    }
                }
            }
        }
        for id in &ids {
            crate::checkpoint::delete_all(
                self.repo.root(),
                &format!("{}/", crate::agent::checkpoint_prefix(id)),
            );
        }
        for dir in gone_dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
        ids.len()
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

    /// The lock that makes counting running agents and starting one a
    /// single step, shared by every window (#limit).
    pub fn agents_lock(&self) -> PathBuf {
        self.dir.join("agents.lock")
    }

    /// How many tasks have an agent running now.
    pub fn running_agents(&self) -> usize {
        self.list()
            .into_iter()
            .filter(|t| t.state == TaskState::Open)
            .filter(|t| {
                crate::agent::Runs::of(self, t)
                    .latest()
                    .is_some_and(|r| r.state == crate::agent::RunState::Running)
            })
            .count()
    }

    /// Tasks whose start is waiting for a free slot, longest first.
    pub fn waiting(&self) -> Vec<Task> {
        let mut waiting: Vec<(i64, Task)> = self
            .list()
            .into_iter()
            .filter(|t| t.state == TaskState::Open)
            .filter_map(|t| {
                let since = crate::agent::Runs::of(self, &t).waiting()?.since;
                Some((since, t))
            })
            .collect();
        waiting.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.created.cmp(&b.1.created)));
        waiting.into_iter().map(|(_, t)| t).collect()
    }

    /// Start waiting tasks, longest first, while there are free slots.
    /// Returns how many went.
    pub fn start_waiting(&self) -> usize {
        let mut started = 0;
        for task in self.waiting() {
            match crate::agent::Runs::of(self, &task).start_waiting(&task) {
                Ok(true) => started += 1,
                // Full: the rest wait too.
                Ok(false) => break,
                Err(_) => {}
            }
        }
        started
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
        if task.pending {
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
            .merge_base(task.start(), "HEAD")
            .unwrap_or_else(|_| task.start().to_string());
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
            base_moved: wt.count_commits(&format!("HEAD..{}", task.start())) > 0,
            last_commit: wt.commit_time("HEAD"),
            missing: false,
            unpushed: wt.count_commits("@{upstream}..HEAD"),
        }
    }

    /// What landing the task (or merging its pull request) would take away
    /// from its base that came to the base after the task started: lines
    /// added there since, that the result wouldn't have. That's almost
    /// always files written back from an older copy after a rebase or a
    /// merge, which quietly reverts other work. Lines that moved elsewhere
    /// don't count. Per file, most lines first.
    /// Nothing when the merge would conflict (that shows as conflicts) or
    /// the branch's reflog doesn't say where it started.
    pub fn undoes(&self, task: &Task) -> Vec<(String, usize)> {
        if task.state != TaskState::Open || !task.worktree.exists() {
            return Vec::new();
        }
        // Where the branch was created from, by its reflog.
        let Some(start) = self.repo.first_logged(&task.branch) else {
            return Vec::new();
        };
        let arrived: std::collections::HashSet<String> = self
            .repo
            .commits_in(&format!("{start}..{}", task.base))
            .into_iter()
            .collect();
        if arrived.is_empty() {
            return Vec::new();
        }
        let Some(tree) = task.repo().merged_tree(&task.base, "HEAD") else {
            return Vec::new();
        };
        let touched = self.repo.changed_paths(&start, &task.base);
        // A line that's somewhere else in the result moved; it didn't go.
        let added = self.repo.added_lines(&task.base, &tree);
        let mut undone: Vec<(String, usize)> = self
            .repo
            .removed_lines(&task.base, &tree, &touched)
            .into_iter()
            .filter_map(|(path, runs)| {
                let n = self
                    .repo
                    .blame_runs(&task.base, &path, &runs)
                    .iter()
                    .filter(|l| arrived.contains(&l.hash) && !added.contains(l.text.trim()))
                    .count();
                (n > 0).then_some((path, n))
            })
            .collect();
        undone.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        undone
    }

    /// Everything the task changed since it left its base, uncommitted work
    /// and new files included.
    pub fn patch(&self, task: &Task) -> Result<String, String> {
        if task.pending {
            return Ok(String::new());
        }
        let wt = task.repo();
        let start = wt.merge_base(task.start(), "HEAD")?;
        wt.patch_since(&start)
    }

    /// Delete a task's folder and its branch (only the folder for a branch
    /// it adopted). Uncommitted work in the folder is lost; the branch can
    /// come back with [`Tasks::restore`].
    pub fn discard(&self, task: &Task) -> Result<Undoable, String> {
        if task.pending {
            // Nothing was made: forgetting the task is all there is.
            self.unlist(&task.id)?;
            return Ok(Undoable {
                task: task.clone(),
                branch_sha: String::new(),
                base_before: None,
            });
        }
        let branch_sha = self.repo.resolve(&task.branch)?;
        if task.worktree.exists() {
            self.repo.remove_worktree(&task.worktree, true)?;
        }
        self.drop_branch(task)?;
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
        if undo.task.pending {
            return self.save(&Task {
                state: TaskState::Open,
                ..undo.task.clone()
            });
        }
        if let Some(before) = &undo.base_before {
            let current = self.repo.tracking()?.branch;
            if current.as_deref() != Some(undo.task.base.as_str()) {
                return Err(format!("Switch back to {} first", undo.task.base));
            }
            self.repo.reset(before, ResetMode::Keep)?;
        }
        if undo.task.owns_branch()
            || self
                .repo
                .resolve(&format!("refs/heads/{}", undo.task.branch))
                .is_err()
        {
            self.repo
                .restore_branch(&undo.task.branch, &undo.branch_sha)?;
        }
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
    /// moved, so landing itself never conflicts. A task started from the
    /// remote's copy of the base (`from`) needs the base to have what it
    /// started from, or the squash would carry the remote's commits too:
    /// pull first. With `commit_uncommitted`
    /// uncommitted work in its folder is committed first, otherwise it's
    /// refused. Afterwards the folder and branch are removed.
    pub fn land(
        &self,
        task: &Task,
        mode: LandMode,
        commit_uncommitted: bool,
    ) -> Result<Undoable, String> {
        if task.pending {
            return Err(format!("{} hasn't changed anything to merge", task.id));
        }
        let here = self.repo.tracking()?;
        if here.branch.as_deref() != Some(task.base.as_str()) {
            return Err(format!(
                "Check out {} to merge {} into it",
                task.base, task.id
            ));
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
            commit_all(&wt, task)?;
        }
        if self
            .repo
            .count_commits(&format!("{}..{}", task.base, task.branch))
            == 0
        {
            return Err(format!("{} has nothing to merge", task.id));
        }
        if let Some(from) = &task.from {
            let fork = self.repo.merge_base(from, &task.branch)?;
            let missing = self.repo.count_commits(&format!("{}..{fork}", task.base));
            if missing > 0 {
                return Err(format!(
                    "{} is {missing} {} behind {from}, where {} started — pull {} first, then merge",
                    task.base,
                    if missing == 1 { "commit" } else { "commits" },
                    task.id,
                    task.base
                ));
            }
        }
        if wt.count_commits(&format!("HEAD..{}", task.base)) > 0 {
            match wt.rebase(&task.base)? {
                Outcome::Done => {}
                Outcome::Conflicts(files) => {
                    wt.abort_operation()?;
                    return Err(format!(
                        "{} conflicts with {} in {} — resolve that in its folder (or ask the agent), then merge",
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
            return Err(format!("Merging conflicted in {}", files.join(", ")));
        }
        let commit = self.repo.head_sha()?;
        self.repo.remove_worktree(&task.worktree, true)?;
        self.drop_branch(task)?;
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
        if task.pending {
            return Err(format!("{} has no commits to publish", task.id));
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
            commit_all(&wt, task)?;
        }
        if self
            .repo
            .count_commits(&format!("{}..{}", task.start(), task.branch))
            == 0
        {
            return Err(format!("{} has no commits to publish", task.id));
        }
        self.push(task, remote, false)?;
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
    /// Push the task's branch to `remote`. With `commit_uncommitted`,
    /// uncommitted work in its folder is committed first, so it goes too.
    pub fn push(&self, task: &Task, remote: &str, commit_uncommitted: bool) -> Result<(), String> {
        if task.state != TaskState::Open || task.pending || !task.worktree.exists() {
            return Err(format!("{} has no folder to push from", task.id));
        }
        let wt = task.repo();
        if commit_uncommitted && !wt.status()?.is_empty() {
            commit_all(&wt, task)?;
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
        if task.pending {
            self.save(&Task {
                state,
                ..task.clone()
            })?;
            return Ok(Undoable {
                task: task.clone(),
                branch_sha: String::new(),
                base_before: None,
            });
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
        self.drop_branch(task)?;
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
    /// Delete the task's branch when it's the task's own; an adopted
    /// branch is yours and stays.
    fn drop_branch(&self, task: &Task) -> Result<(), String> {
        if task.owns_branch() {
            self.repo.delete_branch(&task.branch, true)?;
        }
        Ok(())
    }

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
/// Commit everything in the task's folder, as work the agent left uncommitted.
fn commit_all(wt: &Repo, task: &Task) -> Result<(), String> {
    wt.stage_all()?;
    wt.commit(&format!("Uncommitted work from {}", task.id))
}

fn bytecount_lines(bytes: &[u8]) -> usize {
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    newlines + usize::from(bytes.last().is_some_and(|&b| b != b'\n'))
}

const ADJECTIVES: &[&str] = &[
    "brave", "calm", "clever", "cosmic", "crisp", "curious", "daring", "eager", "fancy", "fierce",
    "gentle", "giddy", "golden", "happy", "humble", "jolly", "keen", "kind", "lively", "lucky",
    "mellow", "merry", "mighty", "nimble", "noble", "odd", "patient", "plucky", "polite", "proud",
    "quick", "quiet", "rapid", "rosy", "shiny", "silent", "sleepy", "smooth", "snappy", "sneaky",
    "solid", "speedy", "spry", "steady", "stout", "sunny", "swift", "tidy", "tiny", "vivid",
    "warm", "wild", "wise", "witty", "zany", "zesty", "amber", "bold", "bright", "dusty", "fuzzy",
    "grand", "lone", "red",
];
const NOUNS: &[&str] = &[
    "otter", "badger", "falcon", "heron", "lynx", "marmot", "newt", "owl", "panda", "quail",
    "raven", "seal", "stoat", "swan", "tiger", "walrus", "weasel", "wombat", "yak", "zebra",
    "acorn", "anchor", "beacon", "birch", "bridge", "canyon", "cedar", "comet", "coral", "creek",
    "delta", "ember", "fjord", "forest", "glacier", "harbor", "island", "juniper", "kettle",
    "lagoon", "lantern", "maple", "meadow", "meteor", "orchid", "pebble", "planet", "prairie",
    "river", "summit", "thicket", "tundra", "valley", "willow", "badge", "bell", "compass", "drum",
    "flute", "garden", "kite", "mitten", "quilt", "ribbon",
];
const VERBS: &[&str] = &[
    "jumps",
    "hums",
    "waves",
    "dances",
    "drifts",
    "glides",
    "grins",
    "hops",
    "juggles",
    "laughs",
    "leaps",
    "lingers",
    "naps",
    "paddles",
    "pounces",
    "races",
    "rambles",
    "roams",
    "sails",
    "sings",
    "skips",
    "slides",
    "soars",
    "spins",
    "sprints",
    "stretches",
    "strolls",
    "swims",
    "swings",
    "tumbles",
    "twirls",
    "wanders",
    "whistles",
    "wiggles",
    "yawns",
    "zooms",
    "bakes",
    "blooms",
    "builds",
    "climbs",
    "cooks",
    "dives",
    "dreams",
    "explores",
    "fishes",
    "floats",
    "flies",
    "gathers",
    "grows",
    "hikes",
    "listens",
    "paints",
    "plays",
    "plants",
    "reads",
    "rests",
    "rides",
    "ripples",
    "shines",
    "sparkles",
    "stirs",
    "travels",
    "watches",
];

/// Three random words that read as a sentence: `brave-otter-jumps`. They
/// have nothing to do with the task's prompt, so a branch name never leaks
/// it, and a prompt in any language gets a name that is valid.
pub fn random_name() -> String {
    use std::hash::{BuildHasher, Hasher};
    // The standard library seeds each `RandomState` from the system.
    let pick = |words: &[&'static str]| {
        let n = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        words[(n % words.len() as u64) as usize]
    };
    format!("{}-{}-{}", pick(ADJECTIVES), pick(NOUNS), pick(VERBS))
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
    let pending = t.pending.then_some("pending\ttrue\n");
    fields
        .iter()
        .map(|(k, v)| format!("{k}\t{}\n", escape(v)))
        .chain(t.from.iter().map(|f| format!("from\t{}\n", escape(f))))
        .chain(
            optional
                .iter()
                .filter_map(|(k, v)| v.map(|n| format!("{k}\t{n}\n"))),
        )
        .chain(pending.map(String::from))
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
        from: get("from"),
        branch: get("branch")?,
        worktree: PathBuf::from(get("worktree")?),
        created: get("created")?.parse().unwrap_or(0),
        state,
        issue: get("issue").and_then(|n| n.parse().ok()),
        pull: get("pull").and_then(|n| n.parse().ok()),
        pending: get("pending").is_some(),
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
    fn names_are_three_valid_words_and_vary() {
        let names: std::collections::HashSet<String> = (0..50).map(|_| random_name()).collect();
        assert!(names.len() > 40, "{names:?}");
        for n in &names {
            assert_eq!(n.split('-').count(), 3, "{n}");
            assert!(n.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{n}");
        }
        for list in [ADJECTIVES, NOUNS, VERBS] {
            let unique: std::collections::HashSet<_> = list.iter().collect();
            assert_eq!(unique.len(), list.len(), "a word is listed twice");
        }
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
            from: Some("origin/main".into()),
            branch: "agent/x".into(),
            worktree: PathBuf::from("/tmp/r.spor/x"),
            created: 42,
            state: TaskState::Landed("abc".into()),
            issue: Some(7),
            pull: Some(12),
            pending: true,
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
            from: None,
            branch: "agent/x".into(),
            worktree: PathBuf::from("/tmp/my repo.spor/x"),
            created: 0,
            state: TaskState::Open,
            issue: None,
            pull: None,
            pending: false,
        };
        assert_eq!(
            task.agent_command(),
            r"cd '/tmp/my repo.spor/x' && claude 'Don'\''t break it'"
        );
    }
}
