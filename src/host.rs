//! The agent host: a small process that owns one Claude Code run and lets
//! spor talk to it while it works (#20).
//!
//! `claude -p` with stdin closed can't be asked anything and can't be
//! steered. Run with `--input-format stream-json --permission-prompt-tool
//! stdio` it can: it reads user messages and control requests on stdin, and
//! asks before using a tool its permission mode doesn't allow, as a
//! `control_request` (`can_use_tool`) on stdout. The host sits between:
//!
//! - it starts `claude`, sends the prompt, and appends everything Claude
//!   prints to the run's log, exactly as the plain wrapper did, so the
//!   transcript, the exit file and spor quitting and coming back all work
//!   the same;
//! - it listens on a unix socket for `message` (steer the turn),
//!   `interrupt` and `answer` (a permission question), one JSON object per
//!   line, and writes answers to the log as `spor_answer` lines, so a
//!   question and whether it was answered survive spor restarting;
//! - when Claude reports the turn's result it closes stdin and Claude exits.
//!   A run is one turn: a follow-up after that is a new run.
//!
//! An app opts in by calling [`set_program`] with an executable that runs
//! [`main`] when given [`ARG`] as its first argument (usually itself).
//! Without that, runs use the plain one-shot wrapper.

use crate::json::Json;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

/// The first argument that turns an app's executable into the host.
pub const ARG: &str = "__spor-agent-host";

static PROGRAM: OnceLock<PathBuf> = OnceLock::new();

/// Run Claude Code under the host, started as `program ARG …`.
pub fn set_program(program: PathBuf) {
    let _ = PROGRAM.set(program);
}

/// The host program, if an app opted in (unix only: it needs a socket).
pub fn program() -> Option<&'static Path> {
    if cfg!(unix) {
        PROGRAM.get().map(PathBuf::as_path)
    } else {
        None
    }
}

/// A JSON string literal.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A user message, as Claude's stream-json input takes it.
pub fn user_message(text: &str) -> String {
    format!(
        r#"{{"type":"user","message":{{"role":"user","content":{}}}}}"#,
        quote(text)
    )
}

