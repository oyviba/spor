import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_PANE, runFacts } from "./panel.ts";
import type { RunInfo, TaskDetail } from "../../types.ts";

const run = (over: Partial<RunInfo> = {}): RunInfo => ({
  index: 0,
  agent: "claude",
  label: "Claude Code",
  prompt: "",
  state: "finished",
  exit_code: 0,
  started: 100,
  ended: 340,
  cost_usd: 0.4,
  tokens_in: 9_000,
  tokens_out: 1_000,
  context: null,
  model: "claude-sonnet-5-5",
  mcp_failed: [],
  resumable: true,
  undone: false,
  ...over,
});

const detail = (runs: RunInfo[]) => ({ runs, transcript: [] }) as unknown as TaskDetail;

test("a task without runs has nothing to summarize", () => {
  assert.deepEqual(runFacts(detail([]), false), { runs: 0, summary: null, checks: null, model: null, mcpFailed: [] });
});

test("the summary adds up cost and tokens over every run", () => {
  const f = runFacts(detail([run(), run({ index: 1, cost_usd: 0.25 })]), false);
  assert.equal(f.summary, "Claude Code · 2 runs · last 4m 00s · $0.65 · 20k tokens");
  assert.equal(f.model, "claude-sonnet-5-5");
});

test("only the last run's tools that didn't connect are reported", () => {
  const f = runFacts(detail([run({ mcp_failed: ["db"] }), run({ index: 1 })]), false);
  assert.deepEqual(f.mcpFailed, []);
  assert.deepEqual(runFacts(detail([run({ mcp_failed: ["db", "figma"] })]), false).mcpFailed, ["db", "figma"]);
});

test("the debrief and checks show by default, the run summary doesn't", () => {
  assert.deepEqual(DEFAULT_PANE, { debrief: true, checks: true, summary: false });
});
