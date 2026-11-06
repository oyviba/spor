use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub parents: Vec<String>,
    pub refs: Vec<String>,        // branch names / tags pointing here
    pub head_ref: Option<String>, // which branch HEAD is on, if any
    pub subject: String,
    pub author: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Staged,        // staged add / modify / rename / copy
    StagedDeleted, // deletion recorded in the index
    Modified,      // unstaged modification
    Deleted,       // deleted in the working tree, not staged
    Untracked,
    /// Both sides changed it and git couldn't combine them.
    Conflicted,
}

impl FileStatus {
    /// Does this entry live in the index (vs the working tree)?
    pub fn is_staged(&self) -> bool {
        matches!(self, FileStatus::Staged | FileStatus::StagedDeleted)
    }
}

#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub status: FileStatus,
    pub path: String,
    /// For staged renames/copies: the path the file came from. Unstaging a
    /// rename has to touch both paths, so we keep it around.
    pub orig_path: Option<String>,
}

/// Where the current branch sits relative to its upstream.
/// `None` values mean git didn't report that field — either detached HEAD,
/// no upstream configured, or brand-new unborn branch.
#[derive(Debug, Clone, Default)]
pub struct TrackingInfo {
    pub branch: Option<String>,   // current branch name, or None if detached
    pub upstream: Option<String>, // e.g. "origin/main"
    pub ahead: usize,             // commits we have that upstream doesn't
    pub behind: usize,            // commits upstream has that we don't
    pub detached: bool,
}

/// Full message and authorship of one commit, for a details view.
#[derive(Debug, Clone, Default)]
pub struct CommitDetails {
    pub author: String,
    pub email: String,
    pub author_time: i64,
    pub committer: String,
    pub commit_time: i64,
    /// Author date in local time, formatted by git, e.g. "Oct 01, 2026 at 14:02".
    pub date: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub name: String, // display name, e.g. "main" or "origin/feat/x"
    pub is_current: bool,
    pub is_remote: bool,
}

/// A repository on disk. Every command runs in its root, never in the
/// process's working directory, so a background job keeps working on the
/// repository it started in while another one is opened. Cheap to clone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repo {
    root: PathBuf,
}

/// What a command that can stop half-way left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Stopped on conflicts in these files; resolve them, then continue or
    /// abort with [`Repo::continue_operation`] / [`Repo::abort_operation`].
    Conflicts(Vec<String>),
}

/// A multi-step operation git is in the middle of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Operation::Merge => "Merge",
            Operation::Rebase => "Rebase",
            Operation::CherryPick => "Cherry-pick",
            Operation::Revert => "Revert",
        }
    }
}

/// Which side of a conflict to keep for a whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictSide {
    /// The checked-out branch.
    Ours,
    /// The branch or commit being brought in.
    Theirs,
}

/// How far `git reset` goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    /// Move the branch only; the changes stay staged.
    Soft,
    /// Move the branch and the index; the changes stay in the files.
    Mixed,
    /// Move everything, throwing uncommitted changes away.
    Hard,
    /// Move the branch, keeping uncommitted changes (refuses if they'd clash).
    Keep,
}

/// One entry of an interactive rebase plan, oldest commit first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseStep {
    Pick(String),
    /// Pick, then replace its message.
    Reword(String, String),
    /// Fold into the commit before, keeping both messages.
    Squash(String),
    /// Fold into the commit before, keeping only its message.
    Fixup(String),
    Drop(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    pub index: usize,
    pub sha: String,
    /// "WIP on main: …" or "On main: <message>".
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// The commit it's on (empty for a bare repository).
    pub head: String,
    /// The branch it has checked out, `None` when detached.
    pub branch: Option<String>,
    /// The repository's main working tree, as opposed to a linked one.
    pub is_main: bool,
    pub locked: bool,
    /// Its directory is gone; `git worktree prune` would remove it.
    pub prunable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub hash: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
    /// One-based line number in the file as it is now.
    pub line: usize,
    pub text: String,
}

/// Which diff a line selection was made in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// Index → working tree: what `git diff` shows.
    Unstaged,
    /// HEAD → index: what `git diff --cached` shows.
    Staged,
}

