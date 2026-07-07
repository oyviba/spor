use std::process::{Command, Output};

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
            fields[2]
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        };
        let (refs, head_ref) = parse_refs(fields[3]);
        let timestamp = fields[6].trim().parse().unwrap_or(0);
        commits.push(Commit {
            hash: fields[0].to_string(),
            short: fields[1].to_string(),
            parents,
            refs,
            head_ref,
            subject: fields[4].to_string(),
            author: fields[5].to_string(),
            timestamp,
        });
    }
    Ok(commits)
}

/// `%D` gives decorations like: "HEAD -> main, origin/main, tag: v1.0"
/// Returns (refs, head_ref) where head_ref is the branch HEAD points to.
fn parse_refs(s: &str) -> (Vec<String>, Option<String>) {
    let mut head_ref: Option<String> = None;
    let refs = s
        .split(',')
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
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

/// Working tree status. `-z` gives NUL-separated records with no quoting, so
/// paths with spaces, quotes, or non-ASCII survive, and rename origins arrive
/// as a separate field instead of a literal `old -> new` string.
pub fn status() -> Result<Vec<StatusEntry>, String> {
    let out = run_ok(&["status", "--porcelain=v1", "-z", "-uall"])?;
    Ok(parse_status(&out))
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

/// Parse the branch headers from `git status --porcelain=v2 --branch`.
/// Headers look like:
///   # branch.oid <sha>
///   # branch.head <name>        (or "(detached)")
///   # branch.upstream <name>    (absent if no upstream)
///   # branch.ab +<ahead> -<behind>  (absent if no upstream)
pub fn tracking() -> Result<TrackingInfo, String> {
    let out = run_ok(&["status", "--porcelain=v2", "--branch"])?;
    Ok(parse_tracking(&out))
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

pub fn stage(path: &str) -> Result<(), String> {
    run_ok(&["add", "--", path]).map(|_| ())
}

/// Unstage an entry. A staged rename is two index operations (delete at the
/// old path, add at the new), so undoing it has to restore both paths.
pub fn unstage(entry: &StatusEntry) -> Result<(), String> {
    let mut args = vec!["restore", "--staged", "--", entry.path.as_str()];
    if let Some(orig) = &entry.orig_path {
        args.push(orig.as_str());
    }
    run_ok(&args).map(|_| ())
}

/// Throw away unstaged changes to a tracked file — reverts modifications,
/// brings back deletions. Does not touch the index.
pub fn discard_worktree(path: &str) -> Result<(), String> {
    run_ok(&["restore", "--", path]).map(|_| ())
}

/// Delete an untracked file from the working tree.
pub fn remove_untracked(path: &str) -> Result<(), String> {
    run_ok(&["clean", "-f", "--", path]).map(|_| ())
}

pub fn commit(message: &str) -> Result<(), String> {
    run_ok(&["commit", "-m", message]).map(|_| ())
}

pub fn push_args() -> Result<Vec<String>, String> {
    let has_upstream = run(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_upstream {
        return Ok(vec!["push".into()]);
    }
    let branch = run_ok(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    Ok(vec![
        "push".into(),
        "--set-upstream".into(),
        "origin".into(),
        branch.trim().to_string(),
    ])
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
    pub name: String, // display name, e.g. "main" or "origin/feat/x"
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

/// Names of configured remotes, e.g. ["origin", "upstream"].
pub fn remotes() -> Vec<String> {
    run_ok(&["remote"])
        .map(|out| out.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Attempt `git switch`. For a remote-tracking ref like `origin/feat/x`, git
/// will auto-create a local tracking branch `feat/x` if no local exists.
pub fn checkout_branch(name: &str) -> Result<(), String> {
    // Strip the remote prefix if we're checking out a remote-tracking ref.
    // `git switch origin/feat/x` is an error; `git switch feat/x` is what we want,
    // and git will set up tracking automatically.
    let target = match name.split_once('/') {
        Some((remote, rest)) if remotes().iter().any(|r| r == remote) => rest,
        _ => name,
    };

    run_ok(&["switch", target]).map(|_| ())
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

#[cfg(test)]
mod tests {
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
        let (refs, head) = parse_refs("HEAD -> main, origin/main, tag: v1.0");
        assert_eq!(refs, vec!["main", "origin/main", "tag:v1.0"]);
        assert_eq!(head.as_deref(), Some("main"));
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
        let info = parse_tracking("# branch.oid abc\n# branch.head (detached)\n");
        assert!(info.detached);
        assert!(info.branch.is_none());
    }
}
