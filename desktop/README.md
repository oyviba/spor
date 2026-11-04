# Spor desktop (Tauri)

Spor's app as a web UI on Tauri, over the same Rust core (`spor::git`,
`spor::graph`, `spor::github`, `spor::tasks`, `spor::agent`) that the
terminal and GPUI builds use. It began as a trial of the Agents feature and
now has all five sections. Each is built from the same four pieces: a list
in the sidebar, a page, a details card on the right, and a bar or composer
at the bottom.

| Section | ⌘ | Sidebar | Page | Details card | Bottom |
|---|---|---|---|---|---|
| **Changes** | 1 | Conflicts, staged and unstaged files | The file's diff: click lines, shift-click to extend, then stage, unstage or discard them | Branch and upstream, Fetch / Pull / Push, stashes | Commit box (⌘↩), amend |
| **History** | 2 | Search, branches (local, agent, remote) | The commit graph, and a commit's diff | The commit: message, parents, Branch here, Cherry-pick, Revert | |
| **Pull requests** | 3 | Review / Yours / Open / Closed | Conversation · Files · Checks | Branches, checks, conflicts, linked agent task, merge, check out, close | Comment / Approve, with line comments from Files |
| **Actions** | 4 | Runs, by workflow and branch | Jobs, and the log folded by step (failed steps open) | Run, commit, linked agent task, re-run, cancel | |
| **Agents** | 5 | Tasks, with Working / Settled shelves | Conversation · Changes | Folder, branch, conflicts, land, discard | Composer |

What one app makes possible:

- **One undo stack.** Every change (stage, discard, commit, amend, pull,
  branch, switch, merge, cherry-pick, revert, land) records how to take it
  back. ⌘Z, or a toast's Undo, takes back the latest from any section.
- **Links between sections.** A PR's failed check opens in Actions. A failed
  run on an agent's branch has *Send failure to agent*, which sends the
  failing log. A task's branch opens in History, and a run's commit too.
- **One conflict banner.** A merge or rebase that stops shows on every
  section, with *Resolve in Changes*, *Continue* and *Abort*.
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
- `e2e/run.mjs` (agent tasks) and `e2e/sections.mjs` (Changes, History, a
  conflicting merge, stashes, pull requests and Actions) drive the real app
  through WebDriver and check the repository with git at every step. They
  need `tauri-driver` (`cargo install tauri-driver`) and, on Linux,
  `WebKitWebDriver` (`webkit2gtk-driver`). Point them at **throwaway**
  repositories: they commit, merge and land.

  ```sh
  tauri-driver &
  SPOR_REPO=/tmp/scratch node e2e/run.mjs "$PWD/src-tauri/target/debug/spor-desktop" shots/
  SPOR_REPO=/tmp/demo FAKE_GH_STATE=/tmp/gh.json node e2e/sections.mjs "$PWD/src-tauri/target/debug/spor-desktop" shots/
  ```

  `sections.mjs` describes the repository it expects at its top. Neither
  needs a real agent or GitHub. Put stand-ins first on `PATH`: a `claude`
  that prints Claude Code's `stream-json` events, and a `gh` that prints what
  `gh … --jq` would. Spor's parsers treat them exactly like the real tools.

## Layout

```
src-tauri/src/
  lib.rs        setup, repository state for the chrome, undo, open_url
  undo.rs       the undo stack
  history.rs    graph (lanes → segments), commits, branches, switch, merge, cherry-pick, revert
  changes.rs    status, diffs, staging lines, discard, commit, sync, stashes, conflicts
  github.rs     pull requests and Actions, through spor's gh wrappers
  agents.rs     agent tasks
src/
  App.tsx       the shell: rail, sections, conflict banner, toasts, ⌘1–5, ⌘Z
  shared/       DiffView, details card, status pills, CI status, icons, hooks
  sections/     changes/ history/ pulls/ actions/ agents/
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
changes. A file watcher would be better.
