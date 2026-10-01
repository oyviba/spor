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

The same timeline, working tree and diff in a native window (egui). Build the
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

The window is laid out like a desktop git client:

- **Toolbar** — repository and branch switchers (type to filter branches),
  then Fetch, Pull, Push (with ahead/behind counts), Branch, Stash and Pop,
  and on the right Pull request and Refresh
- **Sidebar** — local branches, remote branches per remote, and tags. Click
  to jump to a branch's tip, double-click to check it out, right-click for
  more
- **Commit list** — the timeline graph with branch/tag pills (a local branch
  and its remote twin share one pill), author avatars, dates and SHAs. When
  the working tree is dirty, an *Uncommitted changes* row sits on top
- **Inspector** — for a commit: message, author, SHA, parents and changed
  files; for uncommitted changes: unstaged/staged files with hover actions
  (stage, unstage, discard) and a commit composer (summary + description,
  ⌘⏎ to commit)
- **Diff** — the selected file with line numbers and highlighted
  additions/removals

Open a repository with **Open…** (⌘O), by dropping a folder on the window,
from the recent list on the start screen, or by passing a path. Keyboard:
`j`/`k` or arrows move through commits, `Enter` checks out, ⌘R refreshes.
Appearance follows the system; pick Light or Dark in the repository menu.

Pull and push run in the background. The app has no terminal, so
credentials must come from a helper (macOS keychain, ssh-agent); a push that
would need a password fails with the reason instead of hanging.

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
- `src/diff.rs` — unified-diff parser (files, hunks, numbered lines)

Terminal UI (`spor`):

- `src/ui.rs` — ANSI rendering (no ratatui)
- `src/main.rs` — event loop, state, modal key handling

Mac app (`spor-app`, `--features gui`):

- `src/gui/main.rs` — app state, git actions, background fetch/pull/push
- `src/gui/views.rs` — welcome screen, workspace layout, commit list
- `src/gui/graph_view.rs` — paints timeline rows (lanes, curves, ref pills)
- `src/gui/toolbar.rs`, `sidebar.rs`, `inspector.rs`, `diff_view.rs`,
  `modals.rs` — the panes and dialogs
- `src/gui/theme.rs` — light/dark palettes, fonts (Inter, JetBrains Mono,
  Phosphor icons)
- `scripts/bundle-macos.sh` — wraps the binary into `Spor.app`

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), your choice.
