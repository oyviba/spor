import { test } from "node:test";
import assert from "node:assert/strict";
import { EMPTY_NAV, leave, stepBack, stepForward, type Place } from "./nav.ts";

const inbox: Place = { repo: null, target: { section: "inbox" }, label: "Inbox" };
const task: Place = { repo: "/r", target: { section: "work", taskId: "a" }, label: "Work · a" };
const run: Place = { repo: "/r", target: { section: "actions", runId: 3 }, label: "Actions · #3" };

test("back returns to where you were, with what was open there", () => {
  let nav = leave(EMPTY_NAV, inbox);
  nav = leave(nav, task);
  const b = stepBack(nav, run)!;
  assert.deepEqual(b.to, task);
  const b2 = stepBack(b.nav, b.to)!;
  assert.deepEqual(b2.to, inbox);
  assert.equal(stepBack(b2.nav, b2.to), null);
  // And forward again, in order.
  const f = stepForward(b2.nav, inbox)!;
  assert.deepEqual(f.to, task);
  assert.deepEqual(stepForward(f.nav, task)!.to, run);
});

test("going somewhere new drops what was ahead", () => {
  const nav = leave(leave(EMPTY_NAV, inbox), task);
  const b = stepBack(nav, run)!;
  assert.equal(b.nav.forward.length, 1);
  assert.deepEqual(leave(b.nav, task).forward, []);
});

test("the same place isn't kept twice in a row", () => {
  const nav = leave(leave(EMPTY_NAV, task), { ...task });
  assert.equal(nav.back.length, 1);
});
