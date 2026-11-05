//! The environment an app started from the Dock or Finder needs.
//!
//! macOS starts such apps with launchd's bare `PATH`
//! (`/usr/bin:/bin:/usr/sbin:/sbin`), so `gh` from Homebrew, `claude` from
//! its installer or npm, and anything nvm, mise or asdf manage look missing,
//! though they're on the machine. [`fix_path`] asks the user's login shell
//! for the `PATH` a terminal would have, and adds the usual install folders
//! as a fallback.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MARK: &str = "__SPOR_PATH__";

/// Give this process the `PATH` the user's terminal has. Call once, at
/// startup, before any threads start. Started from a terminal (`TERM` is
/// set), the `PATH` is already right and only the fallback folders are
/// added.
pub fn fix_path() {
    if !cfg!(unix) {
        return;
    }
    let current = std::env::var("PATH").unwrap_or_default();
    let from_shell = if std::env::var_os("TERM").is_some() {
        None
    } else {
        login_shell(&shell())
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let merged = merge(&current, from_shell.as_deref(), home.as_deref(), |p| {
        p.is_dir()
    });
    std::env::set_var("PATH", merged);
}

fn shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "/bin/zsh"
            } else {
                "/bin/sh"
            }
            .into()
        })
}

/// The `PATH` the login shell sets up, or `None` if it can't say within a
/// few seconds (a slow or broken shell profile mustn't hold up the app).
/// Its profile may print anything, so the value is read between markers.
pub fn login_shell(shell: &str) -> Option<String> {
    let script = format!("printf '\\n{MARK}%s{MARK}\\n' \"$PATH\"");
    let mut child = Command::new(shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    between_marks(&out)
}

fn between_marks(out: &str) -> Option<String> {
    let start = out.find(MARK)? + MARK.len();
    let len = out[start..].find(MARK)?;
    let path = &out[start..start + len];
    (!path.trim().is_empty()).then(|| path.to_string())
}

/// Folders tools usually install into, for when the shell can't tell.
fn fallbacks(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"]
        .iter()
        .map(PathBuf::from)
        .collect();
    if let Some(home) = home {
        for d in [
            ".local/bin",
            ".claude/local",
            ".npm-global/bin",
            ".bun/bin",
            ".cargo/bin",
            ".volta/bin",
            ".local/share/mise/shims",
            ".asdf/shims",
        ] {
            dirs.push(home.join(d));
        }
    }
    dirs
}

/// The login shell's entries first (in its order), then what the process
/// had, then any fallback folder that exists — each once.
fn merge(
    current: &str,
    from_shell: Option<&str>,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut add = |entry: &str| {
        if !entry.is_empty() && !out.iter().any(|e| e == entry) {
            out.push(entry.to_string());
        }
    };
    for e in from_shell.unwrap_or("").split(':') {
        add(e);
    }
    for e in current.split(':') {
        add(e);
    }
    for dir in fallbacks(home) {
        if exists(&dir) {
            add(&dir.to_string_lossy());
        }
    }
    out.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_path_between_the_marks() {
        let noise =
            format!("Welcome!\nlast login: today\n\n{MARK}/opt/homebrew/bin:/usr/bin{MARK}\nbye\n");
        assert_eq!(
            between_marks(&noise).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(between_marks("no marks here"), None);
        assert_eq!(between_marks(&format!("{MARK}{MARK}")), None);
    }

    #[test]
    fn merges_shell_first_then_current_then_fallbacks() {
        let home = Path::new("/Users/me");
        let merged = merge(
            "/usr/bin:/bin",
            Some("/Users/me/.nvm/bin:/opt/homebrew/bin:/usr/bin"),
            Some(home),
            |p| p == Path::new("/Users/me/.local/bin") || p == Path::new("/opt/homebrew/bin"),
        );
        assert_eq!(
            merged,
            "/Users/me/.nvm/bin:/opt/homebrew/bin:/usr/bin:/bin:/Users/me/.local/bin"
        );
        // Without the shell, the fallbacks still find Homebrew.
        let merged = merge("/usr/bin:/bin", None, Some(home), |p| {
            p == Path::new("/opt/homebrew/bin")
        });
        assert_eq!(merged, "/usr/bin:/bin:/opt/homebrew/bin");
    }

    #[cfg(unix)]
    #[test]
    fn asks_a_real_login_shell() {
        // A "shell" whose profile is noisy: it prints before running the script.
        let dir = std::env::temp_dir().join(format!("spor-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("noisy-sh");
        std::fs::write(&fake, "#!/bin/sh\necho 'motd: hello'\nPATH=/from/profile:$PATH\nexport PATH\nshift\nexec /bin/sh -c \"$1\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = login_shell(fake.to_str().unwrap()).expect("a PATH");
        assert!(path.starts_with("/from/profile:"), "{path}");
        // A shell that never answers is given up on.
        let stuck = dir.join("stuck-sh");
        std::fs::write(&stuck, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&stuck, std::fs::Permissions::from_mode(0o755)).unwrap();
        let t = Instant::now();
        assert_eq!(login_shell(stuck.to_str().unwrap()), None);
        assert!(t.elapsed() < Duration::from_secs(8));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
