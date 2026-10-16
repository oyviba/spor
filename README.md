# spor

A minimal git client inspired by GitKraken's timeline — as a terminal UI and as
a native Mac app, both built on the same core. The terminal build has a single
dependency (`crossterm`); the app is opt-in behind the `gui` feature.

"Spor" means *track* or *trace* in Norwegian — what you follow to see where a branch has been.

## Features

- Timeline graph with `main` / `master` / `trunk` pinned to the leftmost lane
- Color by branch prefix: all `feat/*` branches share a hue, all `bug/*` share another
- Stage / unstage files, commit, push, pull
- Live diff pane for the selected commit or file
- Branch switching (checkout by selection, picker, create from commit)
- Ahead/behind tracking vs upstream
- Auto-stash on switch when the working tree blocks
- Open a PR / MR from the current branch (uses `gh` or `glab` if available, falls back to printing the compare URL)
- PR badges on branch tips — `(feat/login #42✓)` shows the open PR's number, review state, and CI status (GitHub repos with `gh` installed)

## PR badges

When the repo's `origin` is GitHub and `gh` is installed and authenticated, spor
fetches the open PRs in the background (never blocking the UI) and decorates
branch-tip labels in the graph:

- `#42` — the PR number, colored by review state: green = approved,
  red = changes requested, dim = draft, blue = no decision yet
- `✓` (green) — all checks passing
- `✗` (red) — at least one check failing
- `●` (yellow) — checks still running
- no glyph — the PR has no checks

Badges refresh on startup, on `r`, and after opening a PR with `R`. On other
hosts, or without `gh`, the graph simply renders without badges.

## Run

```sh
cd your-repo
cargo run --manifest-path /path/to/spor/Cargo.toml --release
```

Or install it:

```sh
cargo install --path /path/to/spor
cd your-repo
spor
```

## Mac app

The same timeline, working tree and diff in a native window, built on
[GPUI](https://gpui.rs) (Zed's UI framework) via GPUI Kit. Build the
app bundle on a Mac with the Xcode command line tools installed:

```sh
scripts/bundle-macos.sh              # this Mac's architecture
scripts/bundle-macos.sh --universal  # Apple Silicon + Intel
open target/macos/Spor.app
```

CI also builds a universal `Spor.app` on every push — grab the `Spor-macos`
artifact from the workflow run. It's ad-hoc signed, not notarized, so the first
launch of a downloaded copy needs right-click → Open (or
`xattr -dr com.apple.quarantine Spor.app`).

To run it without bundling (works on Linux and Windows too):

```sh
cargo run --release --features gui --bin spor-app -- /path/to/repo
```

The layout follows Zed's:

- **Workspace rail** — like Slack, the strip on the far left switches
  between workspaces such as *Work* and *Side projects* (⌘1–⌘9, ⌘⇧W for a
  new one; right-click to rename or delete). Each workspace has its own
  repositories and reopens the one you last had open in it. Opening a
  repository adds it to the current workspace, or jumps to the workspace it
  already belongs to; the repository menu in the title bar switches between
  a workspace's repositories, moves one to another workspace or removes it
- **Repositories** — the workspace's repositories in a sidebar beside the
  rail (⌘⇧E hides it); each shows its current branch, and a blue `↓3`
  when it has commits to pull (the open one also its change count). Hover
  for where it lives; click to switch, right-click to reveal or remove one
- **Title bar** — repository and branch pickers, and the branch's PR
- **Dock** — on the right by default; the status bar button moves it to the
  left. It holds the **git panel** (⌘⇧G): changed files split into Staged
  and Unstaged — click a file to move it across (and see its diff), so a
  commit takes exactly the files you pick — a commit message editor
  (⌘↩ commits), and one **Sync** button
  that fetches, then pulls or pushes as needed (or publishes a new branch);
  or the **branch list** (⌘⇧B) with local and remote branches, tags and
  stashes — click to show a branch in History, double-click to check it out.
  The search field on top (⌘⇧F) narrows it as you type. Right-click a tag
  to delete it (⌘Z brings it back) or delete it from a remote too.
  ⌘B hides the dock
- **Center tabs** — **Uncommitted Changes** (⌘⇧C), every changed file in one
  scrolling, syntax-highlighted diff; **History** (⌘⇧H), the commit graph
  over the selected commit's message and diff (`j`/`k` or arrows move,
  `Return` checks out). Branches and tags get their own column: a laptop
  marks a local branch, a cloud a branch on the remote (one label when it's
  both), and the default branch is filled in, with a bold lane. The ⚙ menu
  in the header picks columns, labels, row height and lane width. **Drag a
  branch** onto another branch's row to merge it there (⌘Z undoes) or open
  a pull request between the two
- **Status bar** — dock toggles, the branch with ahead counts and a
  clickable *↓3 to pull*,
  background jobs, and unified / split diffs
- **Undo instead of confirmations** — discarding changes, staging and
  committing happen immediately and can be taken back with ⌘Z or the Undo
  button in the notification (a commit stops being undoable once pushed)

Open a repository with ⌘O, from the welcome page, by dropping a folder on
the window, or by passing a path. Light and dark themes (based on One Light
and One Dark) follow the system or can be set from the repository menu.

