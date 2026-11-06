import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveTarget } from "./targets.ts";
import type { TaskSummary } from "../types.ts";

const task = (id: string, extra: Partial<TaskSummary>) =>
  ({ id, branch: `agent/${id}`, issue: null, pull: null, landed: null, ...extra }) as TaskSummary;
const tasks = [task("c", { issue: 9, landed: "abc123" }), task("b", { issue: 7, pull: 12 }), task("a", { issue: 7 })];

test("a pull request, issue or run of a work item opens the item", () => {
  assert.deepEqual(resolveTarget({ section: "pulls", number: 12 }, tasks), { section: "work", taskId: "b" });
  // The newest task started from the issue.
  assert.deepEqual(resolveTarget({ section: "issues", number: 7 }, tasks), { section: "work", taskId: "b" });
  assert.deepEqual(resolveTarget({ section: "actions", runId: 3, branch: "agent/a" }, tasks), { section: "work", taskId: "a" });
  // A pull request not yet recorded on the task: by its branch.
  assert.deepEqual(resolveTarget({ section: "pulls", number: 14, branch: "agent/a" }, tasks), { section: "work", taskId: "a" });
  // A landed work item is still one.
  assert.deepEqual(resolveTarget({ section: "issues", number: 9 }, tasks), { section: "work", taskId: "c" });
});

test("what belongs to no work item stays in its browser", () => {
  for (const t of [
    { section: "pulls", number: 13 },
    { section: "issues", number: 8 },
    { section: "actions", runId: 3, branch: "main" },
    { section: "actions", runId: 3 },
    { section: "pulls" },
    { section: "history", branch: "agent/a" },
  ] as const)
    assert.deepEqual(resolveTarget(t, tasks), t);
});
