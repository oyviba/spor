import { test } from "node:test";
import assert from "node:assert/strict";
import { duration, runs, summarize, tokens, type Tool } from "./transcript.ts";
import type { Item } from "../../types.ts";

const tool = (name: string, detail = ""): Tool => ({ kind: "tool", name, detail });

test("summarize names the two most important actions and counts the rest", () => {
  const s = summarize([
    tool("Read", "a.rs"),
    tool("Bash", "cargo test"),
    tool("Edit", "a.rs"),
    tool("Edit", "b.rs"),
    tool("Grep", "fn main"),
    tool("Bash", "cargo build"),
  ]);
  assert.equal(s.text, "Ran 2 commands and edited 2 files, and performed 2 other actions");
  assert.equal(s.failed, 0);
});

test("summarize counts distinct files, and failures separately", () => {
  const s = summarize([tool("Edit", "a.rs"), tool("Edit", "a.rs"), { kind: "tool_error", text: "denied" }]);
  assert.equal(s.text, "Edited a file");
  assert.equal(s.failed, 1);
});

test("a finished run folds everything but its final message", () => {
  const items: Item[] = [
    { kind: "prompt", text: "Fix it" },
    { kind: "text", text: "Looking" },
    { kind: "tool", name: "Read", detail: "a.rs" },
    { kind: "text", text: "Fixed." },
    { kind: "done", ok: true, summary: "Fixed.", cost_usd: 0.1, turns: 3, seconds: 125 },
  ];
  const [run] = runs(items, false);
  assert.equal(run.fold?.label, "Worked for 2m 05s");
  assert.deepEqual(run.fold?.final, { kind: "text", text: "Fixed." });
  assert.equal(run.fold?.hidden.length, 2);
});

test("live, stopped and failed-tool runs don't fold", () => {
  const live = runs([{ kind: "prompt", text: "a" }, { kind: "tool", name: "Bash", detail: "x" }], true);
  assert.equal(live[0].live, true);
  assert.equal(live[0].fold, null);
  const stopped = runs(
    [
      { kind: "prompt", text: "a" },
      { kind: "tool", name: "Bash", detail: "x" },
      { kind: "done", ok: false, summary: "Stopped", cost_usd: null, turns: null, seconds: 9 },
    ],
    false,
  );
  assert.equal(stopped[0].fold, null);
  const refused = runs(
    [
      { kind: "prompt", text: "a" },
      { kind: "tool_error", text: "denied" },
      { kind: "text", text: "ok" },
      { kind: "done", ok: true, summary: "ok", cost_usd: null, turns: null, seconds: 9 },
    ],
    false,
  );
  assert.equal(refused[0].fold, null);
});

test("consecutive tool calls merge into one block", () => {
  const [run] = runs(
    [
      { kind: "prompt", text: "a" },
      { kind: "tool", name: "Read", detail: "a" },
      { kind: "tool", name: "Edit", detail: "a" },
      { kind: "text", text: "between" },
      { kind: "tool", name: "Bash", detail: "x" },
    ],
    true,
  );
  assert.deepEqual(
    run.blocks.map((b) => b.kind),
    ["tools", "text", "tools"],
  );
});

test("questions take their answers, and a refusal keeps the run open", () => {
  const items: Item[] = [
    { kind: "prompt", text: "a" },
    { kind: "ask", id: "q1", tool: "Bash", detail: "make test", session: null },
    { kind: "answer", id: "q1", allow: false, message: "use cargo", always: false },
    { kind: "steer", text: "also docs" },
    { kind: "text", text: "ok" },
    { kind: "done", ok: true, summary: "ok", cost_usd: null, turns: null, seconds: 3 },
  ];
  const [run] = runs(items, false);
  assert.deepEqual(run.blocks[0], {
    kind: "ask",
    id: "q1",
    tool: "Bash",
    detail: "make test",
    answer: { allow: false, message: "use cargo", always: false },
  });
  assert.deepEqual(run.blocks[1], { kind: "steer", text: "also docs" });
  assert.equal(run.fold, null);
  const allowed = runs(
    items.map((i) => (i.kind === "answer" ? { ...i, allow: true, message: "" } : i)),
    false,
  );
  assert.notEqual(allowed[0].fold, null, "an allowed question folds like any work");
});

test("the setup script's output goes with it", () => {
  const [run] = runs(
    [
      { kind: "prompt", text: "a" },
      { kind: "setup", ok: null, code: null },
      { kind: "output", text: "npm install" },
      { kind: "setup", ok: false, code: 3 },
      { kind: "done", ok: false, summary: "Exited with code 3", cost_usd: null, turns: null, seconds: 1 },
    ],
    false,
  );
  assert.deepEqual(run.blocks, [{ kind: "setup", ok: false, code: 3, lines: ["npm install"] }]);
});

test("duration formats", () => {
  assert.equal(duration(42), "42s");
  assert.equal(duration(125), "2m 05s");
  assert.equal(duration(3780), "1h 03m");
});

test("compaction is marked where it happened and noted on the fold", () => {
  const items: Item[] = [
    { kind: "prompt", text: "a" },
    { kind: "text", text: "before" },
    { kind: "compacted" },
    { kind: "text", text: "after" },
    { kind: "done", ok: true, summary: "", cost_usd: null, turns: null, seconds: 65 },
  ];
  const [run] = runs(items, false);
  assert.deepEqual(run.blocks.map((b) => b.kind), ["text", "compacted", "text"]);
  assert.equal(run.fold?.label, "Worked for 1m 05s, compacted");
});

test("an MCP server that failed to connect keeps the run unfolded", () => {
  const items: Item[] = [
    { kind: "prompt", text: "a" },
    { kind: "init", model: "m", mcp_failed: ["db"] },
    { kind: "text", text: "hi" },
    { kind: "tool", name: "Read", detail: "x" },
    { kind: "done", ok: true, summary: "", cost_usd: null, turns: null, seconds: 5 },
  ];
  const [run] = runs(items, false);
  assert.deepEqual(run.blocks[0], { kind: "mcp", failed: ["db"] });
  assert.equal(run.fold, null);
  // A clean start shows nothing.
  assert.equal(runs([{ kind: "prompt", text: "a" }, { kind: "init", model: "m", mcp_failed: [] }], false)[0].blocks.length, 0);
});

test("tokens are short", () => {
  assert.deepEqual([tokens(850), tokens(1234), tokens(12_345), tokens(1_250_000)], ["850", "1.2k", "12k", "1.3M"]);
});
