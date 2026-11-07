# Spor for parallel coding agents

A design for making spor the place where you run, watch, review and land
the work of several coding agents at once. **Phases 1 and 2 are built**,
with most of 3 and the conflict radar from 4 (see the README); the rest is
the plan.

## The problem

Running one coding agent is easy. Running four is a mess:

- **They collide.** Two agents in one working tree overwrite each other's
  files and fight over the index. People work around this with clones or
  worktrees by hand, and then lose track of which is which.
- **You can't see them.** Each one lives in its own terminal. Which is still
  working, which is waiting for permission, which finished an hour ago,
  which went off the rails?
- **Review is the bottleneck.** Agents write code faster than anyone can
  read it. Reading a diff, saying "not like that, like this", and getting a
  second attempt is slow when the diff, the conversation and the terminal
  are in three places.
- **Landing is manual.** Rebasing each branch on a moving main, resolving
  conflicts between two agents' work, running the tests after each merge.
- **The why gets lost.** A month later, `git blame` points at a commit an
  agent made. The conversation that explains it is gone.

None of these are about writing code. They're about branches, diffs,
merges and history, which is what spor already shows. Spor shouldn't try to
be an agent. It should be the control tower the agents report to.

## The idea in one paragraph

A **task** is a prompt plus the branch and worktree it runs in. Starting one
creates `agent/<slug>` from a base branch, checks it out in its own
worktree, and launches an agent there. From then on spor treats the branch
like any other, so its commits show up live in History as their own lane.
It also tracks what git can't tell it: whether the agent is running,
waiting for you, or done. When the agent finishes, the task opens like a
pull request does today: the diff against its base, the agent's
transcript, checks, and buttons to send feedback, merge, open a real PR, or
throw it away. All of these can be undone, like everything else in spor.

## What's already in place

The groundwork this needs is now in the core (`src/git.rs`):

- **Every git call runs in its own repository.** `git::Repo` carries a
  root, so a worktree is just another `Repo`. A job started for one task
  can't land in another, which was impossible while git ran in the
  process's working directory.
- **Worktrees.** `worktrees()`, `add_worktree(path, branch, base)` and
  `remove_worktree(path, force)`, and the branch list shows linked
  worktrees.
- **Landing.** `merge`, `rebase`, `cherry_pick` and `rebase_interactive`
  return an `Outcome`: done, or stopped on conflicts. `operation()`,
  `conflicts()`, `continue_operation()` and `abort_operation()` handle the
  stopped case, and the app has a banner and a resolver for it.
- **Undo** for rewrites (`Undo::Reset`), deleted branches and applied
  patches. Discarding an agent's work can be taken back the same way.
- **The PR page and the Actions views**, which a task page can borrow from.

## Design

### Tasks

```rust
struct Task {
    id: String,             // short slug, also the branch suffix
    prompt: String,
    base: String,           // branch it started from, e.g. "main"
    branch: String,         // "agent/<id>"
    worktree: PathBuf,      // <repo>.spor/<id>, beside the repository
    agent: AgentKind,       // which adapter runs it
    session: Option<String>,// the agent's own session id, to resume it
    state: TaskState,
    started: i64,
    landed: Option<String>, // the commit it was merged as
}

enum TaskState {
    Running,
    NeedsYou(Ask),          // a permission request or a question
    Ready,                  // finished, waiting for review
    Failed(String),
    Landed,
    Discarded,
}
```

Tasks are saved per repository in `.git/spor/tasks/*.json`. That keeps
them out of the working tree and out of commits, and every worktree of the
repository sees the same list.

Worktrees go beside the repository (`../spor.spor/fix-login/`), not inside
`.git`. Editors, language servers and build tools treat them as ordinary
checkouts, and "Open in editor" just works.

### Agents are adapters

Spor runs agents rather than reimplementing them. Each adapter answers four
questions: how to start the agent, how to tell its state, how to send it a
follow-up, and how to answer its questions.

| | Generic command | Claude Code |
|---|---|---|
| Start | a command template, `{prompt}` filled in, run in the worktree | `claude -p "{prompt}" --output-format stream-json` |
| State | process alive or exited, plus git activity | the stream's events: tool calls, messages, the final result with cost |
| Follow-up | start the command again with the feedback | `claude -p "{feedback}" --resume {session}` |
| Questions | none, so give the agent a permission mode that never asks | a hook forwards permission requests to spor (below) |

The generic adapter is what makes this useful on day one with any agent,
including ones run by hand in the worktree. With no adapter at all, a task
still has a branch, a worktree and a diff. Spor just can't tell "thinking"
from "stuck".

