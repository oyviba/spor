# Spor desktop (Tauri)

Spor's app: a web UI on Tauri over the Rust core in the repository root
(`spor::git`, `spor::graph`, `spor::github`, `spor::tasks`, `spor::agent`,
`spor::askpass`). The layout reads left to right from the general to the
particular: workspace → repository → section. The rail holds the workspaces,
the **Inbox** (what needs me?) and **Agents** (what are my agents doing?),
both across every repository, so while one is open the app shows no
repository (no repository column, branch bar or repository header; a
workspace's badge goes back in, to the section last open there); the column
beside it lists the workspace's repositories, and under the open one its
sections, ordered by the questions you come with (#83): **Work** (what's in
flight?), **Changes** (what's in my checkout?) and **History** (what
happened?). Below a divider, **Issues**, **Pull requests** and **Actions**
are browsers for what belongs to no work item yet. Each section is built from the same four pieces: a list
in the sidebar, a page, a details card on the right, and a bar or composer
at the bottom.

| Section | ⌘ | Sidebar | Page | Details card | Bottom |
|---|---|---|---|---|---|
| **Inbox** | 1 | What needs you in every workspace: answers, finished runs, failed CI, review requests | | | |
| **Agents** | ⇧A | Every agent task in every workspace, by status or by repository | Counts per repository (click one for its Work) | | |
| **Work** | 2 | Every work item: agents' tasks, your branches with commits ahead of the base, and open PRs from anyone. Shelves: Needs you, In progress, In review, Settled (folded); filters Mine / Agents / Teammates; right-click to move one up or down. *New…* (⌘N): a branch, an agent task, or from an issue | The work page, for a task, a PR or a branch: the stage track and who it waits on, and the tabs with something in them: Agent · Discussion · Changes · Checks. *Take over* opens a task's folder; *Start an agent on this* hands a branch to one | The work card (issue → branch · folder → PR → checks, and the next step). A task: conflicts, publish, merge, discard. A PR: merge, check out, close. A branch: open a PR, switch to it | A task's composer, or a PR's comment box |
| **Changes** | 3 | Conflicts, staged and unstaged files | The file's diff (unified or side by side): click lines, shift-click to extend, then stage, unstage or discard them; a conflicted file's regions, a side picked for each | The checked-out branch's work header (*Hand to an agent*: it moves to a task folder with your uncommitted work, ⌘Z brings it back) and work card, Fetch / Pull / Push (Publish the first time), stashes (pop, apply, drop) | Commit box (⌘↩), amend |
| **History** | 4 | Search (`path:` for a file), branches (local, agent, remote), tags, worktrees — right-click for more, drag one onto another to merge or open a PR | The commit graph with PR badges and options (⚙), a commit's diff, Rewrite (interactive rebase), and Blame | The commit: message, parents, Branch here, Cherry-pick, Revert, More… (tag, check out, rebase, reset, rewrite) | |
| **Issues** (browser) | 5 | Assigned / Yours / Open / Closed | The issue and its comments | The work card, labels, assignees, close or reopen | *Start with agent*: the issue as the prompt (editable), the base and the agent |
| **Pull requests** (browser) | 6 | Review / Yours / Open / Closed | The work page: Discussion · Changes · Checks | The work card, checks, conflicts, merge (squash, merge commit or rebase), check out, close | Comment / Approve, with line comments from Files |
| **Actions** (browser) | 7 | Runs, by workflow and branch | Jobs, and the log folded by step (failed steps open) | The work card, run, commit, re-run, cancel | |

What one app makes possible:

- **One inbox** (#42). Across every repository of every workspace:
  agents waiting on an answer, finished agent runs you haven't looked at, failed runs on your
  branches (agent branches included) and pull requests waiting for your
  review. Each row opens in the right section of the right repository, and
  goes once it's dealt with; nothing is stored. Agent tasks are read from
  each repository's `.git/spor/tasks` every few seconds; GitHub is one
  `gh search prs` per host and one `gh run list` per repository, every two
  minutes and whenever the Inbox is opened. The Inbox's count is the
  rail's only badge: one number for how many things need you.
- **Links open the work, not the list** (#83). A pull request, an issue or a
  CI run that belongs to a work item opens that item in Work, whether you
  click it in the Inbox, the palette or another section. Only what belongs
  to none stays in its browser. Links that name the browser (*Open in
  Actions* on a PR's check, a work item's own issue or PR) go there.

- **One undo stack.** Every change (stage, discard, commit, amend, pull,
  branch, switch, merge, cherry-pick, revert, merging a task) records how to take it
  back. ⌘Z, or a toast's Undo, takes back the latest from any section.
- **Links between sections.** A PR's failed check opens in Actions. A failed
  run on an agent's branch has *Send failure to agent*, which sends the
  failing log. A task's branch opens in History, and a run's commit too.
- **One conflict banner.** A merge or rebase that stops shows on every
  section, with *Resolve in Changes*, *Continue* and *Abort*.
- **From issue to merged PR** (#47, #24). *Start with agent* on an issue
  gives the agent the issue and its comments, on `agent/<number>-<three-random-words>`.
  *Publish* (or *Publish as a draft*) on the task pushes its branch and opens
  a PR whose title and description Claude Code writes from the branch and
  the agent's session (read-only; the prompt's if it can't), and which says
  `Closes #n`; merging locally puts `Closes #n`
  in the commit instead. The task then shows its PR's state, offers *Push*
  when the agent has commits the PR doesn't (*Commit and push* when it left
  work uncommitted), *Merge #n* (squash and merge on GitHub, which cleans the
  task up once GitHub has merged it; ⌘Z brings it back; *Merge into* the base is hidden while a PR is open,
  so the two can't diverge), and *Send review to agent*
  (on the task, or on the PR page) sends reviews and line comments made
  since the agent last ran, as `file:line — comment`. When the PR merges, a
  notification offers *Clean up*: the task is marked merged and its folder
  and branch go (⌘Z brings them back). PRs opened from a task's branch some
  other way are linked too.
- **Stash and switch.** A switch that uncommitted changes would block says
  so and offers to stash them.
- **Credentials.** Fetch, pull and push run git with this app as its
  askpass helper (`spor::askpass`): a passphrase, password or new-host
  question comes up as a dialog. The background fetch (every five minutes)
  never asks; it just fails quietly without stored credentials.
- **Live, without polling.** The backend watches the repository
  (`src-tauri/src/watch.rs`) and tells the window what changed: refs, the
  index, the working tree (what `.gitignore` doesn't exclude) or agent
  tasks, the last two at most once a second. Each section reloads on the
  changes it shows, and every 30 s anyway in case an event went missing.
- **⌘K palette and workspaces.** Commands, sections, branches and every
  workspace's repositories by typing. Workspaces sit at the top of the rail;
  beside it, the workspace's repositories, the open one marked and its
  sections under it, with their branch, changed files and what there is to
  pull or push (⌘⇧E folds the column to a strip of the open repository's
  sections; the repository name at the top of every sidebar lists them too).
  The first launch imports the old native (GPUI) app's
  workspaces (`shared/workspaces.ts`).
- **Settings (⌘,).** Appearance and themes (`shared/theme.ts`, with the
  editor in `shared/ThemeEditor.tsx`), diffs, the graph's options, and
  shortcuts. Settings are one object in localStorage (`shared/settings.ts`);
  every command and its default shortcut is in one table in `App.tsx`, read
  by the key handler, the palette and the Shortcuts page (`shared/keys.ts`
  resolves them). When the rail's defaults moved (#83), the shortcuts you
  had changed were kept, and the first launch after says so once, pointing
  to ⌘/; the move of the sections under the open repository is announced
  once the same way.

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

- `pnpm test`: the transcript folding and summaries, shortcut parsing and
  keeping your shortcuts when the defaults move, how reloads coalesce, the
  Inbox's rows, and which links open a work item.
- `cargo test` in `src-tauri`: the watcher, on scratch repositories.
- `e2e/run.mjs` (agent tasks), `e2e/setup.mjs` (setup scripts) and
  `e2e/sections.mjs` (Changes, History, renaming and tagging, a conflicting
  merge, stashes, pull requests, the Inbox and Actions) drive the real app through WebDriver and check the
  repository with git at every step. CI runs all three on Linux under Xvfb.
- `e2e/journeys.mjs` drives the five journeys in `docs/journeys.md`, counting
  how often each changes section against its budget. Steps waiting on work
  that hasn't landed are known gaps, reported but not failed; CI runs it
  without failing the build. `e2e/run-all.sh <app> <out> journeys` runs it.

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
  watch.rs      watching the repository: refs-, index-, worktree- and tasks-changed events
  undo.rs       the undo stack
  history.rs    graph (lanes → segments), commits, branches, tags, worktrees, switch, merge,
                cherry-pick, revert, rebase, reset, rewrite, PR badges
  changes.rs    status, diffs, staging lines, discard, commit, sync, stashes, conflicts
  github.rs     issues, pull requests and Actions, through spor's gh wrappers
  agents.rs     agent tasks, and their pull requests
  inbox.rs      what needs you across every workspace's repositories
  work.rs       your own branches, for Work
src/
  App.tsx       the shell: rail, sections, conflict banner, toasts, ⌘1–7, ⌘Z, ⌘K,
                background fetch, credential prompts, recent repositories
  shared/       DiffView, details card, menus and dialogs, palette, status pills, CI status, icons, hooks
  sections/     inbox/ work/ changes/ history/ issues/ pulls/ actions/ (and agents/, a task's page)
```

`src-tauri` is its own Cargo workspace, so the root crate stays a
dependency-free library.

## Not here yet

A signed, notarized Mac app (#62): CI's `Spor.app` is ad-hoc signed.

On Linux the watcher uses inotify, which needs a
watch per folder: a working tree with more folders than
`fs.inotify.max_user_watches` isn't watched, and the window reloads only
every 30 s.
