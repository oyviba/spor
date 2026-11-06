import { test } from "node:test";
import assert from "node:assert/strict";
import { findWork, itemFromIssue, itemFromPull, itemFromTask, nextStep, waitingText } from "./work.ts";
import type { Pull, TaskSummary, WorkItemInfo } from "../types.ts";

const item = (key: string, extra: Partial<WorkItemInfo>): WorkItemInfo => ({
  key,
  title: key,
  branch: null,
  base: "main",
  worktree: null,
  task: null,
  issue: null,
  pull: null,
  checks: null,
  owner: { kind: "you" },
  stage: "inprogress",
  waiting_on: null,
  updated: 0,
  ...extra,
});
const pr = (number: number, extra: Partial<NonNullable<WorkItemInfo["pull"]>> = {}) => ({
  number,
  title: `PR ${number}`,
  state: "open" as const,
  review: "none" as const,
  author: "you",
  url: `https://github.com/demo/greeter/pull/${number}`,
  ...extra,
});

const items = [
  item("issue:7", { issue: 7, stage: "planned" }),
  item("branch:agent/fix", { branch: "agent/fix", task: "fix", issue: 7, pull: pr(12) }),
  item("branch:feat/x", { branch: "feat/x" }),
];

test("finds an item by task, pull request, branch or issue", () => {
  assert.equal(findWork(items, { task: "fix" })?.key, "branch:agent/fix");
  assert.equal(findWork(items, { pull: 12 })?.key, "branch:agent/fix");
  assert.equal(findWork(items, { branch: "feat/x" })?.key, "branch:feat/x");
  // An issue with a branch is that branch's, not its Planned item.
  assert.equal(findWork(items, { issue: 7 })?.key, "branch:agent/fix");
  assert.equal(findWork(items, { issue: 8 }), undefined);
  // What's known wins over what isn't: a run's branch with no PR yet.
  assert.equal(findWork(items, { pull: 99, branch: "feat/x" })?.key, "branch:feat/x");
});

test("an item from what an inspector has, when the list doesn't", () => {
  const closed = { number: 9, title: "Shout", author: "grace", head: "try/shout", base: "main", state: "closed", review: "none", checks: "failing", updated: 1, additions: 1, deletions: 0, url: "u" } as Pull;
  const it = itemFromPull(closed, "you");
  assert.equal(it.key, "branch:try/shout");
  assert.equal(it.stage, "closed");
  assert.equal(it.checks, "failed");
  assert.deepEqual(it.owner, { kind: "teammate", login: "grace" });

  const t = { id: "a", title: "Do a", branch: "agent/a", base: "main", worktree: "/r.spor/a", issue: 3, pull: null, landed: "abc", closed: false, run: null } as unknown as TaskSummary;
  assert.equal(itemFromTask(t).stage, "landed");
  assert.equal(itemFromTask(t).worktree, null, "a landed task's folder is gone");
  assert.equal(itemFromIssue(3, "Three", true).stage, "planned");
});

test("the next step for each stage, in the glossary's words", () => {
  const step = (extra: Partial<WorkItemInfo>, from?: Parameters<typeof nextStep>[1], current?: string) =>
    nextStep(item("i", extra), from, current);
  assert.deepEqual(step({ issue: 7, stage: "planned" }), { label: "Start", target: { section: "issues", number: 7 }, browse: true });
  assert.equal(step({ branch: "agent/a", task: "a", waiting_on: { who: "you", why: "question" } })?.label, "Answer the agent");
  assert.equal(step({ branch: "f", pull: pr(4), waiting_on: { who: "you", why: "review" } })?.label, "Review #4");
  assert.deepEqual(step({ branch: "f", waiting_on: { who: "you", why: "checksfailed" } })?.target, { section: "actions", branch: "f" });
  // Approved and green: land it where the task's landing buttons are, or
  // on the pull request's page.
  assert.deepEqual(step({ branch: "agent/a", task: "a", pull: pr(4), waiting_on: { who: "you", why: "ready" } }), {
    label: "Land #4",
    target: { section: "work", taskId: "a" },
  });
  assert.equal(step({ branch: "f", pull: pr(4), waiting_on: { who: "you", why: "ready" } })?.target.section, "pulls");
  // Unpublished: a task publishes from Work; your checkout from Changes.
  assert.equal(step({ branch: "agent/a", task: "a" })?.label, "Publish");
  assert.deepEqual(step({ branch: "feat/x" }, undefined, "feat/x")?.target, { section: "changes" });
  assert.equal(step({ branch: "feat/x" }, undefined, "main"), null);
  // Nothing for a teammate's branch, or once it's landed.
  assert.equal(step({ branch: "f", owner: { kind: "teammate", login: "ada" } }), null);
  assert.equal(step({ branch: "f", task: "a", stage: "landed" }), null);
  // A step back to the section showing the card is that section's buttons.
  assert.equal(step({ branch: "agent/a", task: "a" }, "work"), null);
});

test("says who it's waiting on", () => {
  assert.equal(waitingText(item("i", { waiting_on: { who: "you", why: "checksfailed" } })), "Waiting on you: checks failed");
  assert.equal(waitingText(item("i", { owner: { kind: "agent", name: "Claude Code" }, waiting_on: { who: "agent", why: null } })), "Waiting on Claude Code");
  assert.equal(waitingText(item("i", { owner: { kind: "teammate", login: "ada" }, waiting_on: { who: "teammate", why: null } })), "Waiting on ada");
  assert.equal(waitingText(item("i", {})), null);
});
