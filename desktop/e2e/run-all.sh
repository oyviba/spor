#!/bin/sh
# Run the end-to-end suites against the built app, with the stand-in
# `claude` and `gh` from e2e/bin first on PATH and fresh scratch
# repositories for each:
#
#   e2e/run-all.sh <app binary> [out dir] [suite …]
#
# Suites: run (agent tasks), setup (setup scripts), sections (Changes,
# History, a conflicting merge, stashes, Pulls, Actions): all three by
# default. journeys (docs/journeys.md, with its known gaps) only when named.
# Needs tauri-driver and, on Linux, WebKitWebDriver and a display (run it
# under xvfb-run). Screenshots go to <out dir>/<suite>/.
set -eu

app=$(cd "$(dirname "${1:?usage: run-all.sh <app binary> [out dir] [suite …]}")" && pwd)/$(basename "$1")
out=${2:-e2e-shots}
shift $(( $# > 1 ? 2 : 1 ))
suites=${*:-run setup sections}

here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/spor-e2e.XXXXXX")
PATH="$here/bin:$PATH"
export PATH

driver=""
stop_driver() {
  if [ -n "$driver" ]; then
    kill "$driver" 2>/dev/null || true
    wait "$driver" 2>/dev/null || true
    driver=""
  fi
}
trap stop_driver EXIT

failed=""
for suite in $suites; do
  case $suite in
    run) repo=agents ;;
    setup) repo=setup ;;
    sections) repo=sections ;;
    journeys) repo=journeys ;;
    *) echo "No suite $suite" >&2; exit 2 ;;
  esac
  echo "== $suite"
  "$here/make-repos.sh" "$scratch" > /dev/null
  # The app opens $SPOR_REPO; tauri-driver starts it, so it starts with it.
  SPOR_REPO="$scratch/$repo"
  FAKE_GH_STATE="$scratch/gh-state.json"
  echo '{}' > "$FAKE_GH_STATE"
  export SPOR_REPO FAKE_GH_STATE
  tauri-driver > "$out/tauri-driver-$suite.log" 2>&1 &
  driver=$!
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    curl -s http://127.0.0.1:4444/status > /dev/null 2>&1 && break
    sleep 0.5
  done
  if ! node "$here/$suite.mjs" "$app" "$out/$suite"; then
    failed="$failed $suite"
  fi
  stop_driver
done

if [ -n "$failed" ]; then
  echo "Failed:$failed" >&2
  exit 1
fi
echo "All suites passed"
