//! Credential prompts from git and ssh, answered in a dialog.
//!
//! A window app has no terminal, so ssh can't ask for a key's passphrase
//! (or whether to trust a new host) and git can't ask for an HTTPS
//! password. Pull, Push, Sync and Fetch run git with this same binary as
//! `SSH_ASKPASS`/`GIT_ASKPASS`. Started that way, spor-app doesn't open a
//! window: it hands the prompt to the running app over a Unix socket in a
//! private directory, the app shows a dialog, and the helper prints the
//! answer for ssh or git to read.

use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

/// Set on the git process; its presence makes spor-app act as the helper.
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
