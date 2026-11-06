//! Checkpoints: a snapshot of a working tree — committed, staged, unstaged
//! and untracked work alike — as a commit behind a ref of spor's own
//! (`refs/spor/…`). Nothing is added to a branch and the index is left
//! alone: the snapshot is built in a copy of the index.
//!
//! Agent runs take one before and one after (see `agent`), which gives each
//! run its own diff and lets a run be undone.

use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// Snapshot the working tree of the current directory to `$SPOR_REF`, and
/// print the commit. POSIX sh, so the detached agent wrapper can run it
/// after the agent exits, without spor.
///
/// The index is copied and `git add -A` runs on the copy, so ignored files
/// stay out and the real index (with what's staged) is untouched. The
/// commit's parent is HEAD, so restoring can put the branch back too.
pub const SNAPSHOT_SH: &str = r#"set -e
export GIT_AUTHOR_NAME=spor GIT_AUTHOR_EMAIL=spor@localhost
export GIT_COMMITTER_NAME=spor GIT_COMMITTER_EMAIL=spor@localhost
idx=$(git rev-parse --path-format=absolute --git-path index)
tmp="$(git rev-parse --path-format=absolute --git-common-dir)/spor-snapshot-$$"
trap 'rm -f "$tmp" "$tmp.lock"' EXIT
rm -f "$tmp"
if [ -f "$idx" ]; then cp "$idx" "$tmp"; fi
GIT_INDEX_FILE="$tmp" git add -A
tree=$(GIT_INDEX_FILE="$tmp" git write-tree)
if parent=$(git rev-parse -q --verify HEAD); then
  commit=$(git commit-tree "$tree" -p "$parent" -m "spor checkpoint")
else
  commit=$(git commit-tree "$tree" -m "spor checkpoint")
fi
git update-ref "$SPOR_REF" "$commit"
echo "$commit"
"#;

fn git(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    cmd
}

fn ok(out: std::io::Result<Output>) -> Result<String, String> {
    let out = out.map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Snapshot `dir`'s working tree to `refname`; returns the commit.
pub fn capture(dir: &Path, refname: &str) -> Result<String, String> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(SNAPSHOT_SH)
        .current_dir(dir)
        .env("SPOR_REF", refname)
        .stdin(Stdio::null())
        .output();
    ok(out).map(|s| s.trim().to_string())
}

/// The commit `refname` points at, if it exists.
pub fn resolve(dir: &Path, refname: &str) -> Option<String> {
    ok(git(
        dir,
        &[
            "rev-parse",
            "-q",
            "--verify",
            &format!("{refname}^{{commit}}"),
        ],
    )
    .output())
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// The patch from one snapshot to another, limited to `paths` if given.
pub fn diff(dir: &Path, from: &str, to: &str, paths: Option<&[String]>) -> Result<String, String> {
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "-M",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        from,
        to,
    ];
    if let Some(paths) = paths {
        if paths.is_empty() {
            return Ok(String::new());
        }
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
    }
    ok(git(dir, &args).output())
}

/// Paths that differ between two commits (or trees).
pub fn changed(dir: &Path, from: &str, to: &str) -> Result<Vec<String>, String> {
    let out = ok(git(
        dir,
        &["diff", "--no-renames", "--name-only", "-z", from, to],
    )
    .output())?;
    Ok(out
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect())
}

/// The commit a snapshot was taken on top of (HEAD at the time).
pub fn parent(dir: &Path, snapshot: &str) -> Option<String> {
    resolve(dir, &format!("{snapshot}^"))
}

/// Put `dir`'s working tree back as `snapshot` had it: the branch where it
/// was (the snapshot's parent), and every file as it was, including
/// uncommitted and untracked ones. Files the snapshot didn't have are
/// removed; ignored files are left alone. What was staged comes back
/// unstaged.
pub fn restore(dir: &Path, snapshot: &str) -> Result<(), String> {
    if let Some(parent) = parent(dir, snapshot) {
        ok(git(dir, &["reset", "-q", "--hard", &parent]).output())?;
    }
    // Write the snapshot's files through a scratch index, so the real one
    // stays at the branch's commit.
    let common = ok(git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .output())?;
    let tmp = Path::new(common.trim()).join(format!("spor-restore-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let with_tmp = |args: &[&str]| ok(git(dir, args).env("GIT_INDEX_FILE", &tmp).output());
    let written =
        with_tmp(&["read-tree", snapshot]).and_then(|_| with_tmp(&["checkout-index", "-a", "-f"]));
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("lock"));
    written?;
    // Remove what the snapshot didn't have: tracked files it had deleted,
    // and untracked files made since.
    let keep: HashSet<String> =
        ok(git(dir, &["ls-tree", "-r", "-z", "--name-only", snapshot]).output())?
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect();
    let present = ok(git(
        dir,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )
    .output())?;
    for path in present
        .split('\0')
        .filter(|p| !p.is_empty() && !keep.contains(*p))
    {
        let _ = std::fs::remove_file(dir.join(path));
    }
    Ok(())
}

/// The refs under `prefix`.
pub fn list(dir: &Path, prefix: &str) -> Vec<String> {
    ok(git(dir, &["for-each-ref", "--format=%(refname)", prefix]).output())
        .map(|refs| {
            refs.lines()
                .filter(|r| !r.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Delete every ref under `prefix` (e.g. `refs/spor/tasks/fix-login/`).
pub fn delete_all(dir: &Path, prefix: &str) {
    for r in list(dir, prefix) {
        let _ = git(dir, &["update-ref", "-d", &r]).output();
    }
}
