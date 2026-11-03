//! Process and filesystem plumbing the views don't care about: where to look
//! for a repository at startup, the recent-repositories list, settings files.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::askpass;

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
    // Where agents like Claude Code usually install themselves, too.
    let home = std::env::var("HOME").unwrap_or_default();
    let user = [
        ".local/bin",
        ".claude/local",
        ".npm-global/bin",
        ".bun/bin",
        ".cargo/bin",
    ]
    .map(|d| format!("{home}/{d}"));
    let system = ["/opt/homebrew/bin", "/usr/local/bin"].map(String::from);
    for extra in system
        .iter()
        .chain(user.iter().filter(|_| !home.is_empty()))
    {
        if !parts.iter().any(|p| p == extra) && Path::new(extra).is_dir() {
            parts.push(extra.to_string());
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}

/// Where to start: a path passed on the command line, else the working
/// directory if it's inside a repo (Finder launches apps in `/`, which never
/// is). Without either, the app reopens the active workspace's repository.
pub fn initial_repo() -> Option<PathBuf> {
    if let Some(arg) = std::env::args_os().nth(1).filter(|a| !a.is_empty()) {
        return Some(PathBuf::from(arg));
    }
    std::env::current_dir()
        .ok()
        .filter(|cwd| cwd != Path::new("/") && repo_root(cwd).is_ok())
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
/// line, minus its prefix. When that line only says the remote couldn't be
/// reached, report the reason ssh printed before it instead.
pub fn git_error(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let last = lines
        .iter()
        .rposition(|l| l.starts_with("fatal:") || l.starts_with("error:"));
    let mut line = match last {
        Some(ix) => lines[ix],
        None => lines.first().copied().unwrap_or("unknown error"),
    };
    if is_remote_boilerplate(line) {
        let before = &lines[..last.unwrap_or(lines.len())];
        if let Some(cause) = before
            .iter()
            .rev()
            .find(|l| !is_remote_boilerplate(l) && !l.starts_with("hint:"))
        {
            line = cause;
        }
    }
    let line = line
        .trim_start_matches("fatal: ")
        .trim_start_matches("error: ")
        .trim();
    if line.contains("Permission denied (publickey") {
        return format!("{line} The remote didn't accept your SSH key.");
    }
    line.to_string()
}

/// What git says whenever the transport fails, whatever the cause.
fn is_remote_boilerplate(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "could not read from remote repository",
        "please make sure you have the correct access rights",
        "and the repository exists",
        "the remote end hung up unexpectedly",
        "could not fetch ",
    ]
    .iter()
    .any(|s| lower.contains(s))
}

/// ssh must never wait on a terminal prompt we can't show; fail fast
/// instead. Respects a user-set command.
fn batch_ssh_command() -> String {
    std::env::var("GIT_SSH_COMMAND").unwrap_or_else(|_| "ssh -o BatchMode=yes".into())
}

/// Run a network git command the user asked for. A passphrase, password or
/// new-host question from ssh or git opens a dialog (`askpass`). Returns
/// stdout.
pub fn git_network(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = network_command(root, args);
    match askpass::env() {
        Some(env) => {
            askpass::take_cancelled();
            cmd.envs(env);
        }
        None => {
            cmd.env("GIT_SSH_COMMAND", batch_ssh_command());
        }
    }
    let result = run_network(cmd);
    if result.is_err() && askpass::take_cancelled() {
        return Err("Cancelled".into());
    }
    result
}

/// Run a network git command nobody is waiting on (the background fetch).
/// It never asks anything; without stored credentials it just fails.
pub fn git_network_quiet(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = network_command(root, args);
    cmd.env("GIT_SSH_COMMAND", batch_ssh_command());
    run_network(cmd)
}

fn network_command(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null());
    cmd
}

fn run_network(mut cmd: Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("Couldn't run git: {e}"))?;
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

#[cfg(test)]
mod tests {
    use super::git_error;

    #[test]
    fn git_error_reports_why_ssh_failed() {
        let stderr = "git@github.com: Permission denied (publickey).\r\n\
                      fatal: Could not read from remote repository.\n\n\
                      Please make sure you have the correct access rights\n\
                      and the repository exists.\n\
                      error: could not fetch origin\n";
        assert_eq!(
            git_error(stderr),
            "git@github.com: Permission denied (publickey). The remote didn't accept your SSH key."
        );
        let stderr = "Host key verification failed.\n\
                      fatal: Could not read from remote repository.\n\n\
                      Please make sure you have the correct access rights\n\
                      and the repository exists.\n";
        assert_eq!(git_error(stderr), "Host key verification failed.");
    }

    #[test]
    fn git_error_keeps_specific_errors() {
        let stderr = "hint: Diverging branches can't be fast-forwarded.\n\
                      fatal: Not possible to fast-forward, aborting.\n";
        assert_eq!(git_error(stderr), "Not possible to fast-forward, aborting.");
        assert_eq!(
            git_error("fatal: Could not read from remote repository.\n"),
            "Could not read from remote repository."
        );
    }
}
