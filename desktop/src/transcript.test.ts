import { test } from "node:test";
import assert from "node:assert/strict";
import { duration, runs, summarize, type Tool } from "./transcript.ts";
import type { Item } from "./types.ts";

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

test("duration formats", () => {
  assert.equal(duration(42), "42s");
  assert.equal(duration(125), "2m 05s");
  assert.equal(duration(3780), "1h 03m");
});
