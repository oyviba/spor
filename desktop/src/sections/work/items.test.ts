import { test } from "node:test";
import assert from "node:assert/strict";
import { matches, moveTo, shelve, unseen, workItems, type Sources } from "./items.ts";
import type { Pull, TaskSummary, WorkBranch } from "../../types.ts";

const task = (id: string, created: number, over: Partial<TaskSummary> = {}): TaskSummary => ({
  id,
  title: id,
  prompt: id,
  base: "main",
  from: null,
  branch: `agent/${id}`,
  worktree: `/w/${id}`,
  created,
  landed: null,
  stats: { commits: 1, uncommitted: 0, files: 1, additions: 1, deletions: 0, base_moved: false, last_commit: created, missing: false, unpushed: 0 },
  run: null,
  runs: 0,
  command: "",
  asks: 0,
  mode: null,
  issue: null,
  pull: null,
  ...over,
});

const pull = (number: number, head: string, over: Partial<Pull> = {}): Pull => ({
  number,
  title: `PR ${number}`,
  author: "ada",
  head,
  base: "main",
  state: "open",
  review: "none",
  checks: "passing",
  updated: 500,
  additions: 1,
  deletions: 0,
  url: "",
  ...over,
});

const branch = (name: string, time: number, over: Partial<WorkBranch> = {}): WorkBranch => ({
  name,
  base: "main",
  current: false,
  commits: [{ hash: "h", short: "h", subject: `Work on ${name}`, author: "me", time }],
  files: 1,
  additions: 1,
  deletions: 0,
  updated: time,
  ...over,
});

const sources = (over: Partial<Sources>): Sources => ({
  tasks: [],
  taskPulls: {},
  branches: [],
  pulls: [],
  yours: new Set(),
  review: new Set(),
  seen: {},
  ...over,
});

test("tasks, your branches and everyone's open PRs are one list, each once", () => {
  const items = workItems(
    sources({
      tasks: [task("fix-login", 100)],
      branches: [branch("feat/x", 200), branch("docs/y", 150)],
      pulls: [pull(1, "agent/fix-login"), pull(2, "docs/y"), pull(3, "feat/async", { author: "grace" })],
      yours: new Set([2]),
    }),
  );
  assert.deepEqual(
    items.map((i) => [i.key, i.owner, i.shelf]),
    [
      ["task:fix-login", "agent", "review"],
      ["branch:feat/x", "mine", "progress"],
      ["branch:docs/y", "mine", "review"],
      ["pull:3", "teammate", "review"],
    ],
  );
  assert.equal(items.find((i) => i.key === "branch:docs/y")!.title, "PR 2");
});

test("what needs you: questions, finished work you haven't seen, reviews asked of you, your PRs in trouble", () => {
  const finished = task("done", 10, { run: { index: 0, agent: "claude", label: "", prompt: "", state: "finished", exit_code: 0, started: 20, ended: 30, cost_usd: null, resumable: true, undone: false } });
  const s = sources({
    tasks: [task("asking", 1, { asks: 1 }), finished],
    branches: [branch("mine-failing", 5)],
    pulls: [pull(7, "mine-failing", { checks: "failing" }), pull(8, "theirs"), pull(9, "other")],
    yours: new Set([7]),
    review: new Set([8]),
  });
  const shelf = (key: string, items = workItems(s)) => items.find((i) => i.key === key)!.shelf;
  assert.equal(shelf("task:asking"), "needs");
  assert.equal(shelf("task:done"), "needs");
  assert.equal(shelf("branch:mine-failing"), "needs");
  assert.equal(shelf("pull:8"), "needs");
  assert.equal(shelf("pull:9"), "review");
  // Opening the finished task clears it.
  assert.equal(shelf("task:done", workItems({ ...s, seen: { "task:done": 30 } })), "progress");
});

test("landed tasks and merged PRs are settled", () => {
  const items = workItems(
    sources({
      tasks: [task("landed", 1, { landed: "abc" }), task("merged", 2)],
      taskPulls: { merged: { task: "merged", pull: pull(4, "agent/merged", { state: "merged" }), merge_commit: "def" } },
    }),
  );
  assert.deepEqual(items.map((i) => i.shelf), ["settled", "settled"]);
});

test("filters: Mine, Agents, Teammates", () => {
  const items = workItems(
    sources({ tasks: [task("t", 1)], branches: [branch("b", 2)], pulls: [pull(5, "c")] }),
  );
  const keys = (f: Parameters<typeof matches>[1]) => items.filter((i) => matches(i, f)).map((i) => i.key);
  assert.deepEqual(keys("mine"), ["branch:b"]);
  assert.deepEqual(keys("agents"), ["task:t"]);
  assert.deepEqual(keys("teammates"), ["pull:5"]);
  assert.equal(keys("all").length, 3);
});

test("the order is stable under activity, newest first, and yours to change", () => {
  const s = sources({ tasks: [task("old", 1), task("new", 2)] });
  const before = shelve(workItems(s), {}).progress.map((i) => i.key);
  assert.deepEqual(before, ["task:new", "task:old"]);
  // The old one doing something doesn't move it.
  const busy = { ...s, tasks: [task("old", 1, { stats: { ...task("old", 1).stats, last_commit: 999 } }), task("new", 2)] };
  assert.deepEqual(shelve(workItems(busy), {}).progress.map((i) => i.key), before);
  // Moving it to the top does.
  const items = workItems(s);
  const order = moveTo(items, {}, "task:old", true);
  assert.deepEqual(shelve(items, order).progress.map((i) => i.key), ["task:old", "task:new"]);
});

test("unseen until opened", () => {
  const [item] = workItems(sources({ pulls: [pull(1, "x", { updated: 50 })] }));
  assert.equal(unseen(item, {}), true);
  assert.equal(unseen(item, { "pull:1": 50 }), false);
  assert.equal(unseen(item, { "pull:1": 40 }), true);
});
