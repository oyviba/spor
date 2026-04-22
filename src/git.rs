use std::process::{Command, Output};

#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub parents: Vec<String>,
    pub refs: Vec<String>,     // branch names / tags pointing here
    pub subject: String,
    pub author: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Staged,
    Modified,
    Untracked,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub status: FileStatus,
    pub path: String,
}

/// Where the current branch sits relative to its upstream.
/// `None` values mean git didn't report that field — either detached HEAD,
/// no upstream configured, or brand-new unborn branch.
#[derive(Debug, Clone, Default)]
pub struct TrackingInfo {
    pub branch: Option<String>,    // current branch name, or None if detached
    pub upstream: Option<String>,  // e.g. "origin/main"
    pub ahead: usize,              // commits we have that upstream doesn't
    pub behind: usize,             // commits upstream has that we don't
    pub detached: bool,
}

fn run(args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))
}

fn run_ok(args: &[&str]) -> Result<String, String> {
    let out = run(args)?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Load commit graph. We use ASCII unit separators to survive weird subjects.
pub fn log_all(limit: usize) -> Result<Vec<Commit>, String> {
    // %x1f = unit sep, %x1e = record sep
    let fmt = "%H%x1f%h%x1f%P%x1f%D%x1f%s%x1f%an%x1f%at%x1e";
    let n = format!("-n{limit}");
    let pretty = format!("--pretty=format:{fmt}");
    let out = run_ok(&["log", "--all", "--date-order", &n, &pretty])?;

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
        let parents = if fields[2].is_empty() {
            Vec::new()
        } else {
            fields[2].split_whitespace().map(|s| s.to_string()).collect()
        };
        let refs = parse_refs(fields[3]);
        let timestamp = fields[6].trim().parse().unwrap_or(0);
        commits.push(Commit {
            hash: fields[0].to_string(),
            short: fields[1].to_string(),
            parents,
            refs,
            subject: fields[4].to_string(),
            author: fields[5].to_string(),
            timestamp,
        });
    }
    Ok(commits)
}

/// `%D` gives decorations like: "HEAD -> main, origin/main, tag: v1.0"
/// We just want the branch names (strip HEAD->, origin/, tag: prefixes).
fn parse_refs(s: &str) -> Vec<String> {
    s.split(',')
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .map(|r| {
            if let Some(rest) = r.strip_prefix("HEAD -> ") {
                rest.to_string()
            } else if let Some(rest) = r.strip_prefix("tag: ") {
                format!("tag:{rest}")
            } else {
                r.to_string()
            }
        })
        .collect()
}

pub fn status() -> Result<Vec<StatusEntry>, String> {
    let out = run_ok(&["status", "--porcelain=v1", "-uall"])?;
    let mut entries = Vec::new();
    for line in out.lines() {
        if line.len() < 3 {
            continue;
        }
        let x = line.as_bytes()[0] as char;
        let y = line.as_bytes()[1] as char;
        let path = &line[3..];

        // Untracked
        if x == '?' && y == '?' {
            entries.push(StatusEntry {
                status: FileStatus::Untracked,
                path: path.to_string(),
            });
            continue;
        }
        // Staged (index column)
        if x != ' ' && x != '?' {
            let st = if x == 'D' {
                FileStatus::Deleted
            } else {
                FileStatus::Staged
            };
            entries.push(StatusEntry {
                status: st,
                path: path.to_string(),
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
            });
        }
    }
    Ok(entries)
}

/// Parse the branch headers from `git status --porcelain=v2 --branch`.
/// Headers look like:
///   # branch.oid <sha>
///   # branch.head <name>        (or "(detached)")
///   # branch.upstream <name>    (absent if no upstream)
///   # branch.ab +<ahead> -<behind>  (absent if no upstream)
pub fn tracking() -> Result<TrackingInfo, String> {
    let out = run_ok(&["status", "--porcelain=v2", "--branch"])?;
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

    Ok(info)
}

pub fn stage(path: &str) -> Result<(), String> {
    run_ok(&["add", "--", path]).map(|_| ())
}

