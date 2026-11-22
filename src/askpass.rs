//! Credential prompts from git and ssh, answered in a dialog.
//!
//! A window app has no terminal, so ssh can't ask for a key's passphrase
//! (or whether to trust a new host) and git can't ask for an HTTPS
//! password. Pull, Push, Sync and Fetch run git with this same binary as
//! `SSH_ASKPASS`/`GIT_ASKPASS`. Started that way, the app doesn't open a
//! window: it hands the prompt to the running app over a Unix socket in a
//! private directory, the app shows a dialog, and the helper prints the
//! answer for ssh or git to read.

use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

/// Set on the git process; its presence makes the app act as the helper.
const SOCKET_ENV: &str = "SPOR_ASKPASS_SOCKET";

/// A prompt was dismissed since the last `take_cancelled`.
static CANCELLED: AtomicBool = AtomicBool::new(false);

static SERVER: OnceLock<Option<Server>> = OnceLock::new();

struct Server {
    socket: PathBuf,
    requests: Mutex<Receiver<Request>>,
}

/// One question from ssh or git, waiting for the user.
pub struct Request {
    pub prompt: String,
    reply: Option<Sender<Option<String>>>,
}

impl Request {
    /// Send the answer back; `None` means the user cancelled.
    pub fn answer(mut self, answer: Option<String>) {
        self.send(answer);
    }

    fn send(&mut self, answer: Option<String>) {
        if let Some(reply) = self.reply.take() {
            if answer.is_none() {
                CANCELLED.store(true, Ordering::SeqCst);
            }
            let _ = reply.send(answer);
        }
    }

    /// ssh asking whether to trust a host it hasn't seen.
    pub fn is_confirmation(&self) -> bool {
        self.prompt.contains("(yes/no")
    }

    /// Everything but a username is typed hidden.
    pub fn is_secret(&self) -> bool {
        !self.prompt.trim_start().starts_with("Username")
    }
}

impl Drop for Request {
    /// A dialog closed any other way counts as Cancel, so ssh never waits
    /// on an answer that won't come.
    fn drop(&mut self) {
        self.send(None);
    }
}

/// When git started this binary as its askpass helper, answer the prompt
/// and exit; otherwise return and let the app start.
pub fn run_helper_if_invoked() {
    let Ok(socket) = std::env::var(SOCKET_ENV) else {
        return;
    };
    let prompt = std::env::args().nth(1).unwrap_or_default();
    match ask(&socket, &prompt) {
        Some(answer) => {
            println!("{answer}");
            std::process::exit(0);
        }
        None => std::process::exit(1),
    }
}

/// Environment for a git command that may need to ask for credentials, or
/// `None` where the app can't answer prompts (then git runs without them).
pub fn env() -> Option<Vec<(&'static str, std::ffi::OsString)>> {
    let server = SERVER.get_or_init(start).as_ref()?;
    let exe = std::env::current_exe().ok()?;
    Some(vec![
        ("SSH_ASKPASS", exe.clone().into()),
        ("SSH_ASKPASS_REQUIRE", "force".into()),
        ("GIT_ASKPASS", exe.into()),
        (SOCKET_ENV, server.socket.clone().into()),
    ])
}

/// The next prompt waiting for a dialog, if any.
pub fn next_request() -> Option<Request> {
    let server = SERVER.get()?.as_ref()?;
    server.requests.lock().ok()?.try_recv().ok()
}

/// Whether the user cancelled a prompt since the last call.
pub fn take_cancelled() -> bool {
    CANCELLED.swap(false, Ordering::SeqCst)
}

#[cfg(unix)]
fn start() -> Option<Server> {
    use std::os::unix::fs::DirBuilderExt as _;
    use std::os::unix::net::UnixListener;

    // Only this user can reach the socket: the directory is 0700.
    let dir = std::env::temp_dir().join(format!("spor-askpass-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new().mode(0o700).create(&dir).ok()?;
    let socket = dir.join("socket");
    let listener = UnixListener::bind(&socket).ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("askpass".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve(stream, tx));
            }
        })
        .ok()?;
    Some(Server {
        socket,
        requests: Mutex::new(rx),
    })
}

#[cfg(not(unix))]
fn start() -> Option<Server> {
    None
}

/// One helper connection: read the prompt to EOF, wait for the dialog,
/// write `y<answer>` or `n`.
#[cfg(unix)]
fn serve(mut stream: std::os::unix::net::UnixStream, tx: Sender<Request>) {
    let mut prompt = String::new();
    if stream.read_to_string(&mut prompt).is_err() {
        return;
    }
    let (reply, answer) = std::sync::mpsc::channel();
    let request = Request {
        prompt: prompt.trim_end().to_string(),
        reply: Some(reply),
    };
    if tx.send(request).is_err() {
        return;
    }
    let response = match answer.recv() {
        Ok(Some(answer)) => format!("y{answer}"),
        _ => "n".to_string(),
    };
    let _ = stream.write_all(response.as_bytes());
}

#[cfg(unix)]
fn ask(socket: &str, prompt: &str) -> Option<String> {
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket).ok()?;
    stream.write_all(prompt.as_bytes()).ok()?;
    stream.shutdown(Shutdown::Write).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    response.strip_prefix('y').map(str::to_string)
}

#[cfg(not(unix))]
fn ask(_: &str, _: &str) -> Option<String> {
    None
}

// ── Running network git commands ─────────────────────────────────────────

