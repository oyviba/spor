//! Process and filesystem plumbing the views don't care about: where to look
//! for a repository at startup, the recent-repositories list, settings files.

use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_RECENT: usize = 10;

/// Apps launched from Finder/Dock inherit launchd's bare PATH
/// (/usr/bin:/bin:/usr/sbin:/sbin), which hides Homebrew's `git` and `gh`.
/// Append the usual install locations so PR badges and newer gits work.
pub fn fix_path_for_gui_launch() {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current
        .split(':')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !parts.iter().any(|p| p == extra) && Path::new(extra).is_dir() {
            parts.push(extra.to_string());
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}

/// Where to start: a path passed on the command line, else the working
/// directory if it's inside a repo (Finder launches apps in `/`, which never
/// is), else the most recently opened repository.
pub fn initial_repo() -> Option<PathBuf> {
    if let Some(arg) = std::env::args_os().nth(1) {
        return Some(PathBuf::from(arg));
    }
    std::env::current_dir()
        .ok()
        .filter(|cwd| cwd != Path::new("/") && repo_root(cwd).is_ok())
        .or_else(|| recent_repos().into_iter().next())
}

/// `~/Library/Application Support/spor/` on macOS,
/// `$XDG_CONFIG_HOME/spor/` (or `~/.config/spor/`) elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home?.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))?
    };
    Some(base.join("spor"))
}

pub fn read_setting(name: &str) -> Option<String> {
    std::fs::read_to_string(config_dir()?.join(name))
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn write_setting(name: &str, value: &str) {
    if let Some(dir) = config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(name), value);
    }
}

/// Recently opened repositories, newest first.
pub fn recent_repos() -> Vec<PathBuf> {
    read_setting("recent-repos")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

pub fn remember_repo(root: &Path) {
    let mut list = recent_repos();
    list.retain(|p| p != root);
    list.insert(0, root.to_path_buf());
    list.truncate(MAX_RECENT);
    let body: Vec<String> = list.iter().map(|p| p.to_string_lossy().into()).collect();
    write_setting("recent-repos", &body.join("\n"));
}

/// Resolve `dir` to its repository root, or explain why it isn't one.
pub fn repo_root(dir: &Path) -> Result<PathBuf, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("Couldn't run git: {e}"))?;
    if !out.status.success() {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string());
        return Err(format!("“{name}” isn't a Git repository"));
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// `/Users/me/code/spor` → `~/code/spor`.
pub fn tildify(path: &Path) -> String {
    let s = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

pub fn first_line(e: &str) -> &str {
    e.lines().next().unwrap_or(e)
}

/// git prints hints around the real error; keep the last "fatal:"/"error:"
/// line, minus its prefix.
pub fn git_error(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        .or_else(|| stderr.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or("unknown error");
    line.trim_start_matches("fatal: ")
        .trim_start_matches("error: ")
        .trim()
        .to_string()
}

/// ssh must never wait on a terminal prompt we can't show; fail fast
/// instead. Respects a user-set command.
pub fn ssh_command() -> String {
    std::env::var("GIT_SSH_COMMAND").unwrap_or_else(|_| "ssh -o BatchMode=yes".into())
}

/// Run a network git command with prompts disabled. Returns stdout.
pub fn git_network(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", ssh_command())
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Couldn't run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(git_error(&String::from_utf8_lossy(&out.stderr)))
    }
}

pub fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// "3 hours ago" style, no timezone needed.
pub fn relative_time(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let dt = (now - timestamp).max(0);
    let ago = |n: i64, unit: &str| format!("{} ago", plural(n as usize, unit));
    match dt {
        0..=59 => "just now".into(),
        60..=3599 => ago(dt / 60, "minute"),
        3600..=86_399 => ago(dt / 3600, "hour"),
        86_400..=172_799 => "yesterday".into(),
        172_800..=604_799 => ago(dt / 86_400, "day"),
        604_800..=2_591_999 => ago(dt / 604_800, "week"),
        2_592_000..=31_535_999 => ago(dt / 2_592_000, "month"),
        _ => ago(dt / 31_536_000, "year"),
    }
}