impl Repo {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The repository containing `dir`, at its top level.
    pub fn discover(dir: &Path) -> Result<Self, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|e| format!("failed to run git: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        Ok(Self::new(String::from_utf8_lossy(&out.stdout).trim()))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A path inside the working tree, for reading and writing files.
    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Reads never take the index lock to refresh stat info, so they can't
    /// make a pull or commit running alongside fail with "index.lock: File
    /// exists". Writes still take every lock they need. No command may stop
    /// to ask for a message: there's nobody at a terminal to answer.
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.root)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_EDITOR", "true")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Result<Output, String> {
        self.command(args)
            .output()
            .map_err(|e| format!("failed to run git: {e}"))
    }

    fn run_ok(&self, args: &[&str]) -> Result<String, String> {
        let out = self.run(args)?;
        if !out.status.success() {
            return Err(error_text(&out));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn succeeds(&self, args: &[&str]) -> bool {
        self.run(args).map(|o| o.status.success()).unwrap_or(false)
    }

    /// Run with `input` on stdin, for `git apply -`.
    fn run_with_input(&self, args: &[&str], input: &str) -> Result<(), String> {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to run git: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .map_err(|e| format!("failed to write to git: {e}"))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("failed to run git: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(error_text(&out))
        }
    }

    /// A file under `.git` (or a linked worktree's own git directory).
    fn git_path(&self, name: &str) -> Option<PathBuf> {
        let out = self.run_ok(&["rev-parse", "--git-path", name]).ok()?;
        Some(self.root.join(out.trim()))
    }

    // ── Reading ──────────────────────────────────────────────────────────

    /// The git directory every worktree of this repository shares, for
    /// state that belongs to the repository rather than one checkout.
    pub fn common_dir(&self) -> Result<PathBuf, String> {
        let out = self.run_ok(&["rev-parse", "--git-common-dir"])?;
        Ok(self.root.join(out.trim()))
    }

    /// How many commits `range` (e.g. `main..feature`) holds.
    pub fn count_commits(&self, range: &str) -> usize {
        self.run_ok(&["rev-list", "--count", range])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    /// The commit two revisions last had in common.
    pub fn merge_base(&self, a: &str, b: &str) -> Result<String, String> {
        self.run_ok(&["merge-base", a, b])
            .map(|s| s.trim().to_string())
    }

    /// Files, added and deleted lines between `base` and the working tree
    /// (committed and uncommitted changes to tracked files).
    pub fn numstat_since(&self, base: &str) -> (usize, usize, usize) {
        let out = self
            .run_ok(&["diff", "--no-ext-diff", "--numstat", base, "--"])
            .unwrap_or_default();
        out.lines().fold((0, 0, 0), |(f, a, d), line| {
            let mut it = line.split('\t');
            let n = |s: Option<&str>| s.and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
            (f + 1, a + n(it.next()), d + n(it.next()))
        })
    }

    /// Everything between `base` and the working tree as one patch,
    /// untracked files included.
    pub fn patch_since(&self, base: &str) -> Result<String, String> {
        let mut args = vec!["diff"];
        args.extend(PATCH_FLAGS);
        args.extend([base, "--"]);
        let mut patch = self.run_ok(&args)?;
        let untracked: Vec<String> = self
            .status()?
            .into_iter()
            .filter(|e| e.status == FileStatus::Untracked)
            .map(|e| e.path)
            .collect();
        patch.push_str(&self.untracked_patch(&untracked));
        Ok(patch)
    }

    /// When the last commit on `rev` was made.
    pub fn commit_time(&self, rev: &str) -> i64 {
        self.run_ok(&["log", "-1", "--format=%ct", rev])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Stage everything in the working tree, new files too.
    pub fn stage_all(&self) -> Result<(), String> {
        self.run_ok(&["add", "--all"]).map(|_| ())
    }

    /// Bring `branch`'s changes into the index as one change, without
    /// committing. Conflicts are left in the files.
    pub fn squash_merge(&self, branch: &str) -> Result<Outcome, String> {
        let out = self.run(&["merge", "--squash", branch])?;
        if out.status.success() {
            return Ok(Outcome::Done);
        }
        // A squash merge records no MERGE_HEAD, so look for conflicts directly.
        let conflicts = self.conflicts()?;
        if conflicts.is_empty() {
            Err(error_text(&out))
        } else {
            Ok(Outcome::Conflicts(conflicts))
        }
    }

    /// Files that would conflict if `a` and `b` were merged, worked out
    /// without touching any working tree or the index. Empty when they
    /// merge cleanly.
    pub fn merge_conflicts(&self, a: &str, b: &str) -> Result<Vec<String>, String> {
        let out = self.run(&[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            a,
            b,
        ])?;
        match out.status.code() {
            Some(0) => Ok(Vec::new()),
            // The first line is the tree it would write; then one path per
            // conflicted file.
            Some(1) => Ok(String::from_utf8_lossy(&out.stdout)
                .lines()
                .skip(1)
                .take_while(|l| !l.is_empty())
                .map(str::to_string)
                .collect()),
            _ => Err(error_text(&out)),
        }
    }

    /// The tree merging `a` and `b` would give, without touching anything;
    /// None when the merge would conflict.
    pub fn merged_tree(&self, a: &str, b: &str) -> Option<String> {
        let out = self
            .run(&["merge-tree", "--write-tree", "--no-messages", a, b])
            .ok()?;
        if out.status.code() != Some(0) {
            return None;
        }
        let tree = String::from_utf8_lossy(&out.stdout);
        tree.lines().next().map(str::to_string)
    }

    /// The oldest commit `branch`'s reflog remembers: for a branch created
    /// here, what it was created from. None without a reflog.
    pub fn first_logged(&self, branch: &str) -> Option<String> {
        let reference = format!("refs/heads/{branch}");
        let out = self
            .run_ok(&["log", "-g", "--format=%H", &reference, "--"])
            .ok()?;
        out.lines().last().map(str::to_string)
    }

    /// The commits `range` (e.g. `main..feature`) holds.
    pub fn commits_in(&self, range: &str) -> Vec<String> {
        self.run_ok(&["rev-list", range, "--"])
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// The files that differ between two revisions (or trees).
    pub fn changed_paths(&self, from: &str, to: &str) -> Vec<String> {
        self.run_ok(&[
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-ext-diff",
            "--name-only",
            from,
            to,
            "--",
        ])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
    }

    /// Lines of `from` that are gone in `to`, per file in `paths`: each
    /// file's removed runs as (first line, count), one-based in `from`.
    pub fn removed_lines(
        &self,
        from: &str,
        to: &str,
        paths: &[String],
    ) -> Vec<(String, Vec<(usize, usize)>)> {
        if paths.is_empty() {
            return Vec::new();
        }
        let mut args = vec![
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--no-prefix",
            "-U0",
            from,
            to,
            "--",
        ];
        args.extend(paths.iter().map(String::as_str));
        let Ok(out) = self.run_ok(&args) else {
            return Vec::new();
        };
        parse_removed(&out)
    }

    /// Every line `to` has that `from` doesn't, anywhere, trimmed: to tell
    /// code that moved from code that went.
    pub fn added_lines(&self, from: &str, to: &str) -> std::collections::HashSet<String> {
        let out = self
            .run_ok(&[
                "diff",
                "--no-ext-diff",
                "--no-renames",
                "-U0",
                from,
                to,
                "--",
            ])
            .unwrap_or_default();
        out.lines()
            .filter(|l| !l.starts_with("+++ "))
            .filter_map(|l| l.strip_prefix('+'))
            .map(|l| l.trim().to_string())
            .collect()
    }

    /// Who wrote `path`'s lines in `runs` (first line, count) as of `rev`.
    pub fn blame_runs(&self, rev: &str, path: &str, runs: &[(usize, usize)]) -> Vec<BlameLine> {
        let ranges: Vec<String> = runs
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(start, n)| format!("-L{start},+{n}"))
            .collect();
        if ranges.is_empty() {
            return Vec::new();
        }
        let mut args = vec!["blame", "--porcelain"];
        args.extend(ranges.iter().map(String::as_str));
        args.extend([rev, "--", path]);
        self.run_ok(&args)
            .map(|out| parse_blame(&out))
            .unwrap_or_default()
    }

    /// Merge `branch` with a merge commit even when it could fast-forward.
    pub fn merge_no_ff(&self, branch: &str, message: &str) -> Result<Outcome, String> {
        let out = self.run(&["merge", "--no-ff", "-m", message, branch])?;
        self.outcome(out)
    }

    /// Load commit graph. We use ASCII unit separators to survive weird subjects.
    pub fn log_all(&self, limit: usize) -> Result<Vec<Commit>, String> {
        let n = format!("-n{limit}");
        let pretty = format!("--pretty=format:{LOG_FORMAT}");
        // Not `--all`: that would pull in refs/stash, whose internal "WIP on" /
        // "index on" commits would show up as phantom branches.
        let out = self.run_ok(&[
            "log",
            "--branches",
            "--remotes",
            "--tags",
            "HEAD",
            "--date-order",
            &n,
            &pretty,
        ])?;
        Ok(parse_log(&out))
    }

    /// Commits that touched `path`, newest first, following renames.
    pub fn file_log(&self, path: &str, limit: usize) -> Result<Vec<Commit>, String> {
        let n = format!("-n{limit}");
        let pretty = format!("--pretty=format:{LOG_FORMAT}");
        let out = self.run_ok(&["log", "--follow", &n, &pretty, "--", path])?;
        Ok(parse_log(&out))
    }

    /// Commits whose message, author or hash matches `query` (ignoring
    /// case), newest first, from the same refs History shows.
    pub fn search_commits(&self, query: &str, limit: usize) -> Result<Vec<String>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let n = format!("-n{limit}");
        let mut hashes: Vec<String> = Vec::new();
        let mut add = |out: String| {
            for h in out.lines().map(str::trim).filter(|h| !h.is_empty()) {
                if !hashes.iter().any(|x| x == h) {
                    hashes.push(h.to_string());
                }
            }
        };
        let refs = ["--branches", "--remotes", "--tags", "HEAD"];
        for field in ["--grep", "--author"] {
            let pattern = format!("{field}={query}");
            let mut args = vec!["log", "--format=%H", "-i", "--fixed-strings", &n, &pattern];
            args.extend(refs);
            add(self.run_ok(&args)?);
        }
        if query.len() >= 4 && query.chars().all(|c| c.is_ascii_hexdigit()) {
            let spec = format!("{query}^{{commit}}");
            if let Ok(h) = self.run_ok(&["rev-parse", "--verify", "--quiet", &spec]) {
                hashes.insert(0, h.trim().to_string());
                hashes.dedup();
            }
        }
        hashes.truncate(limit);
        Ok(hashes)
    }

    /// Who last changed each line of `path`, as it is in the working tree.
    pub fn blame(&self, path: &str) -> Result<Vec<BlameLine>, String> {
        self.blame_at(path, None)
    }

    /// Who last changed each line of `path` as of `rev` (the working tree,
    /// uncommitted lines included, when `None`). Uncommitted lines have an
    /// all-zero hash.
    pub fn blame_at(&self, path: &str, rev: Option<&str>) -> Result<Vec<BlameLine>, String> {
        let out = match rev {
            Some(rev) => self.run_ok(&["blame", "--porcelain", rev, "--", path])?,
            None => self.run_ok(&["blame", "--porcelain", "--", path])?,
        };
        Ok(parse_blame(&out))
    }

    /// Working tree status. `-z` gives NUL-separated records with no
    /// quoting, so paths with spaces, quotes, or non-ASCII survive, and
    /// rename origins arrive as a separate field instead of `old -> new`.
    pub fn status(&self) -> Result<Vec<StatusEntry>, String> {
        let out = self.run_ok(&["status", "--porcelain=v1", "-z", "-uall"])?;
        Ok(parse_status(&out))
    }

    pub fn tracking(&self) -> Result<TrackingInfo, String> {
        let out = self.run_ok(&["status", "--porcelain=v2", "--branch"])?;
        Ok(parse_tracking(&out))
    }

    pub fn commit_details(&self, hash: &str) -> Result<CommitDetails, String> {
        let out = self.run_ok(&[
            "show",
            "-s",
            "--date=format-local:%b %d, %Y at %H:%M",
            "--format=%an%x1f%ae%x1f%at%x1f%cn%x1f%ct%x1f%ad%x1f%B",
            hash,
        ])?;
        Ok(parse_commit_details(&out))
    }

    /// The last commit's full message, to start an amend from.
    pub fn head_message(&self) -> Result<String, String> {
        self.run_ok(&["log", "-1", "--format=%B"])
            .map(|s| s.trim_end().to_string())
    }

    pub fn head_sha(&self) -> Result<String, String> {
        self.run_ok(&["rev-parse", "HEAD"])
            .map(|s| s.trim().to_string())
    }

    /// What `rev` names, as a full hash.
    pub fn resolve(&self, rev: &str) -> Result<String, String> {
        let spec = format!("{rev}^{{commit}}");
        self.run_ok(&["rev-parse", "--verify", "--quiet", &spec])
            .map(|s| s.trim().to_string())
            .map_err(|_| format!("{rev} isn't a commit"))
    }

    pub fn parents(&self, sha: &str) -> Result<Vec<String>, String> {
        let out = self.run_ok(&["rev-list", "--parents", "-n1", sha])?;
        Ok(out.split_whitespace().skip(1).map(str::to_string).collect())
    }

    /// Is `ancestor` part of `descendant`'s history?
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        self.succeeds(&["merge-base", "--is-ancestor", ancestor, descendant])
    }

    /// Are there merge commits on HEAD after `base`? Rewriting that range
    /// would flatten them.
    pub fn has_merges_since(&self, base: &str) -> bool {
        let range = format!("{base}..HEAD");
        self.run_ok(&["rev-list", "--merges", "-n1", &range])
            .is_ok_and(|out| !out.trim().is_empty())
    }

    /// Commits on HEAD that `base` doesn't have, oldest first — what a
    /// rebase onto `base` would replay.
    pub fn commits_since(&self, base: &str) -> Result<Vec<Commit>, String> {
        let range = format!("{base}..HEAD");
        let pretty = format!("--pretty=format:{LOG_FORMAT}");
        let out = self.run_ok(&["log", "--reverse", "--no-merges", &pretty, &range])?;
        Ok(parse_log(&out))
    }

    /// Commits on `branch` that `base` doesn't have, newest first: a
    /// branch's own work.
    pub fn branch_commits(&self, base: &str, branch: &str) -> Result<Vec<Commit>, String> {
        let range = format!("{base}..{branch}");
        let pretty = format!("--pretty=format:{LOG_FORMAT}");
        let out = self.run_ok(&["log", "-n200", &pretty, &range])?;
        Ok(parse_log(&out))
    }

    /// Files, added and deleted lines on `branch` since it left `base`.
    pub fn branch_numstat(&self, base: &str, branch: &str) -> (usize, usize, usize) {
        let range = format!("{base}...{branch}");
        let out = self
            .run_ok(&["diff", "--no-ext-diff", "--numstat", &range, "--"])
            .unwrap_or_default();
        out.lines().fold((0, 0, 0), |(f, a, d), line| {
            let mut it = line.split('\t');
            let n = |s: Option<&str>| s.and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
            (f + 1, a + n(it.next()), d + n(it.next()))
        })
    }

    /// `branch`'s changes since it left `base`, as one patch.
    pub fn branch_patch(&self, base: &str, branch: &str) -> Result<String, String> {
        let range = format!("{base}...{branch}");
        let mut args = vec!["diff"];
        args.extend(PATCH_FLAGS);
        args.extend([range.as_str(), "--"]);
        self.run_ok(&args)
    }

    /// Diff for one working-tree entry. Untracked files have nothing in the
    /// index to compare against, so they're shown as entirely new content.
    pub fn diff_entry(&self, entry: &StatusEntry) -> Result<String, String> {
        if entry.status != FileStatus::Untracked {
            let staged = if entry.status.is_staged() {
                "--cached"
            } else {
                "--no-color"
            };
            return self.run_ok(&["diff", staged, "--", &entry.path]);
        }
        let out = self.run(&["diff", "--no-index", "--", "/dev/null", &entry.path])?;
        // --no-index exits 1 when the files differ — which, here, they always do.
        match out.status.code() {
            Some(0) | Some(1) => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
            _ => Err(error_text(&out)),
        }
    }

    /// The patch a commit introduced, without the commit header. Merges are
    /// diffed against their first parent — what the merge brought into the
    /// branch — rather than git's combined-diff format.
    pub fn commit_patch(&self, hash: &str, parents: &[String]) -> Result<String, String> {
        let mut args: Vec<&str> = Vec::new();
        match parents.first() {
            Some(first) => {
                args.push("diff");
                args.extend(PATCH_FLAGS);
                args.extend([first.as_str(), hash, "--"]);
            }
            None => {
                args.extend(["show", "--format="]);
                args.extend(PATCH_FLAGS);
                args.extend([hash, "--"]);
            }
        }
        self.run_ok(&args)
    }

    /// One side of the uncommitted changes: index → working tree (with
    /// untracked files as new), or HEAD → index.
    /// Files with conflicts are left out: git shows them in its combined
    /// format, which isn't a patch that can be applied.
    pub fn patch(&self, kind: DiffKind, untracked: &[String]) -> Result<String, String> {
        let mut args = vec!["diff"];
        args.extend(PATCH_FLAGS);
        match kind {
            DiffKind::Unstaged => {
                let excludes: Vec<String> = self
                    .conflicts()
                    .unwrap_or_default()
                    .iter()
                    .map(|p| format!(":(exclude,literal){p}"))
                    .collect();
                args.push("--");
                if !excludes.is_empty() {
                    args.push(".");
                    args.extend(excludes.iter().map(String::as_str));
                }
                let mut patch = self.run_ok(&args)?;
                patch.push_str(&self.untracked_patch(untracked));
                Ok(patch)
            }
            DiffKind::Staged => {
                args.extend(["--cached", "--"]);
                self.run_ok(&args)
            }
        }
    }

    fn untracked_patch(&self, untracked: &[String]) -> String {
        let mut patch = String::new();
        for path in untracked {
            let mut args = vec!["diff", "--no-index"];
            args.extend(PATCH_FLAGS);
            args.extend(["--", "/dev/null", path.as_str()]);
            if let Ok(out) = self.run(&args) {
                // --no-index exits 1 when the files differ — which, here, they do.
                if matches!(out.status.code(), Some(0) | Some(1)) {
                    patch.push_str(&String::from_utf8_lossy(&out.stdout));
                }
            }
        }
        patch
    }

    fn has_head(&self) -> bool {
        self.succeeds(&["rev-parse", "--verify", "--quiet", "HEAD"])
    }

    /// Find the repo's "main" branch name. Checks common names.
    pub fn main_branch(&self) -> Option<String> {
        ["main", "master", "trunk"]
            .into_iter()
            .find(|c| self.succeeds(&["rev-parse", "--verify", "--quiet", c]))
            .map(str::to_string)
    }

    /// Commits on the main branch's first-parent chain — these get pinned to lane 0.
    pub fn main_chain(&self) -> Result<Vec<String>, String> {
        let Some(main) = self.main_branch() else {
            return Ok(Vec::new());
        };
        let out = self.run_ok(&["log", "--first-parent", "--format=%H", &main])?;
        Ok(out.lines().map(|s| s.to_string()).collect())
    }

    /// What does the remote consider its default branch? Reads
    /// `refs/remotes/origin/HEAD`, which git maintains as a symbolic ref.
    /// Falls back to local main/master/trunk detection.
    pub fn default_base_branch(&self) -> Option<String> {
        if let Ok(out) = self.run_ok(&["symbolic-ref", "refs/remotes/origin/HEAD"]) {
            if let Some(name) = out.trim().strip_prefix("refs/remotes/origin/") {
                return Some(name.to_string());
            }
        }
        self.main_branch()
    }

    /// Where new work starts by default: origin's copy of the default
    /// branch (`origin/main`), which a fetch keeps current, rather than a
    /// local one that may be behind. The local branch if origin has none.
    pub fn default_start(&self) -> Option<String> {
        let base = self.default_base_branch()?;
        let remote = format!("origin/{base}");
        let exists = self.succeeds(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote}"),
        ]);
        Some(if exists { remote } else { base })
    }

    /// For a remote-tracking branch (`origin/main`), its name on the remote
    /// (`main`); None for anything else.
    pub fn remote_branch(&self, name: &str) -> Option<String> {
        let (remote, branch) = name.split_once('/')?;
        let tracked = self.succeeds(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{name}"),
        ]);
        (tracked && !branch.is_empty() && self.remotes().iter().any(|r| r == remote))
            .then(|| branch.to_string())
    }

    /// List all branches, local and remote-tracking, excluding `origin/HEAD` stubs.
    pub fn list_branches(&self) -> Result<Vec<Branch>, String> {
        let out = self.run_ok(&[
            "for-each-ref",
            "--format=%(HEAD)%09%(refname)",
            "refs/heads",
            "refs/remotes",
        ])?;
        Ok(parse_branches(&out))
    }

    /// Names of configured remotes, e.g. ["origin", "upstream"].
    pub fn remotes(&self) -> Vec<String> {
        self.run_ok(&["remote"])
            .map(|out| out.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    // ── Staging ──────────────────────────────────────────────────────────

    pub fn stage(&self, path: &str) -> Result<(), String> {
        self.run_ok(&["add", "--", path]).map(|_| ())
    }

    /// Unstage an entry. A staged rename is two index operations (delete at
    /// the old path, add at the new), so undoing it has to restore both.
    pub fn unstage(&self, entry: &StatusEntry) -> Result<(), String> {
        let mut args = vec!["restore", "--staged", "--", entry.path.as_str()];
        if let Some(orig) = &entry.orig_path {
            args.push(orig.as_str());
        }
        if !self.has_head() {
            // Nothing to restore from on an unborn branch: drop from the index.
            args = vec!["rm", "--cached", "--quiet", "--", entry.path.as_str()];
        }
        self.run_ok(&args).map(|_| ())
    }

    /// Stage, unstage or discard the changed lines between `from` and `to`
    /// of `path`, as numbered in the `kind` diff: staging and discarding
    /// work on the unstaged diff, unstaging on the staged one. Returns the
    /// patch that was applied, so it can be undone with [`Repo::apply`].
    pub fn apply_lines(
        &self,
        path: &str,
        kind: DiffKind,
        action: LineAction,
        from: crate::patch::LinePos,
        to: crate::patch::LinePos,
    ) -> Result<AppliedPatch, String> {
        let expected = match action {
            LineAction::Stage | LineAction::Discard => DiffKind::Unstaged,
            LineAction::Unstage => DiffKind::Staged,
        };
        if kind != expected {
            return Err(match action {
                LineAction::Unstage => "Only staged lines can be unstaged".into(),
                _ => "Only unstaged lines can be staged or discarded".into(),
            });
        }
        // An untracked file has to be in the index (empty, "intent to add")
        // before part of it can be staged.
        let untracked = self
            .status()?
            .iter()
            .any(|e| e.path == path && e.status == FileStatus::Untracked);
        if untracked {
            if action == LineAction::Discard {
                return Err("Discard the whole file to remove an untracked file".into());
            }
            self.run_ok(&["add", "--intent-to-add", "--", path])?;
        }
        let mut args = vec!["diff"];
        args.extend(PATCH_FLAGS);
        if kind == DiffKind::Staged {
            args.push("--cached");
        }
        args.extend(["--", path]);
        let text = self.run_ok(&args)?;
        let file = crate::patch::parse(&text)
            .into_iter()
            .next()
            .ok_or_else(|| format!("{path} has no changes there"))?;
        let reverse = action != LineAction::Stage;
        let patch = file
            .select(from, to, reverse)
            .ok_or_else(|| "No changed lines are selected".to_string())?;
        let applied = AppliedPatch {
            patch,
            cached: action != LineAction::Discard,
            reverse,
        };
        self.apply(&applied)?;
        Ok(applied)
    }

    /// Apply a patch to the index (`cached`) or the working tree.
    pub fn apply(&self, p: &AppliedPatch) -> Result<(), String> {
        let mut args = vec!["apply", "--recount", "--whitespace=nowarn"];
        if p.cached {
            args.push("--cached");
        }
        if p.reverse {
            args.push("--reverse");
        }
        args.push("-");
        self.run_with_input(&args, &p.patch)
    }

    /// Take back an [`apply`](Repo::apply).
    pub fn unapply(&self, p: &AppliedPatch) -> Result<(), String> {
        self.apply(&AppliedPatch {
            reverse: !p.reverse,
            ..p.clone()
        })
    }

    /// Throw away unstaged changes to a tracked file — reverts
    /// modifications, brings back deletions. Does not touch the index.
    pub fn discard_worktree(&self, path: &str) -> Result<(), String> {
        self.run_ok(&["restore", "--", path]).map(|_| ())
    }

    /// Delete an untracked file from the working tree.
    pub fn remove_untracked(&self, path: &str) -> Result<(), String> {
        self.run_ok(&["clean", "-f", "--", path]).map(|_| ())
    }

    // ── Committing ───────────────────────────────────────────────────────

    pub fn commit(&self, message: &str) -> Result<(), String> {
        self.run_ok(&["commit", "-m", message]).map(|_| ())
    }

    /// Replace the last commit with one that also holds what's staged, and
    /// `message` (or the old message when `None`).
    pub fn amend(&self, message: Option<&str>) -> Result<(), String> {
        match message {
            Some(m) => self.run_ok(&["commit", "--amend", "-m", m]),
            None => self.run_ok(&["commit", "--amend", "--no-edit"]),
        }
        .map(|_| ())
    }

    pub fn reset(&self, sha: &str, mode: ResetMode) -> Result<(), String> {
        let flag = match mode {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
            ResetMode::Keep => "--keep",
        };
        self.run_ok(&["reset", "--quiet", flag, sha]).map(|_| ())
    }

    // ── Branches ─────────────────────────────────────────────────────────

    /// Attempt `git switch`. For a remote-tracking ref like `origin/feat/x`,
    /// git auto-creates a local tracking branch `feat/x` if none exists.
    pub fn checkout_branch(&self, name: &str) -> Result<(), String> {
        // `git switch origin/feat/x` is an error; `git switch feat/x` is
        // what we want, and git will set up tracking automatically.
        let target = match name.split_once('/') {
            Some((remote, rest)) if self.remotes().iter().any(|r| r == remote) => rest,
            _ => name,
        };
        self.run_ok(&["switch", target]).map(|_| ())
    }

    /// Check out a commit without a branch.
    pub fn checkout_detached(&self, sha: &str) -> Result<(), String> {
        self.run_ok(&["switch", "--detach", sha]).map(|_| ())
    }

    pub fn create_branch_at(&self, name: &str, sha: &str) -> Result<(), String> {
        self.run_ok(&["switch", "-c", name, sha]).map(|_| ())
    }

    /// Delete a local branch and return the commit it was on, so it can be
    /// recreated. Without `force`, git refuses a branch that isn't merged.
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<String, String> {
        let r = format!("refs/heads/{name}");
        let sha = self
            .run_ok(&["rev-parse", "--verify", "--quiet", &r])
            .map(|s| s.trim().to_string())
            .map_err(|_| format!("there's no branch {name}"))?;
        let flag = if force { "-D" } else { "-d" };
        self.run_ok(&["branch", flag, "--", name])?;
        Ok(sha)
    }

    /// Recreate a deleted branch at `sha`.
    pub fn restore_branch(&self, name: &str, sha: &str) -> Result<(), String> {
        self.run_ok(&["branch", "--", name, sha]).map(|_| ())
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<(), String> {
        self.run_ok(&["branch", "-m", "--", old, new]).map(|_| ())
    }

    /// Is `name` a usable branch name?
    pub fn valid_branch_name(&self, name: &str) -> bool {
        !name.is_empty() && self.succeeds(&["check-ref-format", "--branch", name])
    }

    // ── Merging, rebasing, picking ───────────────────────────────────────

    /// Merge `source` into the checked-out branch (fast-forward when possible).
    pub fn merge(&self, source: &str) -> Result<Outcome, String> {
        let out = self.run(&["merge", "--no-edit", source])?;
        self.outcome(out)
    }

    /// Apply the change `sha` introduced on top of HEAD. A merge commit is
    /// taken relative to its first parent.
    pub fn cherry_pick(&self, sha: &str) -> Result<Outcome, String> {
        let mut args = vec!["cherry-pick"];
        if self.parents(sha)?.len() > 1 {
            args.extend(["-m", "1"]);
        }
        args.push(sha);
        let out = self.run(&args)?;
        self.outcome(out)
    }

    /// Commit the inverse of `sha` on top of HEAD.
    pub fn revert(&self, sha: &str) -> Result<Outcome, String> {
        let mut args = vec!["revert", "--no-edit"];
        if self.parents(sha)?.len() > 1 {
            args.extend(["-m", "1"]);
        }
        args.push(sha);
        let out = self.run(&args)?;
        self.outcome(out)
    }

    /// Replay the current branch's commits on top of `onto`.
    pub fn rebase(&self, onto: &str) -> Result<Outcome, String> {
        let out = self.run(&["rebase", onto])?;
        self.outcome(out)
    }

    /// Rewrite the commits after `base` following `plan`, oldest first:
    /// reorder, reword, squash, fix up or drop them.
    pub fn rebase_interactive(&self, base: &str, plan: &[RebaseStep]) -> Result<Outcome, String> {
        let dir = self
            .git_path("spor-rebase")
            .ok_or_else(|| "couldn't find the git directory".to_string())?;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut todo = String::new();
        for (i, step) in plan.iter().enumerate() {
            match step {
                RebaseStep::Pick(sha) => todo.push_str(&format!("pick {sha}\n")),
                RebaseStep::Squash(sha) => todo.push_str(&format!("squash {sha}\n")),
                RebaseStep::Fixup(sha) => todo.push_str(&format!("fixup {sha}\n")),
                RebaseStep::Drop(sha) => todo.push_str(&format!("drop {sha}\n")),
                RebaseStep::Reword(sha, message) => {
                    let file = dir.join(format!("message-{i}"));
                    std::fs::write(&file, message).map_err(|e| e.to_string())?;
                    todo.push_str(&format!(
                        "pick {sha}\nexec git commit --amend --no-verify --quiet -F {}\n",
                        shell_quote(&file.to_string_lossy())
                    ));
                }
            }
        }
        if todo.is_empty() {
            todo.push_str("noop\n");
        }
        let plan_file = dir.join("todo");
        std::fs::write(&plan_file, todo).map_err(|e| e.to_string())?;
        // git runs the sequence editor with the todo file's path appended.
        let editor = format!("cp {}", shell_quote(&plan_file.to_string_lossy()));
        let out = self
            .command(&["rebase", "--interactive", base])
            .env("GIT_SEQUENCE_EDITOR", editor)
            .output()
            .map_err(|e| format!("failed to run git: {e}"))?;
        self.outcome(out)
    }

    /// Which multi-step operation is stopped half-way, if any.
    pub fn operation(&self) -> Option<Operation> {
        let exists = |name: &str| self.git_path(name).is_some_and(|p| p.exists());
        if exists("rebase-merge") || exists("rebase-apply") {
            Some(Operation::Rebase)
        } else if exists("MERGE_HEAD") {
            Some(Operation::Merge)
        } else if exists("CHERRY_PICK_HEAD") {
            Some(Operation::CherryPick)
        } else if exists("REVERT_HEAD") {
            Some(Operation::Revert)
        } else {
            None
        }
    }

    /// Give up on the operation in progress, putting things back as they were.
    pub fn abort_operation(&self) -> Result<(), String> {
        let args: &[&str] = match self.operation() {
            Some(Operation::Merge) => &["merge", "--abort"],
            Some(Operation::Rebase) => &["rebase", "--abort"],
            Some(Operation::CherryPick) => &["cherry-pick", "--abort"],
            Some(Operation::Revert) => &["revert", "--abort"],
            None => return Ok(()),
        };
        self.run_ok(args).map(|_| ())
    }

    /// Carry on once conflicts are resolved and staged. A rebase can stop
    /// again on the next commit.
    pub fn continue_operation(&self) -> Result<Outcome, String> {
        let left = self.conflicts()?;
        if !left.is_empty() {
            return Ok(Outcome::Conflicts(left));
        }
        let args: &[&str] = match self.operation() {
            Some(Operation::Merge) => &["commit", "--no-edit"],
            Some(Operation::Rebase) => &["rebase", "--continue"],
            Some(Operation::CherryPick) => &["cherry-pick", "--continue"],
            Some(Operation::Revert) => &["revert", "--continue"],
            None => return Ok(Outcome::Done),
        };
        let out = self.run(args)?;
        self.outcome(out)
    }

    /// Leave out the commit a rebase or cherry-pick stopped on.
    pub fn skip_commit(&self) -> Result<Outcome, String> {
        let args: &[&str] = match self.operation() {
            Some(Operation::Rebase) => &["rebase", "--skip"],
            Some(Operation::CherryPick) => &["cherry-pick", "--skip"],
            Some(Operation::Revert) => &["revert", "--skip"],
            _ => return Err("there's no commit to skip".into()),
        };
        let out = self.run(args)?;
        self.outcome(out)
    }

    /// Files with unresolved conflicts.
    pub fn conflicts(&self) -> Result<Vec<String>, String> {
        let out = self.run_ok(&["diff", "--name-only", "--diff-filter=U", "-z"])?;
        let mut paths: Vec<String> = out
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        paths.dedup();
        Ok(paths)
    }

    /// Resolve a whole file by taking one side. If that side deleted the
    /// file, the file is deleted.
    pub fn resolve_with(&self, path: &str, side: ConflictSide) -> Result<(), String> {
        let flag = match side {
            ConflictSide::Ours => "--ours",
            ConflictSide::Theirs => "--theirs",
        };
        match self.run_ok(&["checkout", flag, "--", path]) {
            Ok(_) => self.stage(path),
            Err(e) if e.contains("does not have") => {
                self.run_ok(&["rm", "--quiet", "--", path]).map(|_| ())
            }
            Err(e) => Err(e),
        }
    }

    /// Mark a conflicted file resolved as it is in the working tree.
    pub fn mark_resolved(&self, path: &str) -> Result<(), String> {
        if self.path(path).exists() {
            self.stage(path)
        } else {
            self.run_ok(&["rm", "--quiet", "--", path]).map(|_| ())
        }
    }

    /// Turn a command's exit into an [`Outcome`]: success, a stop on
    /// conflicts, or an error.
    fn outcome(&self, out: Output) -> Result<Outcome, String> {
        if out.status.success() {
            return Ok(Outcome::Done);
        }
        if self.operation().is_some() {
            let conflicts = self.conflicts()?;
            if !conflicts.is_empty() {
                return Ok(Outcome::Conflicts(conflicts));
            }
        }
        Err(error_text(&out))
    }

    // ── Stashes ──────────────────────────────────────────────────────────

    pub fn stash_push(&self) -> Result<(), String> {
        self.stash_save(Some("spor-auto-stash"), true)
    }

    /// Stash uncommitted changes (and untracked files when asked), with a
    /// message to tell it apart later.
    pub fn stash_save(&self, message: Option<&str>, untracked: bool) -> Result<(), String> {
        let mut args = vec!["stash", "push"];
        if untracked {
            args.push("--include-untracked");
        }
        if let Some(m) = message.filter(|m| !m.trim().is_empty()) {
            args.extend(["-m", m]);
        }
        self.run_ok(&args).map(|_| ())
    }

    pub fn stash_pop(&self) -> Result<(), String> {
        self.run_ok(&["stash", "pop"]).map(|_| ())
    }

    /// Apply the latest stash with what was staged staged again, and drop
    /// it. Falls back to a plain pop when the index can't be restored.
    pub fn stash_pop_index(&self) -> Result<(), String> {
        self.run_ok(&["stash", "pop", "--index"])
            .or_else(|_| self.run_ok(&["stash", "pop"]))
            .map(|_| ())
    }

    /// Apply stash entry `index` and drop it.
    pub fn stash_pop_at(&self, index: usize) -> Result<(), String> {
        self.run_ok(&["stash", "pop", &stash_ref(index)])
            .map(|_| ())
    }

    /// Apply stash entry `index` and keep it.
    pub fn stash_apply_at(&self, index: usize) -> Result<(), String> {
        self.run_ok(&["stash", "apply", &stash_ref(index)])
            .map(|_| ())
    }

    /// Drop stash entry `index`, returning it so [`Repo::restore_stash`]
    /// can bring it back.
    pub fn stash_drop_at(&self, index: usize) -> Result<Stash, String> {
        let stash = self
            .stashes()
            .into_iter()
            .find(|s| s.index == index)
            .ok_or_else(|| format!("there's no stash {index}"))?;
        self.run_ok(&["stash", "drop", "--quiet", &stash_ref(index)])?;
        Ok(stash)
    }

    /// Put a dropped stash back on the stack (it becomes the newest).
    pub fn restore_stash(&self, stash: &Stash) -> Result<(), String> {
        self.run_ok(&["stash", "store", "-m", &stash.message, &stash.sha])
            .map(|_| ())
    }

    pub fn stashes(&self) -> Vec<Stash> {
        self.run_ok(&["stash", "list", "--format=%H%x1f%gs"])
            .map(|s| {
                s.lines()
                    .enumerate()
                    .filter_map(|(index, line)| {
                        let (sha, message) = line.split_once('\x1f')?;
                        Some(Stash {
                            index,
                            sha: sha.to_string(),
                            message: message.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    // ── Tags ─────────────────────────────────────────────────────────────

    /// Tag `sha`: annotated when there's a message, lightweight otherwise.
    pub fn create_tag(&self, name: &str, sha: &str, message: Option<&str>) -> Result<(), String> {
        match message.filter(|m| !m.trim().is_empty()) {
            Some(m) => self.run_ok(&["tag", "-a", "-m", m, "--", name, sha]),
            None => self.run_ok(&["tag", "--", name, sha]),
        }
        .map(|_| ())
    }

    /// What a tag points at — the tag object itself for an annotated tag —
    /// so a deleted tag can be put back exactly, message and all.
    pub fn tag_target(&self, name: &str) -> Result<String, String> {
        let r = format!("refs/tags/{name}");
        self.run_ok(&["rev-parse", "--verify", "--quiet", &r])
            .map(|s| s.trim().to_string())
    }

    pub fn delete_tag(&self, name: &str) -> Result<(), String> {
        self.run_ok(&["tag", "-d", name]).map(|_| ())
    }

    /// Recreate a deleted tag from [`Repo::tag_target`]. Refuses if a tag by
    /// that name exists again.
    pub fn restore_tag(&self, name: &str, target: &str) -> Result<(), String> {
        let r = format!("refs/tags/{name}");
        self.run_ok(&["update-ref", &r, target, ""]).map(|_| ())
    }

    // ── Worktrees ────────────────────────────────────────────────────────

    pub fn worktrees(&self) -> Result<Vec<Worktree>, String> {
        let out = self.run_ok(&["worktree", "list", "--porcelain"])?;
        Ok(parse_worktrees(&out))
    }

    /// Check out `branch` in a new working tree at `path`. With `base`, the
    /// branch is created there first.
    pub fn add_worktree(
        &self,
        path: &Path,
        branch: &str,
        base: Option<&str>,
    ) -> Result<Repo, String> {
        let p = path.to_string_lossy();
        let mut args = vec!["worktree", "add"];
        match base {
            Some(base) => args.extend(["-b", branch, &p, base]),
            None => args.extend([&*p, branch]),
        }
        self.run_ok(&args)?;
        Ok(Repo::new(path))
    }

    /// Remove a linked working tree. Without `force`, git refuses one with
    /// uncommitted changes.
    pub fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), String> {
        let p = path.to_string_lossy();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&p);
        self.run_ok(&args).map(|_| ())
    }

    // ── Remotes ──────────────────────────────────────────────────────────

    pub fn push_args(&self) -> Result<Vec<String>, String> {
        if self.succeeds(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]) {
            return Ok(vec!["push".into()]);
        }
        let branch = self.run_ok(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        Ok(vec![
            "push".into(),
            "--set-upstream".into(),
            "origin".into(),
            branch.trim().to_string(),
        ])
    }
}

/// A patch [`Repo::apply_lines`] applied, kept so it can be taken back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedPatch {
    pub patch: String,
    /// Applied to the index rather than the working tree.
    pub cached: bool,
    pub reverse: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineAction {
    Stage,
    Unstage,
    Discard,
}

/// Does this error message indicate that the working tree blocked the switch?
pub fn is_worktree_conflict(err: &str) -> bool {
    err.contains("would be overwritten") || err.contains("local changes")
}

fn stash_ref(index: usize) -> String {
    format!("stash@{{{index}}}")
}

/// `'it'\''s'` — safe to paste into a shell command line.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// git's error, from stderr, or stdout when it reports there (merges do).
fn error_text(out: &Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    if err.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        err.into_owned()
    }
}

/// Flags that make patch output independent of the user's git config
/// (`diff.noprefix`, `color.ui`, external diff tools, textconv filters).
const PATCH_FLAGS: [&str; 6] = [
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "-M",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// %x1f = unit separator, %x1e = record separator.
const LOG_FORMAT: &str = "%H%x1f%h%x1f%P%x1f%D%x1f%s%x1f%an%x1f%at%x1e";

fn parse_log(out: &str) -> Vec<Commit> {
    let mut commits = Vec::new();
    for record in out.split('\x1e') {
        let record = record.trim_start_matches('\n');
        if record.is_empty() {
            continue;
        }
        let fields: Vec<&str> = record.split('\x1f').collect();
        if fields.len() < 7 {
            continue;
        }
        let parents = fields[2].split_whitespace().map(str::to_string).collect();
        let (refs, head_ref) = parse_refs(fields[3]);
        commits.push(Commit {
            hash: fields[0].to_string(),
            short: fields[1].to_string(),
            parents,
            refs,
            head_ref,
            subject: fields[4].to_string(),
            author: fields[5].to_string(),
            timestamp: fields[6].trim().parse().unwrap_or(0),
        });
    }
    commits
}

fn parse_branches(out: &str) -> Vec<Branch> {
    let mut branches = Vec::new();
    for line in out.lines() {
        let mut parts = line.splitn(2, '\t');
        let head_marker = parts.next().unwrap_or("");
        let refname = parts.next().unwrap_or("");
        let is_current = head_marker.trim() == "*";
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            branches.push(Branch {
                name: name.to_string(),
                is_current,
                is_remote: false,
            });
        } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
            // Skip "origin/HEAD" symbolic refs — they're noise.
            if name.ends_with("/HEAD") {
                continue;
            }
            branches.push(Branch {
                name: name.to_string(),
                is_current: false,
                is_remote: true,
            });
        }
    }
    branches
}

/// `git worktree list --porcelain`: blank-line-separated records of
/// `worktree <path>`, `HEAD <sha>`, `branch refs/heads/<name>` or
/// `detached`, and optional `bare`, `locked`, `prunable` lines.
fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut list: Vec<Worktree> = Vec::new();
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            list.push(Worktree {
                path: PathBuf::from(path),
                head: String::new(),
                branch: None,
                is_main: list.is_empty(),
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(wt) = list.last_mut() else { continue };
        if let Some(sha) = line.strip_prefix("HEAD ") {
            wt.head = sha.to_string();
        } else if let Some(b) = line.strip_prefix("branch ") {
            wt.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        } else if line == "locked" || line.starts_with("locked ") {
            wt.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            wt.prunable = true;
        }
    }
    list
}

/// `git blame --porcelain`: each line starts with a header
/// `<sha> <orig-line> <final-line> [<group-size>]`, followed by the commit's
/// fields the first time that commit appears, then the line itself after a
/// tab.
/// The removed runs in a `-U0 --no-prefix` diff, per file: from each
/// `@@ -start,count +… @@` (count 1 when left out, 0 for pure additions).
fn parse_removed(out: &str) -> Vec<(String, Vec<(usize, usize)>)> {
    let mut files: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    // Between `diff --git` and the first hunk; after that a `--- ` line is
    // a removed line that started with `-- `.
    let mut header = false;
    for line in out.lines() {
        if line.starts_with("diff --git ") {
            header = true;
        } else if let Some(path) = line.strip_prefix("--- ").filter(|_| header) {
            // `/dev/null` for an added file, which removes nothing.
            files.push((path.to_string(), Vec::new()));
        } else if let Some(hunk) = line.strip_prefix("@@ -") {
            header = false;
            let old = hunk.split(' ').next().unwrap_or("");
            let (start, count) = match old.split_once(',') {
                Some((s, c)) => (s.parse().unwrap_or(0), c.parse().unwrap_or(0)),
                None => (old.parse().unwrap_or(0), 1),
            };
            if let Some((_, runs)) = files.last_mut() {
                if count > 0 && start > 0 {
                    runs.push((start, count));
                }
            }
        }
    }
    files.retain(|(path, runs)| path != "/dev/null" && !runs.is_empty());
    files
}

fn parse_blame(out: &str) -> Vec<BlameLine> {
    #[derive(Default, Clone)]
    struct Info {
        author: String,
        time: i64,
        summary: String,
    }
    let mut info: HashMap<String, Info> = HashMap::new();
    let mut lines = Vec::new();
    let mut current: Option<(String, usize)> = None;
    for line in out.lines() {
        if let Some(text) = line.strip_prefix('\t') {
            if let Some((hash, line_no)) = current.take() {
                let i = info.get(&hash).cloned().unwrap_or_default();
                lines.push(BlameLine {
                    hash,
                    author: i.author,
                    time: i.time,
                    summary: i.summary,
                    line: line_no,
                    text: text.to_string(),
                });
            }
            continue;
        }
        let mut words = line.split(' ');
        let first = words.next().unwrap_or("");
        if first.len() == 40 && first.chars().all(|c| c.is_ascii_hexdigit()) {
            let final_line = words.nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
            current = Some((first.to_string(), final_line));
            info.entry(first.to_string()).or_default();
            continue;
        }
        let Some((hash, _)) = &current else { continue };
        let entry = info.entry(hash.clone()).or_default();
        let rest = line.split_once(' ').map(|(_, r)| r).unwrap_or("");
        match first {
            "author" => entry.author = rest.to_string(),
            "author-time" => entry.time = rest.parse().unwrap_or(0),
            "summary" => entry.summary = rest.to_string(),
            _ => {}
        }
    }
    lines
}

/// `%D` gives decorations like: "HEAD -> main, origin/main, tag: v1.0"
/// Returns (refs, head_ref) where head_ref is the branch HEAD points to.
fn parse_refs(s: &str) -> (Vec<String>, Option<String>) {
    let mut head_ref: Option<String> = None;
    let refs = s
        .split(',')
        .map(|r| r.trim())
        // `origin/HEAD` only says which branch is origin's default.
        .filter(|r| !r.is_empty() && !r.ends_with("/HEAD"))
        .map(|r| {
            if let Some(rest) = r.strip_prefix("HEAD -> ") {
                head_ref = Some(rest.to_string());
                rest.to_string()
            } else if let Some(rest) = r.strip_prefix("tag: ") {
                format!("tag:{rest}")
            } else {
                r.to_string()
            }
        })
        .collect();
    (refs, head_ref)
}

fn parse_status(out: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut fields = out.split('\0');

    while let Some(record) = fields.next() {
        if record.len() < 3 {
            continue;
        }
        let x = record.as_bytes()[0] as char;
        let y = record.as_bytes()[1] as char;
        let path = &record[3..];

        // Renames/copies are followed by an extra NUL-separated field: the
        // path the file came from. Consume it even if we don't use it.
        let orig_path = if x == 'R' || x == 'C' {
            fields.next().map(|s| s.to_string())
        } else {
            None
        };

        // Unmerged: either column U, or both added / both deleted.
        if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
            entries.push(StatusEntry {
                status: FileStatus::Conflicted,
                path: path.to_string(),
                orig_path: None,
            });
            continue;
        }
        // Untracked
        if x == '?' && y == '?' {
            entries.push(StatusEntry {
                status: FileStatus::Untracked,
                path: path.to_string(),
                orig_path: None,
            });
            continue;
        }
        // Staged (index column)
        if x != ' ' && x != '?' {
            let st = if x == 'D' {
                FileStatus::StagedDeleted
            } else {
                FileStatus::Staged
            };
            entries.push(StatusEntry {
                status: st,
                path: path.to_string(),
                orig_path: orig_path.clone(),
            });
        }
        // Working tree (worktree column)
        if y != ' ' && y != '?' {
            let st = if y == 'D' {
                FileStatus::Deleted
            } else {
                FileStatus::Modified
            };
            entries.push(StatusEntry {
                status: st,
                path: path.to_string(),
                orig_path: None,
            });
        }
    }
    entries
}

fn parse_tracking(out: &str) -> TrackingInfo {
    let mut info = TrackingInfo::default();

    for line in out.lines() {
        let rest = match line.strip_prefix("# ") {
            Some(r) => r,
            None => continue, // file entry line, not a header
        };

        if let Some(head) = rest.strip_prefix("branch.head ") {
            if head == "(detached)" {
                info.detached = true;
            } else {
                info.branch = Some(head.to_string());
            }
        } else if let Some(up) = rest.strip_prefix("branch.upstream ") {
            info.upstream = Some(up.to_string());
        } else if let Some(ab) = rest.strip_prefix("branch.ab ") {
            // format: "+N -M"
            for tok in ab.split_whitespace() {
                if let Some(n) = tok.strip_prefix('+') {
                    info.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = tok.strip_prefix('-') {
                    info.behind = n.parse().unwrap_or(0);
                }
            }
        }
    }

    info
}

fn parse_commit_details(out: &str) -> CommitDetails {
    let mut f = out.splitn(7, '\x1f');
    let mut next = || f.next().unwrap_or("").to_string();
    CommitDetails {
        author: next(),
        email: next(),
        author_time: next().trim().parse().unwrap_or(0),
        committer: next(),
        commit_time: next().trim().parse().unwrap_or(0),
        date: next(),
        message: next().trim_end().to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn commit_details_parse() {
        let d = super::parse_commit_details(
            "Ada\x1fada@x.org\x1f100\x1fBob\x1f200\x1fJan 01, 1970 at 00:01\x1fSubject\n\nBody line\n\n",
        );
        assert_eq!(d.author, "Ada");
        assert_eq!(d.email, "ada@x.org");
        assert_eq!((d.author_time, d.commit_time), (100, 200));
        assert_eq!(d.committer, "Bob");
        assert_eq!(d.date, "Jan 01, 1970 at 00:01");
        assert_eq!(d.message, "Subject\n\nBody line");
    }

    use super::*;

    #[test]
    fn parse_status_untracked_and_modified() {
        let entries = parse_status("?? new.txt\0 M lib.rs\0");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].status, FileStatus::Untracked);
        assert_eq!(entries[0].path, "new.txt");
        assert_eq!(entries[1].status, FileStatus::Modified);
        assert_eq!(entries[1].path, "lib.rs");
    }

    #[test]
    fn parse_status_staged_and_modified_same_file() {
        // "MM" = staged changes plus further unstaged edits → two entries.
        let entries = parse_status("MM src/main.rs\0");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].status, FileStatus::Staged);
        assert_eq!(entries[1].status, FileStatus::Modified);
    }

    #[test]
    fn parse_status_deletions_carry_stagedness() {
        let entries = parse_status("D  gone-staged.rs\0 D gone-worktree.rs\0");
        assert_eq!(entries[0].status, FileStatus::StagedDeleted);
        assert!(entries[0].status.is_staged());
        assert_eq!(entries[1].status, FileStatus::Deleted);
        assert!(!entries[1].status.is_staged());
    }

    #[test]
    fn parse_status_rename_keeps_both_paths() {
        let entries = parse_status("R  new-name.rs\0old-name.rs\0");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, FileStatus::Staged);
        assert_eq!(entries[0].path, "new-name.rs");
        assert_eq!(entries[0].orig_path.as_deref(), Some("old-name.rs"));
    }

    #[test]
    fn parse_status_path_with_spaces_and_quotes() {
        // -z output is unquoted, so odd paths come through verbatim.
        let entries = parse_status("?? path with \"quotes\".txt\0");
        assert_eq!(entries[0].path, "path with \"quotes\".txt");
    }

    #[test]
    fn parse_refs_head_and_tags() {
        let (refs, head) = parse_refs("HEAD -> main, origin/main, origin/HEAD, tag: v1.0");
        assert_eq!(refs, vec!["main", "origin/main", "tag:v1.0"]);
        assert_eq!(head.as_deref(), Some("main"));
    }

    #[test]
    fn parse_refs_no_head() {
        let (refs, head) = parse_refs("origin/feat/login");
        assert_eq!(refs, vec!["origin/feat/login"]);
        assert_eq!(head, None);
    }

    #[test]
    fn parse_refs_empty() {
        let (refs, head) = parse_refs("");
        assert!(refs.is_empty());
        assert!(head.is_none());
    }

    #[test]
    fn parse_tracking_ahead_behind() {
        let out = "# branch.oid abc\n# branch.head feat/x\n\
                   # branch.upstream origin/feat/x\n# branch.ab +2 -3\n";
        let info = parse_tracking(out);
        assert_eq!(info.branch.as_deref(), Some("feat/x"));
        assert_eq!(info.upstream.as_deref(), Some("origin/feat/x"));
        assert_eq!(info.ahead, 2);
        assert_eq!(info.behind, 3);
        assert!(!info.detached);
    }

    #[test]
    fn parse_tracking_detached() {
        let info = parse_tracking("# branch.oid abc123\n# branch.head (detached)\n");
        assert!(info.detached);
        assert_eq!(info.branch, None);
        assert_eq!(info.upstream, None);
        assert_eq!(info.ahead, 0);
        assert_eq!(info.behind, 0);
    }

    #[test]
    fn parse_tracking_no_upstream() {
        let info = parse_tracking("# branch.oid abc123\n# branch.head feat/x\n");
        assert_eq!(info.branch, Some("feat/x".to_string()));
        assert_eq!(info.upstream, None);
    }

    #[test]
    fn worktree_conflict_detection() {
        assert!(is_worktree_conflict(
            "error: Your local changes to the following files would be overwritten by checkout"
        ));
        assert!(!is_worktree_conflict("fatal: invalid reference: nope"));
    }
}
