import { test } from "node:test";
import assert from "node:assert/strict";
import { agentGroups, agentState, countsByRepo } from "./overview.ts";
import type { InboxTask } from "../../types.ts";

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

test("a task's state: asking first, then running, then unseen results", () => {
  assert.equal(agentState(task({ asks: 1, run: "running" }), {}), "asking");
  assert.equal(agentState(task({ run: "running", ended: null }), {}), "working");
  assert.equal(agentState(task({ run: "finished" }), {}), "ready");
  assert.equal(agentState(task({ run: "failed" }), {}), "failed");
  assert.equal(agentState(task({ run: "stopped" }), {}), "failed");
  assert.equal(agentState(task({ run: null, started: null, ended: null }), {}), "idle");
});

test("a finished or failed task you've looked at is idle", () => {
  assert.equal(agentState(task({ run: "finished" }), { t: 200 }), "idle");
  assert.equal(agentState(task({ run: "failed" }), { t: 300 }), "idle");
  // Looked at before it last ran: ready again.
  assert.equal(agentState(task({ run: "finished" }), { t: 150 }), "ready");
});

test("grouped by state, in order, newest first, empty groups left out", () => {
  const tasks = [
    task({ id: "old", run: "running", started: 10, ended: null }),
    task({ id: "ask", asks: 2, repo: "/code/b" }),
    task({ id: "new", run: "running", started: 500, ended: null, repo: "/code/b" }),
  ];
  const groups = agentGroups(tasks, {}, "state");
  assert.deepEqual(
    groups.map((g) => [g.key, g.rows.map((r) => r.task.id)]),
    [
      ["asking", ["ask"]],
      ["working", ["new", "old"]],
    ],
  );
});

test("grouped by repository, by name, what needs you first in each", () => {
  const tasks = [
    task({ id: "z1", repo: "/x/zeta", run: "running", started: 900, ended: null }),
    task({ id: "a1", repo: "/y/alpha", run: null, started: null, ended: null, created: 999 }),
    task({ id: "a2", repo: "/y/alpha", asks: 1, started: 1 }),
  ];
  const groups = agentGroups(tasks, {}, "repo");
  assert.deepEqual(
    groups.map((g) => [g.label, g.rows.map((r) => r.task.id)]),
    [
      ["alpha", ["a2", "a1"]],
      ["zeta", ["z1"]],
    ],
  );
});

test("counts per repository", () => {
  const counts = countsByRepo([task({ id: "1" }), task({ id: "2", asks: 1 }), task({ id: "3", repo: "/code/b", run: "running" })], {});
  assert.deepEqual(counts, [
    { repo: "/code/a", counts: { asking: 1, working: 0, ready: 1, failed: 0, idle: 0 } },
    { repo: "/code/b", counts: { asking: 0, working: 1, ready: 0, failed: 0, idle: 0 } },
  ]);
});