The **theme editor** (⌘, or repository menu → Appearance → Edit Theme…)
opens beside the window, which previews every change as you make it. Pick
Dark or Light, start from a preset (Nord, Dracula, Midnight; Solarized,
Paper) and adjust surfaces, text, accent, list, border and diff colors,
corner radius and font size. Edits are saved as overrides on top of the
Spor themes; Reset goes back to them, and Copy as JSON gives a theme file.

**⌘P** opens a palette that searches the repository's pull requests — by
number, title, branch or author; with an empty query it lists the open
ones, typing also finds merged and closed ones — and opens the one you pick
on GitHub (needs `gh`). The same palette runs any command and shows its
shortcut. **⌘/** lists every keyboard shortcut; both are also in the
repository menu.

**Shortcuts are yours to change.** In the ⌘/ list, click a command and press
the keys you want (Esc cancels, ⌫ removes the shortcut); right-click for
Remove or Reset to Default, and Reset All puts everything back. Commands
without a default — Fetch, Pull, Push, Stash — can get one too. A key
already in use moves to the new command, and the list says which one lost
it. Outside History a shortcut needs ⌘, ⌃ or ⌥ so it can't swallow typing;
⌘1–⌘9 stay reserved for workspaces. Changes apply at once and are saved
in `keymap` in the config directory.

**Knowing when to pull** — the open repository is fetched quietly in the
background every few minutes. When its upstream has new commits, a
notification says so (with a Pull button), a *Pull 3* button appears in the
title bar, and the Sync button, status bar and sidebar light up until you
pull.

Sync runs in the background. The app has no terminal, so credentials must
come from a helper (macOS keychain, ssh-agent); a sync that would need a
password fails with the reason instead of hanging.

PR badges work the same as in the terminal (the app finds Homebrew's `gh` even
when launched from Finder).

## Keys

**Global:**
- `?` — show all keyboard shortcuts
- `q` or `Ctrl-C` — quit
- `Tab` — switch focus between graph and file list
- `r` — refresh
- `b` — branch picker
- `R` — open pull/merge request for the current branch
- `P` — pull (fast-forward only)
- `p` — push (asks for confirmation when behind upstream)

**Graph pane:**
- `j` / `k` or arrows — move selection
- `J` / `K` — scroll diff
- `Enter` or `o` — checkout branch at selected commit
- `n` — create new branch from selected commit

**File pane:**
- `j` / `k` — move selection
- `Space` — stage / unstage
- `c` — commit (opens message prompt)

**Branch picker:**
- Type to filter
- `↑` / `↓` — move selection
- `Enter` — checkout
- `Esc` — cancel

**Stash-and-switch prompt** (appears when switch is blocked by dirty tree):
- `s` — stash changes and switch
- `c` or `Esc` — cancel

## Layout

```
┌─ graph ─────────────┬─ Working tree ──────────┐
│ ●  main  fix bug    │  +  src/auth.rs         │
│ ●─┐ (feat/login)    │  ~  README.md           │
│ │ ●  wip            │                         │
│ ●─┘ merge           │  Diff                   │
│ ●                   │  @@ -1,3 +1,3 @@        │
│                     │  - old                  │
│                     │  + new                  │
└─────────────────────┴─────────────────────────┘
 ⎇ main ↑2 ↓1  │  ready  │  [j/k] move  [enter]/[o] checkout ...
```

## Design notes

The core is a library (`src/lib.rs`) that knows nothing about how it's drawn:

- `src/git.rs` — shells out to `git`, parses porcelain output
- `src/graph.rs` — lane assignment, with main pinned to lane 0
- `src/color.rs` — HSL color families by branch prefix
- `src/remote.rs` — remote host detection, compare URLs, PR badges via `gh`

Terminal UI (`spor`):

- `src/ui.rs` — ANSI rendering (no ratatui)
- `src/main.rs` — event loop, state, modal key handling

Mac app (`spor-app`, `--features gui`):

- `src/gui/app.rs` — the `Spor` view: state, git actions, undo, sync
- `src/gui/workspaces.rs` — workspaces and their repositories, persisted
- `src/gui/render.rs` — window frame: title bar, dock + center, status bar
- `src/gui/rail.rs` — the workspace rail
- `src/gui/repos_panel.rs` — the repository sidebar
- `src/gui/git_panel.rs`, `branches.rs` — the two dock panels
- `src/gui/changes_view.rs`, `history.rs` — the two center tabs
- `src/gui/history_options.rs` — the History graph's saved options
- `src/gui/palette.rs` — the ⌘P palette (PR search, commands) and the
  table of commands with their default shortcuts
- `src/gui/keymap.rs` — the user's shortcut changes, and the ⌘/ list
  where they're made
- `src/gui/graph.rs` — paints the commit graph
- `src/gui/welcome.rs` — the page shown when no repository is open
- `src/gui/repo.rs` — snapshot of a repository for the views
- `src/gui/theme.rs`, `assets/themes/spor.json` — Spor Light / Spor Dark,
  and the user's overrides
- `src/gui/theme_editor.rs` — the theme editor
- `scripts/bundle-macos.sh` — wraps the binary into `Spor.app`

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), your choice.
