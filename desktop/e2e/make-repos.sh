#!/bin/sh
# Build the scratch repositories the end-to-end suites drive:
#
#   e2e/make-repos.sh <dir>
#
#   <dir>/agents    for run.mjs: a small Python project on main
#   <dir>/setup     for setup.mjs: the same, with a .spor/setup that fails
#                   the first time and works the second, and a .spor/copy
#                   that brings an ignored .env into new task folders
#   <dir>/sections  for sections.mjs: see the comment at its top
#
# Everything in <dir> is replaced.
set -eu

dir=${1:?usage: make-repos.sh <dir>}
mkdir -p "$dir"
dir=$(cd "$dir" && pwd)
for name in agents setup sections; do
  if [ -e "$dir/$name" ]; then
    git -C "$dir/$name" worktree prune 2>/dev/null || true
    rm -rf "$dir/$name" "$dir/$name.spor"
  fi
done

new_repo() {
  repo="$dir/$1"
  mkdir -p "$repo"
  git -C "$repo" init -q --initial-branch=main
  git -C "$repo" config user.name "Spor Test"
  git -C "$repo" config user.email "test@spor.invalid"
  git -C "$repo" config commit.gpgsign false
  printf '# greeter\nSays hello.\nInstall with pip.\n' > "$repo/README.md"
  printf 'def greet(name):\n    """Say hello."""\n    return f"Hello, {name}!"\n' > "$repo/greet.py"
  printf '__pycache__/\n.env\n' > "$repo/.gitignore"
  git -C "$repo" add -A
  git -C "$repo" commit -qm "Start greeter"
}

# ---- agents -------------------------------------------------------------
new_repo agents

# ---- setup --------------------------------------------------------------
new_repo setup
mkdir -p "$repo/.spor"
cat > "$repo/.spor/setup" <<'EOF'
# Fails the first time (no network, say), works after.
marker="$SPOR_REPO/.git/setup-tried"
if [ ! -e "$marker" ]; then
  : > "$marker"
  echo "pip: can't reach the index"
  exit 1
fi
echo "Set up the folder for $SPOR_TASK"
EOF
printf '# Ignored files new task folders need\n.env\n' > "$repo/.spor/copy"
git -C "$repo" add -A
git -C "$repo" commit -qm "Add a setup script"
printf 'TOKEN=demo\n' > "$repo/.env"

# ---- sections -------------------------------------------------------------
new_repo sections
repo="$dir/sections"
git -C "$repo" remote add origin https://github.com/demo/greeter.git
g() { git -C "$repo" "$@"; }

# A branch that conflicts with main in README.md (line 3).
g switch -q -c fix/readme-install
printf '# greeter\nSays hello.\nInstall with uv.\n' > "$repo/README.md"
g commit -qam "Install with uv"
g switch -q main
printf '# greeter\nSays hello.\nInstall with pip install greeter.\n' > "$repo/README.md"
g commit -qam "Say how to install"

# A feature branch whose tip changes README.md too, so switching to it
# with README.md staged is blocked.
g switch -q -c feat/farewell
printf '\n\ndef farewell(name):\n    return f"Bye, {name}!"\n' >> "$repo/greet.py"
g commit -qam "Add farewell()"
printf 'Also says goodbye.\n' >> "$repo/README.md"
g commit -qam "Tidy farewell"
g switch -q main

# An agent task, as spor makes them: a branch, a folder beside the
# repository, and its file in .git/spor/tasks.
task=let-greet-take-optional
worktree="$dir/sections.spor/$task"
g worktree add -q -b "agent/$task" "$worktree" main
printf 'def greet(name, title=None):\n    """Say hello."""\n    if title:\n        name = f"{title} {name}"\n    return f"Hello, {name}!"\n' > "$worktree/greet.py"
git -C "$worktree" commit -qam "Let greet() take an optional title"
mkdir -p "$repo/.git/spor/tasks/$task.d"
printf 'id\t%s\nprompt\tLet greet() take an optional title\nbase\tmain\nbranch\tagent/%s\nworktree\t%s\ncreated\t%s\nstate\topen\n' \
  "$task" "$task" "$worktree" "$(date +%s)" > "$repo/.git/spor/tasks/$task.task"

# The working tree: README.md staged (line 2), greet.py edited, NOTES.md new.
printf '# greeter\nSays hello, politely.\nInstall with pip install greeter.\n' > "$repo/README.md"
g add README.md
printf '\n\ndef shout(name):\n    return greet(name).upper()\n' >> "$repo/greet.py"
printf 'Ideas: a farewell, a shout.\n' > "$repo/NOTES.md"

echo "Made $dir/agents, $dir/setup and $dir/sections"
