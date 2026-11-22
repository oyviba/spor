import { test } from "node:test";
import assert from "node:assert/strict";
import { issueFrom, splitNames, titleOf } from "./newIssue.ts";

test("a title is the first line, without Markdown", () => {
  assert.equal(titleOf("\n## **Retry** the `fetch`\nmore"), "Retry the fetch");
  assert.equal(titleOf("- a list item"), "a list item");
});

test("a long title is cut at a word", () => {
  const t = titleOf("The session cookie is read before the middleware has had a chance to refresh it, so a stale one wins");
  assert.ok(t.length <= 81);
  assert.ok(t.endsWith("…"));
  assert.ok(!t.includes(" …"));
});

test("a filed issue quotes the text and links back to the task", () => {
  const d = issueFrom({
    task: { id: "31-login-redirect", branch: "spor/31-login-redirect" },
    what: "a risk in the debrief",
    text: "`src/auth.ts` Only relative paths are allowed\n\nCheck the double slash case.",
    labels: ["bug"],
  });
  assert.equal(d.title, "src/auth.ts Only relative paths are allowed");
  assert.ok(d.body.startsWith("> `src/auth.ts` Only relative paths are allowed\n>\n> Check the double slash case."));
  assert.ok(d.body.includes("agent task `31-login-redirect` (branch `spor/31-login-redirect`)"));
  assert.ok(d.body.endsWith("Spor-Task: 31-login-redirect"));
  assert.deepEqual(d.labels, ["bug"]);
  assert.deepEqual(d.assignees, ["@me"]);
});

test("one without a task has no trailer, and keeps its own title and link", () => {
  const d = issueFrom({
    what: "a failed check",
    title: "CI failed on main",
    url: "https://github.com/o/r/actions/runs/9",
    text: "error[E0308]: mismatched types",
  });
  assert.equal(d.title, "CI failed on main");
  assert.ok(d.body.endsWith("Filed from a failed check: https://github.com/o/r/actions/runs/9"));
  assert.ok(!d.body.includes("Spor-Task"));
});

test("names are split on commas, blanks and repeats dropped", () => {
  assert.deepEqual(splitNames(" bug, ui ,, bug,docs "), ["bug", "ui", "docs"]);
  assert.deepEqual(splitNames(""), []);
});
