# Spor desktop (Tauri prototype)

A trial of spor's **agent tasks** in a Tauri app: a web UI over the same Rust
core (`spor::tasks`, `spor::agent`, `spor::git`) that the terminal and GPUI
builds use. It exists to answer one question: is a web UI on Tauri a better
home for spor's app than GPUI? Only the Agents feature is here; the rest of
spor stays in the GPUI app for now.

It follows the interface issues filed from studying T3 Code:

- **#30 Task list**: a stable active list (activity never reorders it),
  *Working* and *Settled* shelves at the bottom, and one status per row
  (*Working 42s*, *Failed*, *Ready*), with Failed and Ready shown only until
  you've looked.
- **#27 Conversation**: prompts as bubbles, and a finished run folded behind
  "Worked for 2m 05s" with only the agent's final message showing. A run of
  tool calls reads as one sentence ("Ran 2 commands and edited 3 files, and
  performed 4 other actions"). Runs that failed, were stopped or had a
  refused tool call never fold.
- **#28 Composer**: one button that follows the task's state (Start, Send,
  Queue while the agent works, Stop when empty), review comments as chips,
  and the agent and permission settings.
- **#29 Details card**: folder, branch, conflicts with the base and *Ask agent
  to rebase*, changes, runs and cost, and Land / Discard (⌘Z undoes).
- **Changes**: the task's diff against its base. Click `+` on a line to
  comment; comments go out with your next message.
- **#31 (part)**: in New Task, ⌘↩ starts the task and keeps the box open for
  the next one.

## Run it

You need Rust, Node 20+ and pnpm, plus the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS
(on Linux, `libwebkit2gtk-4.1-dev` and friends).

```sh
cd desktop
pnpm install
SPOR_REPO=/path/to/a/repo pnpm tauri dev   # or start it inside a repo; click the repo name to switch
```

**UI only.** `pnpm dev` serves the UI at http://localhost:1420 with a mock
backend (`src/mock.ts`): a few tasks, one of which works and finishes while you
watch. No Rust needed.

## Tests

- `pnpm test`: the transcript folding and summaries (`src/transcript.test.ts`).
- `e2e/run.mjs`: the real app through WebDriver, from creating a task to
  landing it and undoing the land, checking the repository with git at each
  step. Needs `tauri-driver` (`cargo install tauri-driver`) and, on Linux,
  `WebKitWebDriver` (`webkit2gtk-driver`). Point it at a **throwaway**
  repository, because it lands:

  ```sh
  tauri-driver &
  SPOR_REPO=/tmp/scratch-repo node e2e/run.mjs src-tauri/target/debug/spor-desktop shots/
  ```

  To run without a real (billed) agent, put a stand-in `claude` first on
  `PATH` that prints Claude Code's `stream-json` events. The parser treats it
  exactly like the real one.

## Layout

| | |
|---|---|
| `src-tauri/src/lib.rs` | Tauri commands: spor's core as JSON, plus the GPUI app's rules for picking the agent, wrapping follow-ups and keeping one undo |
| `src/api.ts` | Typed calls; uses `mock.ts` outside Tauri |
| `src/transcript.ts` | Runs, folds and tool summaries (pure, tested) |
| `src/status.ts` | The one status a task shows |
| `src/components/` | Sidebar, TaskPage (Conversation, ChangesView, Composer, Inspector), NewTask |

`src-tauri` is its own Cargo workspace, so the root crate and its
single-dependency terminal build don't change.

## Not here yet

- **Permission prompts** and **steering** need the long-lived agent session
  from #20. Until then a follow-up typed while the agent works is
  **queued** in the UI and sent when the run ends.
- **Polling, not events.** The list refreshes every 1.5 s and the open task
  every 1 s while it runs. A file watcher on `.git/spor/tasks` would be better.
- **Shared logic.** The rules for starting an agent and wrapping feedback are
  copied from `src/gui/agents.rs`. If the trial is kept, they belong in the
  core.
- **No conflict radar between tasks**, only against the base. No PR button, no
  per-run diffs (#19).
