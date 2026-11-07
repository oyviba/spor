import { test } from "node:test";
import assert from "node:assert/strict";
import { askLine, inboxItems } from "./inbox.ts";
import type { InboxGithub, InboxTask } from "../../types.ts";

const task = (over: Partial<InboxTask>): InboxTask => ({
  repo: "/code/a",
  id: "t",
  title: "A task",
  asks: 0,
  ask: null,
  run: "finished",
  started: 100,
  ended: 200,
  created: 50,
  ...over,
});

const github: InboxGithub = {
  reviews: [{ repo: "/code/b", number: 7, title: "Add a thing", author: "ada", updated: 300, url: "" }],
  runs: [
    {
      repo: "/code/a",
      id: 99,
      number: 3,
      workflow: "CI",
      title: "Fix it",
      branch: "agent/1-fix",
      sha: "",
      event: "push",
      status: "failure",
      attempt: 1,
      created: 0,
      updated: 250,
      url: "",
    },
  ],
  errors: [],
};

test("gathers every kind, grouped, newest first within a group", () => {
  const items = inboxItems(
    [
      task({ id: "asking", asks: 1, run: "running", ended: null }),
      task({ id: "old", ended: 150 }),
      task({ id: "new", ended: 400 }),
      task({ id: "working", run: "running", ended: null }),
    ],
    github,
    {},
  );
  assert.deepEqual(
    items.map((i) => `${i.kind}:${i.key.split(":").pop()}`),
    ["ask:asking", "failed:99", "ready:new", "ready:old", "review:7"],
  );
  assert.deepEqual(items[1].target, { section: "actions", runId: 99, branch: "agent/1-fix" });
  assert.deepEqual(items[4].target, { section: "pulls", number: 7 });
  assert.equal(items[4].repo, "/code/b");
});

test("a task you've looked at since it finished drops out", () => {
  const t = task({ id: "seen-it", ended: 200 });
  assert.equal(inboxItems([t], null, { "seen-it": 200 }).length, 0);
  // It ran again after you looked.
  assert.equal(inboxItems([{ ...t, ended: 500 }], null, { "seen-it": 200 }).length, 1);
});

test("stopped runs and tasks never run aren't waiting on you", () => {
  assert.equal(inboxItems([task({ run: "stopped" }), task({ run: null })], null, {}).length, 0);
});

test("a question's row says what it asks", () => {
  assert.equal(askLine("asks: Keep the old endpoint?", 1), "The agent asks: Keep the old endpoint?");
  assert.equal(
    askLine("has a plan for you to review: Rename", 2),
    "The agent has a plan for you to review: Rename (and 1 more)",
  );
  assert.equal(askLine(null, 1), "The agent is asking before it goes on");
});

test("a finished run says why from its debrief", () => {
  const [item] = inboxItems([task({ reason: "2 open questions" })], null, {});
  assert.equal(item.detail, "The agent finished · 2 open questions");
});
