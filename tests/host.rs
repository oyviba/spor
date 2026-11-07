//! Claude Code under the agent host (#20): questions, answers, steering and
//! interrupts, through the real host (the `spor-agent-host` binary) and a fake
//! `claude` that speaks stream-json both ways.
//!
//! Its own test binary: opting in to the host is process-wide.

use spor::agent::{AgentKind, AskKind, Entry, Mode, Permissions, Progress, RunState, Runs};
use spor::git::Repo;
use spor::tasks::{Task, Tasks};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

struct Fixture {
    dir: PathBuf,
    tasks: Tasks,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The host is the `spor-agent-host` binary; `claude` is a fake that reads stream-json
/// on stdin. What it does depends on the prompt: "ask" asks to run a
/// command, "steer" waits for a second message, "interrupt" waits to be
/// interrupted.
fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        spor::host::set_program(PathBuf::from(env!("CARGO_BIN_EXE_spor-agent-host")));
        let bin = std::env::temp_dir().join(format!("spor-fake-stream-claude-{}", std::process::id()));
        std::fs::create_dir_all(&bin).unwrap();
        let script = r#"#!/bin/sh
printf '%s\n' "$@" > claude-args.txt
say() { printf '{"type":"assistant","message":{"content":[{"type":"text","text":"%s"}]}}\n' "$1"; }
read -r init
read -r first
echo "$init" > init.txt
echo '{"type":"system","subtype":"init","session_id":"sess-stream"}'
case "$first" in
  *mcp-ask*)
    echo '{"type":"control_request","request_id":"req-m","request":{"subtype":"can_use_tool","tool_name":"mcp__spor__ask","input":{"question":"Which database?","options":["Postgres","SQLite"]},"tool_use_id":"tm"}}'
    read -r answer
    echo "$answer" > answer.txt
    say "Using it." ;;
  *todo*)
    echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"TodoWrite","input":{"todos":[{"content":"Read it","activeForm":"Reading it","status":"completed"},{"content":"Change it","activeForm":"Changing it","status":"in_progress"},{"content":"Test it","activeForm":"Testing it","status":"pending"}]}}]}}'
    echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"make test"}}]}}'
    read -r next
    say "Done it." ;;
  *question*)
    echo '{"type":"control_request","request_id":"req-q","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"questions":[{"question":"Keep the old endpoint working?","header":"Endpoint","options":[{"label":"Keep it","description":"Both work for a release"},{"label":"Remove it","description":""}],"multiSelect":false},{"question":"Which docs?","header":"Docs","options":[{"label":"README","description":""},{"label":"CHANGELOG","description":""}],"multiSelect":true}]},"tool_use_id":"tq"}}'
    read -r answer
    echo "$answer" > answer.txt
    say "Keeping it." ;;
  *plan*)
    printf '%s\n' '{"type":"control_request","request_id":"req-p","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":" # Rename greet\n\n1. Rename it\n2. Fix the callers"},"tool_use_id":"tp"}}'
    read -r answer
    echo "$answer" > answer.txt
    case "$answer" in
      *'"behavior":"allow"'*) read -r next; echo "$next" > mode.txt; say "Renaming." ;;
      *) say "Revising the plan." ;;
    esac ;;
  *ask*)
    echo '{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"make test","timeout":5},"tool_use_id":"t1"}}'
    read -r answer
    case "$answer" in
      *'"behavior":"allow"'*) echo "$answer" > answer.txt; say "Ran make test." ;;
      *) echo "$answer" > denied.txt; say "Skipped it." ;;
    esac ;;
  *suggest*)
    echo '{"type":"control_request","request_id":"req-2","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"npm test -- --watch=false"},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"npm test:*"}],"behavior":"allow","destination":"session"}],"tool_use_id":"t2"}}'
    read -r answer
    echo "$answer" > answer.txt
    say "Ran the tests." ;;
  *steer*)
    say "Working on it."
    read -r next
    echo "$next" > steer.txt
    say "Heard you." ;;
  *mode*)
    say "Which mode?"
    read -r next
    echo "$next" > mode.txt ;;
  *interrupt*)
    say "Taking my time."
    read -r next
    case "$next" in
      *'"interrupt"'*) echo '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Interrupted","session_id":"sess-stream"}'; exit 0 ;;
    esac ;;