**Answering an agent's questions** follows the same pattern as `askpass.rs`,
which already brings ssh's passphrase prompts into the app. ssh runs a
helper, the helper hands the question to the GUI over a private channel and
waits, the GUI shows a dialog, and the answer goes back. A Claude Code
`PreToolUse` hook can be the same kind of helper. The task goes to
*Needs you*, a notification names it ("fix-login wants to run `npm
install`"), and Allow / Deny answer it. The rest of the time agents run
with the permission mode the user picked when starting the task.

Two other questions come the same way, and get their own answers:

- **`AskUserQuestion`** (#102): the agent asks one or more questions, each
  with options. They show as buttons (checkboxes when several can be
  picked), with room for an answer of your own, and the picks go back as
  the tool's input (`answers`, keyed by question), so the agent carries on
  in the same turn. The Inbox and the notification say the question.
- **Plan first** (#101), a permission mode: the agent reads and plans, and
  changes nothing until its `ExitPlanMode` is answered. The plan shows in
  the composer; clicking a line quotes it into a comment. *Approve plan*
  lets it start, in the mode picked beside the button; sending a comment
  sends it back to plan again. Each version of the plan stays in the
  transcript.

### What you see

- **Work** in the sidebar (the rail's last section, after Pulls and Actions;
  it was the Agents panel until #81). Tasks are listed beside your own
  branches and everyone's open pull requests. One row
  per task: its state, how long it's been running, files changed and +/−,
  and checks once they've run. *Needs you* rows go to the top, highlighted.
  The workspace's repository sidebar gets a count, the way it shows commits
  to pull today.
- **History** shows each agent branch as its own lane with a robot marker.
  The poll loop already reloads when refs move, so commits appear as the
  agent makes them.
- **The task page** (in the center, like a PR or a run) has:
  - the prompt and the agent's transcript, streamed, with tool calls folded
  - the diff against the base, with the Changes view's All / Unstaged /
    Staged views, since an agent's uncommitted work counts too
  - checks: a per-repository test command run in the worktree, or the
    branch's Actions once it's pushed
  - **Send Feedback**: a box for a follow-up. Comments added on diff lines
    (the diff component already supports line annotations) are gathered
    into it as `file:line — comment`, so review reads like a PR review and
    goes back as one message
  - **Land**: squash or merge into the base (rebasing first when the base
    moved), or **Open Pull Request**, or **Discard**, which removes the
    worktree and branch and can be undone while spor is open

### Landing many at once

- **Collision radar.** In the background, spor does a trial merge of each
  active agent branch against the base and against every other agent
  branch, with `git merge-tree --write-tree`, which touches no working
  tree. The panel then says "conflicts with *fix-login* in
  `src/auth.rs`" before you've merged anything. This is the earlier
  "warn about conflicts before they happen" idea, aimed at agents first.
- **Keep up with the base.** When main moves, *Rebase All* rebases each
  idle task's branch in its worktree. One that conflicts doesn't get the
  resolver. It goes back to its agent as a follow-up: "rebasing onto main
  conflicted in these files; resolve them and continue." Agents are good at
  this, and it's the step people least want to do.
- **Don't undo the base.** A merge that doesn't conflict can still take
  work back out: an agent that rebases or merges the base in, then writes
  files back from the copy it started with, reverts everything that came
  in meanwhile (that's how landing #81 undid the Inbox and five other
  changes). `Tasks::undoes` finds it: the lines the trial merge would drop
  from the base, blamed on the base, that came from commits made after the
  branch was created (its reflog's first entry), leaving out lines that
  moved elsewhere. The task's card says "Would undo 109 lines that reached
  main after it started, in App.tsx…", with *Ask agent to put them back*,
  and landing or opening a pull request asks first.
- **Land in order.** Pick the tasks that are ready and spor lands them one
  at a time: rebase, run the checks, merge, next. It stops at the first
  failure and says which. It's a small local merge queue.

### Remembering why

Each task keeps its transcript (in `.git/spor/tasks/<id>/`) and the commits
it produced. When a task lands, the squash commit's message gets a
`Spor-Task: <id>` trailer. Commit details in History, and later blame, can
then open the conversation behind a line. "Why does this exist?" gets an
answer from the agent's own reasoning, not just a name and a date.

### Safety

- Agents never push. Publishing is always a human click: Open Pull Request
  or Land. The Claude Code adapter starts agents with `git push` denied in
  their permissions.
- Worktrees share the repository's refs, so an agent could still move or
  delete another branch. Spor notices when a task's branch moves in a way
  its agent didn't (a force-move, a deleted branch) and flags the task
  rather than trusting it.
- Discarding a task is undoable while spor is open (the branch tip is kept,
  like a deleted branch today). Removing a worktree with uncommitted
  changes asks first, as it already does.

## Plan

Each phase is useful on its own.

1. **Tasks without agents** — *done*. New Task… creates a branch and a worktree from
   a base. The Agents panel lists tasks with state taken from git (dirty,
   commits ahead, last change). A task page shows the diff against the base,
   with Land, Open Pull Request and Discard. That's already a better way to
   run agents by hand: start a task, run your agent in its folder, review
   and land in spor.
2. **Launch and watch** — *done*. The generic command adapter and the Claude Code
   adapter: spawn in the worktree, stream the transcript, states from the
   process and the stream, a notification when a task finishes or fails.
3. **The review loop** — *done, except the permission bridge*. Send
   Feedback with diff-line comments, resume the agent's session. Still to
   do: the permission bridge for *Needs you*; for now the task's
   permissions are set when it starts.
4. **Many at once** — *collision radar done*, with *Ask Agent to Rebase*
   for conflicts with the base. Still to do: Rebase All with conflicts handed back
   to the agent, and landing in order with checks.

   **From issues** (#47): a task can start from a GitHub issue (the
   issue is its prompt), opens its pull request with `Closes #n`, follows
   that PR's state, sends its reviews back to the agent, and is cleaned up
   when the PR merges (#24). Built in the Tauri app.
5. **Remote agents** (#44). Agents running in the cloud already work on branches
   and open pull requests. A task can follow a remote branch or PR instead
   of a local worktree, so cloud and local agents share one panel.

**Spor as an MCP server** (#106): each Claude Code run gets a stdio MCP
server (`--mcp-config` names a file in the run directory; the server is
the app's executable started with `__spor-mcp`). Tools: `ask` (a question
for the Inbox, answered inside the turn), `report_progress`,
`other_tasks` (with the files you share), `pr_comments` (unresolved
threads), `ci_log` (failing jobs, folded to the failure) and `why` (blame
a line; if the commit has a `Spor-Task` trailer, that task's prompt and
what its agent did to the file). Everything but `ask` is pre-approved and
read-only, apart from `report_progress` writing to the run's log. Still to
do: `checks()` (#43). `ask` isn't asked in `bypassPermissions` mode, where
Claude Code doesn't prompt for any tool.

## Decisions to make

- **Which agents first.** Claude Code has a structured stream and hooks,
  so it can do everything above. The generic adapter covers the rest with
  less detail. Codex and others could get adapters later.
- **Headless or a terminal.** Phase 2 is headless (structured output and
  logs). Some people want to watch the agent's own terminal UI. That needs
  a terminal emulator in the app (xterm.js in the web view, over a pty in
  the backend), which is a project of its own.
- **Squash or merge by default** when landing. Squash keeps main readable
  and puts the trailer in one place. Merge keeps the agent's individual
  commits.
- **Where task state lives.** `.git/spor/` is per-clone and invisible to
  collaborators. Sharing tasks across machines would mean a server, which
  is out of scope for now.

## What the agent is told (#105)

Each run gets a short generated block: its folder, branch and base, that spor publishes (so it shouldn't push), the issue it is for, where attached images live, whether `.spor/checks` exists, and the other active tasks with the files they have changed so far. Claude Code gets it with `--append-system-prompt`; a custom command gets it in `$SPOR_CONTEXT`. A committed `.spor/context.md` is appended. The text is kept beside the run as `run-N.context`, and every run (follow-ups too) builds it fresh.

## Reading what commands printed (#97)

`parse_claude` pairs each `Bash` call with its result by `tool_use_id` and keeps the end of the output (8,000 characters; the whole text stays in `run-N.log`) and whether the tool reported an error. In the Agent tab, an unfolded command opens to its output. Test and build commands (`cargo test`, `pnpm test`, `pytest`, `go test`, `tsc`, `cargo build` and the like) are recognised, and the last outcome of each shows on the run's fold and the task's details card: "Tests ✓ · Build ✓", "Tests ✗ (last run)", or "Tests not run" for a run that changed files.

## Subagents and thinking (#99)

Events that carry a `parent_tool_use_id` are kept as `Entry::Nested` under the `Task`/`Agent` call that started them, and that call also gives an `Entry::Subagent` (its kind, description and prompt). The subagent's final report is the call's result. In the Agent tab a subagent is a card that opens into its own stretch of work; the main agent's summary still counts it as one action, and the live line says "3 subagents working". A subagent's plan and tool calls no longer replace the main agent's. `thinking` blocks become `Entry::Thinking`, shown folded as "Thought for a moment" where they happened, and never in summaries.

## Starting fresh when the context is full (#98)

When the latest Claude run's context is 80% full or more, the composer offers *Start fresh with a summary*. The follow-up then starts a new session (no `--resume`) whose prompt is the task's prompt, the latest run's debrief as a hand-over (what changed, what was checked, risks, open questions, work not done) and the message. Without a debrief it says to look at the folder's changes first. The agent and model are the last run's.

## Rules worth writing down (#107)

*Look for rules to write down* (at the foot of the Work sidebar) collects what people told the agents across the repository's tasks: follow-up messages (line comments go in them), steering, and the reason given for a denial. Claude Code, read-only at low effort, groups them into candidate lines for the repository's rules file. Only rules that at least two different tasks asked for are shown, each with an editable line, *Add* and *Not this one*. *Add* appends a bullet to `CLAUDE.md` (or `AGENTS.md` if that is what the repository has) in the main checkout, as an uncommitted change; added and declined rules are remembered in `.git/spor/rules.known` and not suggested again. Permission patterns (a command allowed in several tasks) aren't suggested yet.