pub fn unstage(path: &str) -> Result<(), String> {
    run_ok(&["restore", "--staged", "--", path]).map(|_| ())
}

pub fn commit(message: &str) -> Result<(), String> {
    run_ok(&["commit", "-m", message]).map(|_| ())
}

pub fn push() -> Result<String, String> {
    match run_ok(&["push"]) {
        Err(e) if e.contains("no upstream branch") || e.contains("has no upstream") => {
            let branch = run_ok(&["rev-parse", "--abbrev-ref", "HEAD"])?;
            let branch = branch.trim();
            run_ok(&["push", "--set-upstream", "origin", branch])
        }
        other => other,
    }
}

pub fn pull() -> Result<String, String> {
    run_ok(&["pull", "--ff-only"])
}

pub fn diff_file(path: &str, staged: bool) -> Result<String, String> {
    if staged {
        run_ok(&["diff", "--cached", "--", path])
    } else {
        run_ok(&["diff", "--", path])
    }
}

pub fn diff_commit(hash: &str) -> Result<String, String> {
    run_ok(&["show", "--stat", "--patch", hash])
}

/// Find the repo's "main" branch name. Checks common names.
pub fn main_branch() -> Option<String> {
    for candidate in &["main", "master", "trunk"] {
        if run(&["rev-parse", "--verify", candidate])
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Commits on the main branch's first-parent chain — these get pinned to lane 0.
pub fn main_chain() -> Result<Vec<String>, String> {
    let Some(main) = main_branch() else {
        return Ok(Vec::new());
    };
    let out = run_ok(&["log", "--first-parent", "--format=%H", &main])?;
    Ok(out.lines().map(|s| s.to_string()).collect())
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub name: String,        // display name, e.g. "main" or "origin/feat/x"
    pub is_current: bool,
    pub is_remote: bool,
}

/// List all branches, local and remote-tracking, excluding `origin/HEAD` stubs.
pub fn list_branches() -> Result<Vec<Branch>, String> {
    let out = run_ok(&[
        "for-each-ref",
        "--format=%(HEAD)%09%(refname)",
        "refs/heads",
        "refs/remotes",
    ])?;

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
    Ok(branches)
}

/// Attempt `git switch`. For a remote-tracking ref like `origin/feat/x`, git
/// will auto-create a local tracking branch `feat/x` if no local exists.
pub fn checkout_branch(name: &str) -> Result<(), String> {
    // Strip the remote prefix if we're checking out a remote-tracking ref.
    // `git switch origin/feat/x` is an error; `git switch feat/x` is what we want,
    // and git will set up tracking automatically.
    let target = if let Some((remote, rest)) = name.split_once('/') {
        // Heuristic: if the first segment matches a known remote, strip it.
        let remotes = run_ok(&["remote"]).unwrap_or_default();
        if remotes.lines().any(|r| r == remote) {
            rest.to_string()
        } else {
            name.to_string()
        }
    } else {
        name.to_string()
    };

    run_ok(&["switch", &target]).map(|_| ())
}

pub fn create_branch_at(name: &str, sha: &str) -> Result<(), String> {
    run_ok(&["switch", "-c", name, sha]).map(|_| ())
}

pub fn stash_push() -> Result<(), String> {
    run_ok(&["stash", "push", "-u", "-m", "spor-auto-stash"]).map(|_| ())
}

/// Does this error message indicate that the working tree blocked the switch?
pub fn is_worktree_conflict(err: &str) -> bool {
    err.contains("would be overwritten") || err.contains("local changes")
}

/// What does the remote consider its default branch? Reads
/// `refs/remotes/origin/HEAD`, which git maintains as a symbolic ref.
/// Falls back to local main/master/trunk detection.
pub fn default_base_branch() -> Option<String> {
    if let Ok(out) = run_ok(&["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        // Returns "refs/remotes/origin/main" — strip the prefix.
        if let Some(name) = out.trim().strip_prefix("refs/remotes/origin/") {
            return Some(name.to_string());
        }
    }
    main_branch()
}