esac
echo '{"type":"result","subtype":"success","is_error":false,"result":"Done.","total_cost_usd":0.01,"num_turns":1,"duration_ms":10,"session_id":"sess-stream"}'
# Claude exits when stdin closes; the host closes it after the result.
while read -r rest; do :; done
"#;
        let path = bin.join("claude");
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    });
}

fn fixture() -> Fixture {
    setup();
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spor-host-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("app");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@x"),
        ("commit.gpgsign", "false"),
    ] {
        git(&root, &["config", k, v]);
    }
    std::fs::write(root.join("app.txt"), "one\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "start"]);
    let tasks = Tasks::open(&Repo::new(&root)).unwrap();
    Fixture { dir, tasks }
}

fn start(f: &Fixture, prompt: &str) -> (Task, Runs) {
    let task = f
        .tasks
        .create(prompt, "main")
        .and_then(|t| f.tasks.start_branch(&t))
        .unwrap();
    let runs = Runs::of(&f.tasks, &task);
    runs.start(
        &task,
        &AgentKind::Claude,
        prompt,
        None,
        Permissions {
            mode: spor::agent::Mode::AcceptEdits,
        },
    )
    .unwrap();
    (task, runs)
}

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn done(runs: &Runs) -> RunState {
    wait_for("the run to end", || {
        runs.latest().unwrap().state != RunState::Running
    });
    runs.latest().unwrap().state
}

fn texts(runs: &Runs) -> Vec<Entry> {
    runs.transcript()
}

