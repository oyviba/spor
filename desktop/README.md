# Spor desktop (Tauri)

Spor's app as a web UI on Tauri, over the same Rust core (`spor::git`,
`spor::graph`, `spor::github`, `spor::tasks`, `spor::agent`) that the
terminal and GPUI builds use. It began as a trial of the Agents feature and
now has six sections. Each is built from the same four pieces: a list
in the sidebar, a page, a details card on the right, and a bar or composer
at the bottom.

| Section | ⌘ | Sidebar | Page | Details card | Bottom |
|---|---|---|---|---|---|
| **Changes** | 1 | Conflicts, staged and unstaged files | The file's diff: click lines, shift-click to extend, then stage, unstage or discard them | Branch and upstream, Fetch / Pull / Push, stashes | Commit box (⌘↩), amend |
| **History** | 2 | Search, branches (local, agent, remote) | The commit graph, and a commit's diff | The commit: message, parents, Branch here, Cherry-pick, Revert | |
| **Issues** | 3 | Assigned / Yours / Open / Closed | The issue and its comments | Labels, assignees, its agent task and PR, close or reopen | *Start agent task*: the issue as the prompt (editable), the base and the agent |
| **Pull requests** | 4 | Review / Yours / Open / Closed | Conversation · Files · Checks | Branches, checks, conflicts, linked agent task, merge, check out, close | Comment / Approve, with line comments from Files |
| **Actions** | 5 | Runs, by workflow and branch | Jobs, and the log folded by step (failed steps open) | Run, commit, linked agent task, re-run, cancel | |
| **Agents** | 6 | Tasks, with Working / Settled shelves | Conversation · Changes | Folder, branch, conflicts, its issue and PR, open a PR, land, discard | Composer |

What one app makes possible:

- **One undo stack.** Every change (stage, discard, commit, amend, pull,
  branch, switch, merge, cherry-pick, revert, land) records how to take it
  back. ⌘Z, or a toast's Undo, takes back the latest from any section.
- **Links between sections.** A PR's failed check opens in Actions. A failed
  run on an agent's branch has *Send failure to agent*, which sends the
  failing log. A task's branch opens in History, and a run's commit too.
- **One conflict banner.** A merge or rebase that stops shows on every
  section, with *Resolve in Changes*, *Continue* and *Abort*.
- **From issue to merged PR** (#47, #24). *Start agent task* on an issue
  gives the agent the issue and its comments, on `agent/<number>-<slug>`.
  *Open pull request* (or a draft) on the task pushes its branch and opens
  a PR whose description says `Closes #n`; landing locally puts `Closes #n`
  in the commit instead. The task then shows its PR's state, offers *Push*
  when the agent has commits the PR doesn't, and *Send review to agent*
  (on the task, or on the PR page) sends reviews and line comments made
  since the agent last ran, as `file:line — comment`. When the PR merges, a
  notification offers *Clean up*: the task is marked landed and its folder
  and branch go (⌘Z brings them back). PRs opened from a task's branch some
  other way are linked too.
- **Stash and switch.** A switch that uncommitted changes would block says
  so and offers to stash them, as the GPUI app does.

The graph's lanes are laid out by `spor::graph::assign_lanes`, and the
segments for each row are worked out in Rust (`src-tauri/src/history.rs`,
mirroring `src/gui/graph.rs`). The UI only draws them as SVG, and only for
the rows in view.

## Run it

You need Rust, Node 20+ and pnpm, plus the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS
(on Linux, `libwebkit2gtk-4.1-dev` and friends). Pull requests and Actions
need the GitHub CLI (`gh auth login`).

```sh
cd desktop
pnpm install
SPOR_REPO=/path/to/a/repo pnpm tauri dev   # or start it inside a repo; click the repo name to switch
```

**UI only.** `pnpm dev` serves the UI at http://localhost:1420 with no Rust:
a mock backend (`src/mock.ts`). It plays back answers recorded from the real
backend on a demo repository (`src/fixtures.json`, made by
`e2e/record-fixtures.mjs`), and has a few agent tasks, one of which works and
finishes while you watch.

## Tests

- `pnpm test`: the transcript folding and summaries.
- `e2e/run.mjs` (agent tasks), `e2e/setup.mjs` (setup scripts) and
  `e2e/sections.mjs` (Changes, History, a conflicting merge, stashes, pull
  requests and Actions) drive the real app through WebDriver and check the
  repository with git at every step. CI runs all three on Linux under Xvfb.

  They need no real agent or GitHub: `e2e/bin` has stand-ins. `claude` is a
  Python script that speaks Claude Code's stream-json both ways (it edits,
  asks before running the tests, takes steering), and `gh` answers from
  `e2e/fixtures/gh.json`, running spor's own `--jq` expressions through
  `jq` and recording what's done in `$FAKE_GH_STATE`. Spor's parsers treat
  them exactly like the real tools. `e2e/make-repos.sh` builds the scratch
  repositories each suite expects, and `e2e/run-all.sh` puts it together:

  ```sh
  cargo install tauri-driver   # Linux also needs WebKitWebDriver (webkit2gtk-driver)
  pnpm build && (cd src-tauri && cargo build --features tauri/custom-protocol)
  xvfb-run -a e2e/run-all.sh src-tauri/target/debug/spor-desktop e2e-shots   # or: … e2e-shots sections
  ```

  `custom-protocol` builds the UI into the app; without it a debug build
  loads it from `pnpm dev`. Screenshots of each step land in `e2e-shots/`.

## Layout

```
src-tauri/src/
  lib.rs        setup, repository state for the chrome, undo, open_url
  undo.rs       the undo stack
  history.rs    graph (lanes → segments), commits, branches, switch, merge, cherry-pick, revert
  changes.rs    status, diffs, staging lines, discard, commit, sync, stashes, conflicts
  github.rs     issues, pull requests and Actions, through spor's gh wrappers
  agents.rs     agent tasks, and their pull requests
src/
  App.tsx       the shell: rail, sections, conflict banner, toasts, ⌘1–6, ⌘Z
  shared/       DiffView, details card, status pills, CI status, icons, hooks
  sections/     changes/ history/ issues/ pulls/ actions/ agents/
```

`src-tauri` is its own Cargo workspace, so the root crate and its
single-dependency terminal build don't change.

## Not here yet

From the GPUI app: interactive rebase, the theme editor, the keymap editor,
workspaces (several repositories open), tags, commit search by path, blame,
and the askpass bridge. Without that bridge, a fetch or push that needs a
password fails with git's message instead of asking.

From the agent issues: permission prompts and steering (#20). For now a
follow-up typed while an agent works is queued and sent when the run ends.

Polling, not events: the window polls the repository's state every 2 s
(cheap: refs and the index's mtime) and each section reloads when that
changes. A file watcher would be better. The rules for starting an agent
are copied from `src/gui/agents.rs`; they belong in the core.
