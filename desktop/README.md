# Spor desktop (Tauri)

Spor's app: a web UI on Tauri over the Rust core in the repository root
(`spor::git`, `spor::graph`, `spor::github`, `spor::tasks`, `spor::agent`,
`spor::askpass`). It has six sections, each built from the same four pieces:
a list in the sidebar, a page, a details card on the right, and a bar or
composer at the bottom.

| Section | ⌘ | Sidebar | Page | Details card | Bottom |
|---|---|---|---|---|---|
| **Changes** | 1 | Conflicts, staged and unstaged files | The file's diff (unified or side by side): click lines, shift-click to extend, then stage, unstage or discard them; a conflicted file's regions, a side picked for each | Branch and upstream, Fetch / Pull / Push, its PR, stashes (pop, apply, drop) | Commit box (⌘↩), amend |
| **History** | 2 | Search (`path:` for a file), branches (local, agent, remote), tags, worktrees — right-click for more, drag one onto another to merge or open a PR | The commit graph with PR badges and options (⚙), a commit's diff, Rewrite (interactive rebase), and Blame | The commit: message, parents, Branch here, Cherry-pick, Revert, More… (tag, check out, rebase, reset, rewrite) | |
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
  so and offers to stash them.
- **Credentials.** Fetch, pull and push run git with this app as its
  askpass helper (`spor::askpass`): a passphrase, password or new-host
  question comes up as a dialog. The background fetch (every five minutes)
  never asks; it just fails quietly without stored credentials.
- **⌘K palette and workspaces.** Commands, sections, branches and every
  workspace's repositories by typing. Workspaces sit at the top of the rail;
  the repository name at the top of every sidebar lists the workspace's
  repositories. The first launch imports the old native (GPUI) app's
  workspaces (`shared/workspaces.ts`).
- **Settings (⌘,).** Appearance and themes (`shared/theme.ts`, with the
  editor in `shared/ThemeEditor.tsx`), diffs, the graph's options, and
  shortcuts. Settings are one object in localStorage (`shared/settings.ts`);
  every command and its default shortcut is in one table in `App.tsx`, read
  by the key handler, the palette and the Shortcuts page (`shared/keys.ts`
  resolves them).

The graph's lanes are laid out by `spor::graph::assign_lanes`, and the
segments for each row are worked out in Rust (`src-tauri/src/history.rs`).
The UI only draws them as SVG, and only for the rows in view.

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

- `pnpm test`: the transcript folding and summaries, and shortcut parsing.
- `e2e/run.mjs` (agent tasks), `e2e/setup.mjs` (setup scripts) and
  `e2e/sections.mjs` (Changes, History, renaming and tagging, a conflicting
  merge, stashes, pull requests and Actions) drive the real app through WebDriver and check the
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
  lib.rs        setup, repository state for the chrome, undo, open_url, the askpass bridge
  undo.rs       the undo stack
  history.rs    graph (lanes → segments), commits, branches, tags, worktrees, switch, merge,
                cherry-pick, revert, rebase, reset, rewrite, PR badges
  changes.rs    status, diffs, staging lines, discard, commit, sync, stashes, conflicts
  github.rs     issues, pull requests and Actions, through spor's gh wrappers
  agents.rs     agent tasks, and their pull requests
src/
  App.tsx       the shell: rail, sections, conflict banner, toasts, ⌘1–6, ⌘Z, ⌘K,
                background fetch, credential prompts, recent repositories
  shared/       DiffView, details card, menus and dialogs, palette, status pills, CI status, icons, hooks
  sections/     changes/ history/ issues/ pulls/ actions/ agents/
```

`src-tauri` is its own Cargo workspace, so the root crate stays a
dependency-free library.

## Not here yet

A signed, notarized Mac app (#62): CI's `Spor.app` is ad-hoc signed.

Polling, not events: the window polls the repository's state every 2 s
(cheap: refs and the index's mtime) and each section reloads when that
changes. A file watcher would be better (#38). The rules for starting an
agent live in `src-tauri/src/agents.rs`; they belong in the core (#39).