#[test]
fn the_prompt_goes_in_on_stdin() {
    let f = fixture();
    let (task, runs) = start(&f, "just do it");
    assert_eq!(done(&runs), RunState::Finished);
    let args = std::fs::read_to_string(task.worktree.join("claude-args.txt")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert!(
        args.windows(2)
            .any(|w| w == ["--input-format", "stream-json"]),
        "{args:?}"
    );
    assert!(
        args.windows(2)
            .any(|w| w == ["--permission-prompt-tool", "stdio"]),
        "{args:?}"
    );
    assert!(
        args.contains(&"-p") && !args.contains(&"just do it"),
        "{args:?}"
    );
    let init = std::fs::read_to_string(task.worktree.join("init.txt")).unwrap();
    assert!(init.contains(r#""subtype":"initialize""#), "{init}");
    assert!(texts(&runs).contains(&Entry::Prompt("just do it".into())));
    assert!(matches!(
        texts(&runs).last(),
        Some(Entry::Done { ok: true, .. })
    ));
    assert!(!runs.listening(), "the socket goes when the run ends");
    assert!(runs.send("too late").is_err());
}

#[test]
fn a_question_waits_for_an_answer() {
    let f = fixture();
    let (task, runs) = start(&f, "please ask first");
    wait_for("the question", || !runs.asks().is_empty());
    assert!(runs.listening());
    let ask = runs.asks().remove(0);
    assert_eq!(
        (ask.id.as_str(), ask.tool.as_str(), ask.detail.as_str()),
        ("req-1", "Bash", "make test")
    );
    assert_eq!(ask.gist(), "wants to run `make test`");
    // Still waiting: nothing happens without an answer.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(runs.latest().unwrap().state, RunState::Running);

    // spor quits and comes back: everything it needs is on disk.
    let reopened = Runs::of(&f.tasks, &task);
    assert!(reopened.listening());
    assert_eq!(reopened.asks().len(), 1);
    reopened.answer(&ask.id, true, "").unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    let sent = std::fs::read_to_string(task.worktree.join("answer.txt")).unwrap();
    assert!(sent.contains(r#""request_id":"req-1""#), "{sent}");
    assert!(
        sent.contains(r#""updatedInput":{"command":"make test","timeout":5}"#),
        "an allow sends the tool's input back: {sent}"
    );
    let t = texts(&runs);
    assert!(t.contains(&Entry::Ask {
        id: "req-1".into(),
        tool: "Bash".into(),
        detail: "make test".into(),
        session: None,
        kind: AskKind::Permission,
    }));
    assert!(t.contains(&Entry::Answer {
        id: "req-1".into(),
        allow: true,
        message: String::new(),
        always: false,
    }));
    assert!(t.contains(&Entry::Text("Ran make test.".into())));
    assert!(runs.asks().is_empty());
}

#[test]
fn a_question_can_be_refused_with_a_reason() {
    let f = fixture();
    let (task, runs) = start(&f, "please ask first");
    wait_for("the question", || !runs.asks().is_empty());
    runs.answer("req-1", false, "Use cargo test instead")
        .unwrap();
    // A second answer to the same question is refused.
    assert!(runs.answer("req-1", true, "").is_err());
    assert_eq!(done(&runs), RunState::Finished);
    let sent = std::fs::read_to_string(task.worktree.join("denied.txt")).unwrap();
    assert!(
        sent.contains(r#""behavior":"deny","message":"Use cargo test instead""#),
        "{sent}"
    );
    assert!(texts(&runs).contains(&Entry::Answer {
        id: "req-1".into(),
        allow: false,
        message: "Use cargo test instead".into(),
        always: false,
    }));
}

#[test]
fn a_question_can_be_allowed_for_the_session() {
    let f = fixture();
    let (task, runs) = start(&f, "please suggest a rule");
    wait_for("the question", || !runs.asks().is_empty());
    let ask = runs.asks().remove(0);
    assert_eq!(ask.session.as_deref(), Some("`npm test` commands"));
    runs.allow_for_session(&ask.id).unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    let sent = std::fs::read_to_string(task.worktree.join("answer.txt")).unwrap();
    // The suggested rules go back with the allow.
    let reply = spor::json::Json::parse(sent.trim()).unwrap();
    let response = reply.get("response").get("response");
    let update = &response.get("updatedPermissions").array()[0];
    assert_eq!(update.get("type").str(), Some("addRules"), "{sent}");
    assert_eq!(update.get("destination").str(), Some("session"));
    let rule = &update.get("rules").array()[0];
    assert_eq!(rule.get("toolName").str(), Some("Bash"));
    assert_eq!(rule.get("ruleContent").str(), Some("npm test:*"));
    assert!(sent.contains(r#""updatedInput":{"command":"npm test -- --watch=false"}"#));
    assert!(texts(&runs).contains(&Entry::Answer {
        id: ask.id.clone(),
        allow: true,
        message: String::new(),
        always: true,
    }));
}

#[test]
fn without_suggestions_it_can_only_be_allowed_once() {
    let f = fixture();
    let (_task, runs) = start(&f, "please ask first");
    wait_for("the question", || !runs.asks().is_empty());
    assert_eq!(runs.asks()[0].session, None);
    let err = runs.allow_for_session("req-1").unwrap_err();
    assert_eq!(err, "This can only be allowed once");
    // Still waiting, and still answerable.
    runs.answer("req-1", true, "").unwrap();
    assert_eq!(done(&runs), RunState::Finished);
}

#[test]
fn a_running_agent_can_be_steered() {
    let f = fixture();
    let (task, runs) = start(&f, "steer me");
    wait_for("it to start", || {
        texts(&runs).contains(&Entry::Text("Working on it.".into()))
    });
    runs.send("Also update the README").unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    let got = std::fs::read_to_string(task.worktree.join("steer.txt")).unwrap();
    assert!(
        got.contains(r#""content":"Also update the README""#),
        "{got}"
    );
    let t = texts(&runs);
    assert!(t.contains(&Entry::Steer("Also update the README".into())));
    assert!(t.contains(&Entry::Text("Heard you.".into())));
}

#[test]
fn finish_interrupts_rather_than_kills() {
    let f = fixture();
    let (_task, runs) = start(&f, "interrupt me");
    wait_for("it to start", || {
        texts(&runs).contains(&Entry::Text("Taking my time.".into()))
    });
    runs.finish(Duration::from_secs(5)).unwrap();
    // Interrupted, not killed: it ended on its own, with a result.
    assert_eq!(runs.latest().unwrap().state, RunState::Finished);
    assert!(matches!(
        texts(&runs).last(),
        Some(Entry::Done { ok: false, .. })
    ));
}

#[test]
fn interrupt_ends_the_turn() {
    let f = fixture();
    let (_task, runs) = start(&f, "interrupt me");
    wait_for("it to start", || {
        texts(&runs).contains(&Entry::Text("Taking my time.".into()))
    });
    runs.interrupt().unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    assert!(matches!(
        texts(&runs).last(),
        Some(Entry::Done { ok: false, .. })
    ));
    // The session survives, for a follow-up.
    assert_eq!(runs.session().as_deref(), Some("sess-stream"));
}

#[test]
fn the_mode_can_change_while_it_works() {
    let f = fixture();
    let task = f
        .tasks
        .create("mode please", "main")
        .and_then(|t| f.tasks.start_branch(&t))
        .unwrap();
    let runs = Runs::of(&f.tasks, &task);
    runs.start(
        &task,
        &AgentKind::Claude,
        "mode please",
        None,
        Permissions {
            mode: Mode::Supervised,
        },
    )
    .unwrap();
    wait_for("it to start", || {
        texts(&runs).contains(&Entry::Text("Which mode?".into()))
    });
    let args = std::fs::read_to_string(task.worktree.join("claude-args.txt")).unwrap();
    assert!(
        args.contains("--permission-mode\nmanual\n"),
        "Supervised is Claude's manual mode: {args}"
    );
    assert_eq!(runs.mode(0), Some(Mode::Supervised));
    runs.set_mode(Mode::AcceptEdits).unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    let sent = std::fs::read_to_string(task.worktree.join("mode.txt")).unwrap();
    assert!(
        sent.contains(r#""subtype":"set_permission_mode","mode":"acceptEdits""#),
        "{sent}"
    );
    assert_eq!(runs.mode(0), Some(Mode::AcceptEdits));
}

#[test]
fn a_question_with_options_is_answered_with_picks() {
    let f = fixture();
    let (task, runs) = start(&f, "a question for you");
    wait_for("the question", || !runs.asks().is_empty());
    let ask = runs.asks().remove(0);
    assert_eq!(ask.tool, "AskUserQuestion");
    assert_eq!(ask.detail, "Keep the old endpoint working?");
    assert_eq!(ask.gist(), "asks: Keep the old endpoint working?");
    assert_eq!(
        ask.session, None,
        "a question is never allowed for the session"
    );
    let AskKind::Questions(qs) = &ask.kind else {
        panic!("not questions: {:?}", ask.kind)
    };
    assert_eq!(qs.len(), 2);
    assert_eq!(qs[0].header, "Endpoint");
    assert_eq!(qs[0].options[0].label, "Keep it");
    assert_eq!(qs[0].options[0].description, "Both work for a release");
    assert!(!qs[0].multi && qs[1].multi);

    assert!(runs.answer_questions(&ask.id, &[vec![], vec![]]).is_err());
    runs.answer_questions(
        &ask.id,
        &[
            vec!["Keep it".into()],
            vec!["README".into(), "CHANGELOG".into()],
        ],
    )
    .unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    let sent = std::fs::read_to_string(task.worktree.join("answer.txt")).unwrap();
    assert!(sent.contains(r#""behavior":"allow""#), "{sent}");
    assert!(
        sent.contains(r#""answers":{"Keep the old endpoint working?":"Keep it","Which docs?":"README, CHANGELOG"}"#),
        "the answers go back in the tool's input: {sent}"
    );
    assert!(
        sent.contains(r#""questions":[{"header":"Endpoint""#),
        "with the questions it asked: {sent}"
    );
    assert!(texts(&runs).contains(&Entry::Answer {
        id: "req-q".into(),
        allow: true,
        message: "Keep it · README, CHANGELOG".into(),
        always: false,
    }));
    assert!(texts(&runs).contains(&Entry::Text("Keeping it.".into())));
}

#[test]
fn a_plan_is_approved_and_the_mode_changes() {
    let f = fixture();
    let (task, runs) = start(&f, "make a plan");
    wait_for("the plan", || !runs.asks().is_empty());
    let ask = runs.asks().remove(0);
    assert_eq!(ask.detail, "Rename greet");
    assert_eq!(ask.gist(), "has a plan for you to review: Rename greet");
    assert_eq!(
        ask.kind,
        AskKind::Plan(" # Rename greet\n\n1. Rename it\n2. Fix the callers".into())
    );
    assert!(runs.approve_plan(&ask.id, Mode::Plan).is_err());
    runs.approve_plan(&ask.id, Mode::AcceptEdits).unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    let sent = std::fs::read_to_string(task.worktree.join("answer.txt")).unwrap();
    assert!(sent.contains(r#""behavior":"allow""#), "{sent}");
    let mode = std::fs::read_to_string(task.worktree.join("mode.txt")).unwrap();
    assert!(
        mode.contains(r#""subtype":"set_permission_mode","mode":"acceptEdits""#),
        "{mode}"
    );
    assert_eq!(runs.mode(0), Some(Mode::AcceptEdits));
    assert!(texts(&runs).contains(&Entry::Text("Renaming.".into())));
}

#[test]
fn a_plan_can_be_sent_back_with_comments() {
    let f = fixture();
    let (task, runs) = start(&f, "make a plan");
    wait_for("the plan", || !runs.asks().is_empty());
    runs.answer("req-p", false, "Keep the old name as an alias")
        .unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    let sent = std::fs::read_to_string(task.worktree.join("answer.txt")).unwrap();
    assert!(
        sent.contains(r#""behavior":"deny","message":"Keep the old name as an alias""#),
        "{sent}"
    );
    assert!(texts(&runs).contains(&Entry::Text("Revising the plan.".into())));
}

#[test]
fn progress_says_how_far_through_its_plan_it_is() {
    let f = fixture();
    let (_task, runs) = start(&f, "a todo list");
    wait_for("its plan", || {
        runs.progress()
            .is_some_and(|p| p.total == 3 && p.tool.is_some())
    });
    assert_eq!(
        runs.progress().unwrap(),
        Progress {
            done: 1,
            total: 3,
            step: Some("Changing it".into()),
            tool: Some(("Bash".into(), "make test".into())),
        }
    );
    runs.send("go on").unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    assert_eq!(runs.progress(), None, "nothing is running");
}

#[test]
fn spors_mcp_server_is_given_to_the_run_and_its_ask_reaches_the_inbox() {
    let f = fixture();
    let (task, runs) = start(&f, "mcp-ask");
    wait_for("the question", || !runs.asks().is_empty());
    let ask = runs.asks().remove(0);
    assert_eq!(ask.tool, "mcp__spor__ask");
    assert_eq!(ask.gist(), "asks: Which database?");
    let AskKind::Questions(qs) = &ask.kind else {
        panic!("not a question: {:?}", ask.kind)
    };
    assert_eq!(qs[0].options.len(), 2);
    assert_eq!(qs[0].options[1].label, "SQLite");
    runs.answer_questions(&ask.id, &[vec!["SQLite".into()]])
        .unwrap();
    assert_eq!(done(&runs), RunState::Finished);
    // The answer goes back in the call's input, where the server finds it.
    let sent = std::fs::read_to_string(task.worktree.join("answer.txt")).unwrap();
    assert!(sent.contains(r#""answer":"SQLite""#), "{sent}");

    let args = std::fs::read_to_string(task.worktree.join("claude-args.txt")).unwrap();
    let lines: Vec<&str> = args.lines().collect();
    let at = lines.iter().position(|l| *l == "--mcp-config").unwrap();
    let config = std::fs::read_to_string(lines[at + 1]).unwrap();
    assert!(
        config.contains(r#""spor""#) && config.contains("__spor-mcp"),
        "{config}"
    );
    let allowed = lines[lines.iter().position(|l| *l == "--allowedTools").unwrap() + 1];
    assert!(allowed.contains("mcp__spor__why"), "{allowed}");
    assert!(
        !allowed.contains("mcp__spor__ask"),
        "asking is never pre-approved"
    );
}