/// The host's entry point: `ARG <log> <exit> <socket file> <after ref> -- <claude…>`,
/// with the prompt in `$SPOR_PROMPT`. Exits with Claude's code.
#[cfg(unix)]
pub fn main(args: &[String]) -> i32 {
    use std::os::unix::net::UnixListener;
    let [log, exit, sock_file, after_ref, dash, claude @ ..] = args else {
        eprintln!("usage: {ARG} <log> <exit> <socket file> <after ref> -- <claude …>");
        return 2;
    };
    if dash != "--" || claude.is_empty() {
        return 2;
    }
    let prompt = std::env::var("SPOR_PROMPT").unwrap_or_default();
    let log = Arc::new(Mutex::new(
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
        {
            Ok(f) => f,
            Err(_) => return 1,
        },
    ));
    let append = {
        let log = log.clone();
        move |line: &str| {
            let mut f = log.lock().unwrap();
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    };

    let mut child = match Command::new(&claude[0])
        .args(&claude[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            append(&format!("Couldn't start {}: {e}", claude[0]));
            let _ = std::fs::write(exit, "127\n");
            return 127;
        }
    };
    let stdin: Arc<Mutex<Option<std::process::ChildStdin>>> =
        Arc::new(Mutex::new(child.stdin.take()));
    let send = {
        let stdin = stdin.clone();
        move |line: &str| -> Result<(), String> {
            let mut guard = stdin.lock().unwrap();
            let w = guard.as_mut().ok_or("The run has finished")?;
            writeln!(w, "{line}")
                .and_then(|_| w.flush())
                .map_err(|e| e.to_string())
        }
    };

    // The socket, somewhere short (unix socket paths are limited to ~104
    // bytes), recorded beside the run so spor can find it.
    let sock = std::env::temp_dir().join(format!("spor-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).ok();
    if listener.is_some() {
        let _ = std::fs::write(sock_file, sock.to_string_lossy().as_bytes());
    }

    // Questions Claude is waiting on: request id → the tool's input, which
    // an allow has to send back, and the rules it suggests for allowing
    // this kind of action for the session (#40).
    let pending: Arc<Mutex<HashMap<String, Pending>>> = Arc::default();

    let _ = send(
        r#"{"type":"control_request","request_id":"spor-init","request":{"subtype":"initialize"}}"#,
    );
    let _ = send(&user_message(&prompt));

    // Claude's stderr goes to the log too, as plain lines.
    if let Some(err) = child.stderr.take() {
        let append = append.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                append(&line);
            }
        });
    }

    if let Some(listener) = listener {
        let send = send.clone();
        let append = append.clone();
        let pending = pending.clone();
        std::thread::spawn(move || {
            let mut n = 0u64;
            for conn in listener.incoming().map_while(Result::ok) {
                let mut reader = BufReader::new(match conn.try_clone() {
                    Ok(c) => c,
                    Err(_) => continue,
                });
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                n += 1;
                let reply = match handle(&line, n, &send, &append, &pending) {
                    Ok(()) => r#"{"ok":true}"#.to_string(),
                    Err(e) => format!(r#"{{"error":{}}}"#, quote(&e)),
                };
                let mut conn = conn;
                let _ = writeln!(conn, "{reply}");
            }
        });
    }

    // Claude's events: into the log; the turn's result closes stdin.
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            append(&line);
            if !(line.contains("\"result\"") || line.contains("can_use_tool")) {
                continue;
            }
            let Some(ev) = Json::parse(&line) else {
                continue;
            };
            match ev.get("type").str() {
                Some("result") => {
                    stdin.lock().unwrap().take();
                }
                Some("control_request")
                    if ev.get("request").get("subtype").str() == Some("can_use_tool") =>
                {
                    if let Some(id) = ev.get("request_id").str() {
                        let request = ev.get("request");
                        let suggestions = request.get("permission_suggestions");
                        pending.lock().unwrap().insert(
                            id.to_string(),
                            Pending {
                                input: request.get("input").to_json(),
                                suggestions: (!suggestions.array().is_empty())
                                    .then(|| suggestions.to_json()),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }
    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
    stdin.lock().unwrap().take();
    let _ = std::fs::remove_file(&sock);
    let _ = std::fs::remove_file(sock_file);
    let _ = crate::checkpoint::capture(Path::new("."), after_ref);
    let _ = std::fs::write(exit, format!("{code}\n"));
    code
}

#[cfg(not(unix))]
pub fn main(_args: &[String]) -> i32 {
    eprintln!("The agent host needs unix sockets");
    2
}

/// A question Claude is waiting on.
#[cfg(unix)]
struct Pending {
    /// The tool's input, which an allow sends back.
    input: String,
    /// The permission updates it suggests (a JSON array), if any.
    suggestions: Option<String>,
}

/// One command from spor: `{"message": "…"}`, `{"interrupt": true}`,
/// `{"mode": "acceptEdits"}`, or
/// `{"answer": {"request_id": "…", "allow": true, "message": "…"}}`, with
/// `"always": true` to allow for the rest of the session.
#[cfg(unix)]
fn handle(
    line: &str,
    n: u64,
    send: &impl Fn(&str) -> Result<(), String>,
    append: &impl Fn(&str),
    pending: &Mutex<HashMap<String, Pending>>,
) -> Result<(), String> {
    let cmd = Json::parse(line.trim()).ok_or("Not JSON")?;
    if let Some(text) = cmd.get("message").str() {
        send(&user_message(text))?;
        // In the log as the user's words, for the transcript.
        append(&format!(
            r#"{{"type":"spor_message","text":{}}}"#,
            quote(text)
        ));
        Ok(())
    } else if let Some(mode) = cmd.get("mode").str() {
        send(&format!(
            r#"{{"type":"control_request","request_id":"spor-mode-{n}","request":{{"subtype":"set_permission_mode","mode":{}}}}}"#,
            quote(mode)
        ))
    } else if cmd.get("interrupt").bool() == Some(true) {
        send(&format!(
            r#"{{"type":"control_request","request_id":"spor-interrupt-{n}","request":{{"subtype":"interrupt"}}}}"#
        ))
    } else if let Some(answer) =
        Some(cmd.get("answer")).filter(|a| a.get("request_id").str().is_some())
    {
        let id = answer.get("request_id").str().unwrap_or_default();
        let allow = answer.get("allow").bool() == Some(true);
        let always = allow && answer.get("always").bool() == Some(true);
        let message = answer.get("message").str().unwrap_or("");
        let question = {
            let mut pending = pending.lock().unwrap();
            let q = pending
                .get(id)
                .ok_or("That question was already answered")?;
            if always && q.suggestions.is_none() {
                return Err("This can only be allowed once".into());
            }
            pending.remove(id).unwrap()
        };
        let input = question.input;
        let response = if always {
            // The suggested rules go back with it, and Claude Code stops
            // asking about this kind of action for the session.
            format!(
                r#"{{"behavior":"allow","updatedInput":{input},"updatedPermissions":{}}}"#,
                question.suggestions.unwrap_or_default()
            )
        } else if allow {
            // An allow sends the tool's input back, unchanged.
            format!(r#"{{"behavior":"allow","updatedInput":{input}}}"#)
        } else {
            let why = if message.is_empty() {
                "The user said no.".to_string()
            } else {
                message.to_string()
            };
            format!(r#"{{"behavior":"deny","message":{}}}"#, quote(&why))
        };
        send(&format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":{},"response":{response}}}}}"#,
            quote(id)
        ))?;
        append(&format!(
            r#"{{"type":"spor_answer","request_id":{},"allow":{allow},"always":{always},"message":{}}}"#,
            quote(id),
            quote(message)
        ));
        Ok(())
    } else {
        Err("Unknown command".into())
    }
}

/// Send one command to a run's host and wait for its reply.
#[cfg(unix)]
pub fn request(sock_file: &Path, command: &str) -> Result<(), String> {
    use std::os::unix::net::UnixStream;
    let path =
        std::fs::read_to_string(sock_file).map_err(|_| "The agent isn't listening".to_string())?;
    let mut stream =
        UnixStream::connect(path.trim()).map_err(|_| "The agent isn't listening".to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    writeln!(stream, "{command}").map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| format!("The agent didn't answer: {e}"))?;
    match Json::parse(reply.trim()) {
        Some(r) if r.get("ok").bool() == Some(true) => Ok(()),
        Some(r) => Err(r
            .get("error")
            .str()
            .unwrap_or("The agent refused")
            .to_string()),
        None => Err("The agent didn't answer".into()),
    }
}

#[cfg(not(unix))]
pub fn request(_sock_file: &Path, _command: &str) -> Result<(), String> {
    Err("Talking to a running agent needs unix sockets".into())
}