/// Run a network git command the user asked for (fetch, pull, push). A
/// passphrase, password or new-host question from ssh or git goes to the
/// app's dialog (see [`next_request`]); where it can't, git fails instead
/// of waiting on a terminal. Returns stdout, or git's real error.
pub fn git_network(root: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = network_command(root, args);
    match env() {
        Some(env) => {
            take_cancelled();
            cmd.envs(env);
        }
        None => {
            cmd.env("GIT_SSH_COMMAND", batch_ssh_command());
        }
    }
    let result = run_network(cmd);
    if result.is_err() && take_cancelled() {
        return Err("Cancelled".into());
    }
    result
}

/// Run a network git command nobody is waiting on (the background fetch).
/// It never asks anything; without stored credentials it just fails.
pub fn git_network_quiet(root: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = network_command(root, args);
    cmd.env("GIT_SSH_COMMAND", batch_ssh_command());
    run_network(cmd)
}

/// ssh must never wait on a terminal prompt nobody can see; fail fast
/// instead. Respects a user-set command.
fn batch_ssh_command() -> String {
    std::env::var("GIT_SSH_COMMAND").unwrap_or_else(|_| "ssh -o BatchMode=yes".into())
}

fn network_command(root: &std::path::Path, args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null());
    cmd
}

fn run_network(mut cmd: std::process::Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("Couldn't run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(git_error(&String::from_utf8_lossy(&out.stderr)))
    }
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
    if let Some(why) = push_rejection(&lines) {
        return why;
    }
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

/// What a push refused because the remote has commits this branch doesn't
/// says, so the app can offer to pull them and push again.
pub const PUSH_BEHIND: &str = "has commits you don't have";

/// Did this push fail because the remote branch moved ahead of ours?
pub fn is_push_behind(err: &str) -> bool {
    err.contains(PUSH_BEHIND)
}

/// git reports a refused push as ` ! [rejected] main -> main (fetch first)`
/// and then "error: failed to push some refs", which says nothing; say why.
fn push_rejection(lines: &[&str]) -> Option<String> {
    let line = lines.iter().find(|l| l.starts_with("! ["))?;
    let remote = line.starts_with("! [remote rejected]");
    let rest = line.split_once(']').map(|(_, r)| r.trim()).unwrap_or("");
    let (refs, reason) = match rest.rfind(" (") {
        Some(ix) if rest.ends_with(')') => (&rest[..ix], &rest[ix + 2..rest.len() - 1]),
        _ => (rest, ""),
    };
    let target = refs.rsplit("-> ").next().unwrap_or(refs).trim();
    Some(if remote {
        format!("The remote refused {target}: {reason}")
    } else {
        match reason {
            "fetch first" | "non-fast-forward" => format!(
                "{target} on the remote {PUSH_BEHIND} — pull them, then push"
            ),
            "stale info" | "remote ref updated since checkout" => format!(
                "{target} on the remote changed since you last pulled — someone else may have pushed. \
                 Not forcing over it: fetch and look first"
            ),
            "already exists" => format!("{target} already exists on the remote"),
            "" => format!("The remote rejected {target}"),
            other => format!("The remote rejected {target} ({other})"),
        }
    })
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for_request() -> Request {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(request) = next_request() {
                return request;
            }
            assert!(Instant::now() < deadline, "no prompt arrived");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // One test: the server and the cancel flag are process-wide.
    #[test]
    fn prompts_round_trip_through_the_app() {
        let env = env().expect("askpass server starts");
        let socket = env
            .iter()
            .find(|(k, _)| *k == SOCKET_ENV)
            .map(|(_, v)| v.to_string_lossy().into_owned())
            .unwrap();

        let s = socket.clone();
        let helper = std::thread::spawn(move || ask(&s, "Enter passphrase for key 'k':\n"));
        let request = wait_for_request();
        assert_eq!(request.prompt, "Enter passphrase for key 'k':");
        assert!(request.is_secret() && !request.is_confirmation());
        request.answer(Some("hunter 2".into()));
        assert_eq!(helper.join().unwrap().as_deref(), Some("hunter 2"));
        assert!(!take_cancelled());

        let s = socket.clone();
        let helper = std::thread::spawn(move || ask(&s, "Username for 'https://x': "));
        let request = wait_for_request();
        assert!(!request.is_secret());
        drop(request);
        assert_eq!(helper.join().unwrap(), None);
        assert!(take_cancelled());
        assert!(!take_cancelled());
    }
}

#[cfg(test)]
mod error_tests {
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
    fn git_error_says_why_a_push_was_refused() {
        let stderr = "To github.com:o/r.git\n ! [rejected]        main -> main (fetch first)\n\
                      error: failed to push some refs to 'github.com:o/r.git'\n\
                      hint: Updates were rejected because the remote contains work that you do\n";
        let err = git_error(stderr);
        assert_eq!(
            err,
            "main on the remote has commits you don't have — pull them, then push"
        );
        assert!(super::is_push_behind(&err));
        let stderr = " ! [rejected]        feat/x -> feat/x (stale info)\nerror: failed to push some refs to 'x'\n";
        assert!(git_error(stderr).starts_with("feat/x on the remote changed since you last pulled"));
        let stderr = " ! [remote rejected] main -> main (protected branch hook declined)\n\
                      error: failed to push some refs to 'x'\n";
        assert_eq!(
            git_error(stderr),
            "The remote refused main: protected branch hook declined"
        );
        let stderr = " ! [rejected]        v1.0 -> v1.0 (already exists)\nerror: failed to push some refs to 'x'\n";
        assert_eq!(git_error(stderr), "v1.0 already exists on the remote");
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
