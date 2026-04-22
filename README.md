# spor

A minimal git TUI inspired by GitKraken's timeline. Single dependency (`crossterm`).

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

## Keys

**Global:**
- `?` — show all keyboard shortcuts
- `q` or `Ctrl-C` — quit
- `Tab` — switch focus between graph and file list
- `r` — refresh
- `b` — branch picker
- `R` — open pull/merge request for the current branch
- `P` — pull (fast-forward only)
- `p` — push (warns if behind)

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

- `src/git.rs` — shells out to `git`, parses porcelain output
- `src/graph.rs` — lane assignment, with main pinned to lane 0
- `src/color.rs` — HSL color families by branch prefix
- `src/ui.rs` — ANSI rendering (no ratatui)
- `src/main.rs` — event loop, state, modal key handling

## License

MIT OR Apache-2.0, your choice.
