import { test } from "node:test";
import assert from "node:assert/strict";
import { doing, liveLine, steps } from "./progress.ts";
import type { Progress } from "../../types.ts";

const p = (over: Partial<Progress>): Progress => ({
  done: 0,
  total: 0,
  step: null,
  tool: null,
  detail: null,
  ...over,
});

test("the live line puts the time, the plan and the action together", () => {
  assert.equal(
    liveLine(p({ done: 5, total: 7, step: "Migrating the tests", tool: "Bash", detail: "pnpm test" }), 720),
    "Working 12m 00s · 5 of 7 · Migrating the tests — running pnpm test",
  );
  assert.equal(liveLine(p({ tool: "Edit", detail: "src/auth.rs" }), 42), "Working 42s · Editing src/auth.rs");
  assert.equal(liveLine(null, 42), "Working 42s");
});

test("a row says the step, or what it's doing without one", () => {
  assert.equal(steps(p({ done: 2, total: 3 }), true), "2/3");
  assert.equal(steps(p({}), true), null, "no plan, no steps");
  assert.equal(doing(p({ step: "Reading the routes", tool: "Read", detail: "a.ts" })), "Reading the routes");
  assert.equal(doing(p({ tool: "Grep", detail: "shareLinks" })), "Searching for shareLinks");
  assert.equal(doing(null), null);
});
