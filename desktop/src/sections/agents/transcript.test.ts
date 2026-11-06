import { test } from "node:test";
import assert from "node:assert/strict";
import { duration, runs, summarize, type Tool } from "./transcript.ts";
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
    { kind: "ask", ask: "permission", id: "q1", tool: "Bash", detail: "make test", session: null },
    { kind: "answer", id: "q1", allow: false, message: "use cargo", always: false },
    { kind: "steer", text: "also docs" },
    { kind: "text", text: "ok" },
    { kind: "done", ok: true, summary: "ok", cost_usd: null, turns: null, seconds: 3 },
  ];
  const [run] = runs(items, false);
  assert.deepEqual(run.blocks[0], {
    kind: "ask",
    ask: "permission",
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

test("a plan sent back doesn't keep the run open; its text goes with it", () => {
  const items: Item[] = [
    { kind: "prompt", text: "a" },
    { kind: "ask", ask: "plan", id: "p1", tool: "ExitPlanMode", detail: "Rename", session: null, plan: "# Rename\n1. Do it" },
    { kind: "answer", id: "p1", allow: false, message: "keep an alias", always: false },
    { kind: "ask", ask: "plan", id: "p2", tool: "ExitPlanMode", detail: "Rename", session: null, plan: "# Rename\n1. Alias" },
    { kind: "answer", id: "p2", allow: true, message: "", always: false },
    { kind: "tool", name: "Edit", detail: "a.ts" },
    { kind: "text", text: "Renamed." },
    { kind: "done", ok: true, summary: "", cost_usd: null, turns: null, seconds: 30 },
  ];
  const [run] = runs(items, false);
  const plans = run.blocks.filter((b) => b.kind === "ask" && b.ask === "plan");
  assert.equal(plans.length, 2, "each version of the plan stays");
  assert.deepEqual(
    plans.map((b) => (b.kind === "ask" && b.ask === "plan" ? [b.plan, b.answer?.allow] : null)),
    [
      ["# Rename\n1. Do it", false],
      ["# Rename\n1. Alias", true],
    ],
  );
  assert.notEqual(run.fold, null, "revising a plan isn't something going wrong");
});

test("questions with options keep them, and the answer", () => {
  const questions = [{ question: "Keep it?", header: "Keep", options: [{ label: "Yes", description: "" }], multi: false }];
  const [run] = runs(
    [
      { kind: "prompt", text: "a" },
      { kind: "ask", ask: "questions", id: "q", tool: "AskUserQuestion", detail: "Keep it?", session: null, questions },
      { kind: "answer", id: "q", allow: true, message: "Yes", always: false },
    ],
    true,
  );
  assert.deepEqual(run.blocks[0], {
    kind: "ask",
    ask: "questions",
    questions,
    id: "q",
    tool: "AskUserQuestion",
    detail: "Keep it?",
    answer: { allow: true, message: "Yes", always: false },
  });
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
