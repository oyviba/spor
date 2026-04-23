//! Background git jobs with in-app credential prompts.
//!
//! When we run `git push` / `git pull`, git may need to ask for a password
//! (HTTPS credentials, or an SSH key passphrase via ssh). Rather than tearing
//! down the TUI to let git prompt on the terminal, we:
//!
//!   1. Bind a Unix domain socket at a temp path.
//!   2. Spawn git with `GIT_ASKPASS` / `SSH_ASKPASS` pointed at this same
//!      binary, and `SPOR_ASKPASS_SOCK` set to the socket path.
//!   3. The askpass helper (see `run_askpass` in `main.rs`) connects to the
//!      socket, writes the prompt, reads a password, and prints it on stdout.
//!   4. On the main side, an accept thread reads each prompt and publishes it
//!      as a `JobEvent::Askpass` — the main event loop opens a modal, collects
//!      input, and sends it back over a one-shot channel.
//!
//! The TUI keeps rendering the whole time.
//!
//! Unix-only. The existing askpass flow was already unix-specific.
#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub enum JobEvent {
    /// git (via its askpass helper) is asking for a credential.
    /// Main thread opens a modal, then sends the answer on `reply`.
    Askpass { prompt: String, reply: Sender<String> },
    /// Child exited. On failure, the string is git's stderr (first useful line).
    Done(Result<String, String>),
}

pub struct JobHandle {
    pub label: String,
    pub events: Receiver<JobEvent>,
    child: Arc<Mutex<Option<Child>>>,
    shutdown: Arc<AtomicBool>,
    dir: PathBuf,
}

impl JobHandle {
    /// Forcefully kill the child. Used when the user cancels an askpass prompt
    /// — otherwise git/ssh would just retry and ask again.
    pub fn cancel(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Best-effort: make sure the child is gone.
        self.cancel();
        // Clean up the socket dir. The accept thread also tries; whichever
        // happens first wins.
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub fn start(label: String, args: Vec<String>) -> Result<JobHandle, String> {
    let dir = make_tmp_dir().map_err(|e| format!("askpass tmp: {e}"))?;
    let sock_path = dir.join("askpass.sock");

    let listener = UnixListener::bind(&sock_path).map_err(|e| format!("bind: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("nonblock: {e}"))?;
    fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600)).ok();

    let exe = std::env::current_exe().map_err(|e| format!("exe: {e}"))?;

    let mut cmd = Command::new("git");
    cmd.args(&args)
        .env("SSH_ASKPASS", &exe)
        // `force` makes ssh call the askpass even when a tty is attached
        // (OpenSSH ≥ 8.4). The pre_exec setsid below handles older clients.
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("GIT_ASKPASS", &exe)
        .env("SPOR_ASKPASS", "1")
        .env("SPOR_ASKPASS_SOCK", &sock_path)
        // Don't let git fall back to reading the tty for HTTPS credentials.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Put the child in its own session so it has no controlling terminal.
    // Without this, older ssh (< 8.4) will open /dev/tty directly to prompt
    // for a passphrase, stomping on the TUI.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            extern "C" {
                fn setsid() -> i32;
            }
            if setsid() == -1 {
                // Already a session leader is fine — just means we're good.
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() != Some(libc_eperm()) {
                    return Err(e);
                }
            }
            Ok(())
        });
    }

    let mut child = cmd.spawn().map_err(|e| format!("spawn git: {e}"))?;

    let (events_tx, events_rx) = mpsc::channel::<JobEvent>();
    let shutdown = Arc::new(AtomicBool::new(false));

    // Take stderr out of the child so we can read it concurrently with wait().
    let stderr = child.stderr.take();

    let child_arc: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(Some(child)));

    // Accept thread: hand each incoming connection off to a worker.
    {
        let events_tx = events_tx.clone();
        let shutdown = shutdown.clone();
        thread::spawn(move || {
            loop {
                match listener.accept() {
                    Ok((conn, _)) => {
                        let tx = events_tx.clone();
                        thread::spawn(move || handle_conn(conn, tx));
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        if shutdown.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // Wait thread: drain stderr, wait for the child, then notify.
    {
        let events_tx = events_tx.clone();
        let shutdown = shutdown.clone();
        let child_arc = child_arc.clone();
        thread::spawn(move || {
            let err_text = stderr
                .map(|mut s| {
                    let mut buf = String::new();
                    use std::io::Read;
                    let _ = s.read_to_string(&mut buf);
                    buf
                })
                .unwrap_or_default();

            let status = {
                let mut guard = match child_arc.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                match guard.as_mut() {
                    Some(c) => c.wait().ok(),
                    None => None,
                }
            };

            shutdown.store(true, Ordering::Relaxed);

            let result = match status {
                Some(s) if s.success() => Ok(err_text),
                Some(s) => {
                    let msg = first_nonempty(&err_text)
                        .unwrap_or_else(|| format!("git exited with {s}"));
                    Err(msg)
                }
                None => Err("git did not start".into()),
            };

            let _ = events_tx.send(JobEvent::Done(result));
        });
    }

    Ok(JobHandle {
        label,
        events: events_rx,
        child: child_arc,
        shutdown,
        dir,
    })
}

fn handle_conn(mut conn: UnixStream, events_tx: Sender<JobEvent>) {
    let read_side = match conn.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(read_side);
    let mut prompt = String::new();
    if reader.read_line(&mut prompt).is_err() {
        return;
    }
    let prompt = prompt.trim_end_matches(['\n', '\r']).to_string();

    let (reply_tx, reply_rx) = mpsc::channel::<String>();
    if events_tx
        .send(JobEvent::Askpass {
            prompt,
            reply: reply_tx,
        })
        .is_err()
    {
        return;
    }

    // Wait for the UI to collect the password. If the main side drops the
    // sender (e.g. job cancelled), we get an Err and fall through — writing
    // nothing is what the child sees as an empty reply.
    let reply = reply_rx.recv().unwrap_or_default();
    let _ = writeln!(conn, "{reply}");
    let _ = conn.flush();
}

fn make_tmp_dir() -> std::io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let name = format!("spor-askpass-{}-{}", std::process::id(), nanos);
    let dir = std::env::temp_dir().join(name);
    fs::create_dir(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

fn first_nonempty(s: &str) -> Option<String> {
    s.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .map(|l| l.to_string())
}

/// EPERM on Linux/macOS. We avoid pulling in the `libc` crate just for this.
const fn libc_eperm() -> i32 {
    1
}
