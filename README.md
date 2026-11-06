# spor

A git client for working alongside coding agents: a GitKraken-style
timeline, staging down to single lines, GitHub pull requests and Actions,
and agent tasks that each get their own branch and folder — in one desktop
app (Tauri) over a dependency-free Rust core.

"Spor" means *track* or *trace* in Norwegian — what you follow to see where a branch has been.

## Run it

You need Rust, Node 20+ and pnpm, plus the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS
(on Linux, `libwebkit2gtk-4.1-dev` and friends). Pull requests, Actions and
issues need the GitHub CLI (`gh auth login`).

```sh
cd desktop
pnpm install
SPOR_REPO=/path/to/a/repo pnpm tauri dev   # or pass the path, or start it inside a repo
```

A Mac app: `pnpm tauri build --bundles app` in `desktop/`. CI builds a
universal `Spor.app` on every push — grab the `Spor-macos` artifact from the
workflow run. It isn't signed or notarized, so the first launch of a
downloaded copy needs right-click → Open (or
`xattr -dr com.apple.quarantine Spor.app`).

`desktop/README.md` covers the UI-only mode (`pnpm dev`, with a mock
backend), the end-to-end tests and the app's layout.

## What it does

Six sections on the rail (⌘1–⌘6), each a list in the sidebar, a page, and a
details card:

- **Changes** — staged and unstaged files; click lines in a diff (shift-click
  to extend) to stage, unstage or discard just those. Commit with ⌘↩, or
  amend. Fetch, Pull (rebasing) and Push or Publish, the branch's pull
  request on GitHub, and stashes: stash (right-click for a message), pop,
  apply and keep, drop
- **History** — the commit graph with `main` / `master` / `trunk` pinned to
  the leftmost lane and branches colored by prefix (all `feat/*` share a
  hue). Search by message, author or hash, or `path:src/main.rs` for the
  commits that touched a file. The sidebar has local, agent and remote
  branches, tags and linked worktrees. Right-click a branch to check it out,
  merge it, rebase onto it, rename or delete it, or delete it from the
  remote, or drag it onto another branch to merge it there or open a pull
  request between the two. Right-click a commit to branch or tag it, check
  it out detached, cherry-pick, revert, rebase onto it, reset to it (soft,
  mixed, or hard after asking), or **Rewrite after here** — an interactive
  rebase on screen: reorder, reword, squash, fix up or drop, then Rewrite
  history. **Blame** any file, at a commit or in the working tree. The ⚙
  picks the graph's columns, row height and lane width
- **Issues**, **Pull requests**, **Actions** — through `gh`: an issue and its
  comments (and *Start agent task* from it); a PR's conversation, files and
  checks, with comments, line comments, approve, merge, check out; a run's
  jobs and its log folded by step, re-run and cancel. Branch labels in the
  graph carry their open PR's number, review state and CI (`#42✓`)
- **Agents** — give a coding agent its own branch and folder so several can
  work at once without touching your checkout. A task is a prompt: spor
  creates `agent/<name>` from a base, checks it out in `<repo>.spor/<name>`
  beside the repository, and starts Claude Code (or any command) there.
  Paste, drop or attach images (screenshots, mockups) into any prompt —
  a new task, an issue's, or a follow-up; they're kept in
  `.git/spor/attachments/` and the agent is told where to find them.
  Watch the conversation, answer its questions, steer it while it works,
  review its changes with line comments, send feedback, undo a run, then
  **Land** it (one squashed commit, or a merge commit), open a pull request
  for it, or discard it. [docs/agents.md](docs/agents.md) has the design

Across the app:

- **Undo instead of confirmations.** Staging, discarding, committing,
  amending, pulling, switching, merging, cherry-picks, reverts, rebases,
  resets, rewrites, renames, deleting a branch, tag or stash, and landing or
  discarding a task all happen at once and can be taken back with ⌘Z or the
  toast's Undo. Pushing makes commits final: their undos go. Only what git
  can't take back asks first (a hard reset, deleting on the remote)
- **One conflict banner.** A merge, rebase, cherry-pick or revert that stops
  shows on every section, with *Resolve in Changes* — pick a side (or both)
  for each conflict in a file, take one side for all of it, or edit it and
  mark it resolved — *Continue*, *Skip commit* and *Abort*
- **Stash and switch.** A switch that uncommitted changes would block says
  so and offers to stash them
- **Knowing when to pull.** The repository is fetched quietly every five
  minutes; when the upstream gains commits, a notification says so with a
  Pull button
- **Credentials.** The app has no terminal, so a passphrase, password or
  new-host question from git or ssh opens a dialog (an askpass bridge)
- **Workspaces** — named, colored groups of repositories on the rail, like
  Slack's (⌘⌥1–9), each reopening the repository last open in it. Beside
  the rail, a column lists the workspace's repositories with the open one
  marked, their branch, changed files and what there is to pull (⌘⇧E hides
  it). The old native
  app's workspaces are imported on the first run
- **⌘K** — a palette for every command, section, branch and repository
- **Settings** (⌘,) — System, Light or Dark, themes (Spor, Nord, Dracula,
  Midnight; Solarized, Paper) and a live theme editor; unified or
  side-by-side diffs; the graph's options; and your own keyboard shortcuts
  (⌘/ lists them)

## Design notes

The core is a library (`src/lib.rs`) that knows nothing about how it's
drawn, and has no dependencies — it runs `git` and `gh` and parses their
output:

- `src/git.rs` — a `Repo` handle that runs `git` in that repository's
  directory (never the process's), and parses its output
- `src/graph.rs` — lane assignment, with main pinned to lane 0
- `src/color.rs` — HSL color families by branch prefix
- `src/patch.rs` — cuts a patch down to selected lines, for staging,
  unstaging or discarding part of a file
- `src/remote.rs` — remote host detection, compare URLs, PR badges via `gh`
- `src/github.rs` — pull requests, checks, reviews, issues, Actions runs,
  jobs and logs via `gh`, and the actions on them
- `src/askpass.rs` — credential prompts from git and ssh, handed to the app
  over a private socket, and running network git commands
- `src/tasks.rs` — agent tasks: a prompt with its own branch and worktree;
  creating, measuring, landing, discarding and restoring them
- `src/agent.rs`, `src/host.rs`, `src/checkpoint.rs` — running agents in
  task folders, the host that lets the app talk to a running Claude Code,
  and per-run checkpoints
- `src/json.rs` — a small JSON reader for agents' event streams

The app is `desktop/`: `src-tauri/` turns the core into commands (one
module per section, plus the undo stack), and `src/` is the React UI.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), your choice.
