import { test } from "node:test";
import assert from "node:assert/strict";
import { branchStatus, syncButtons } from "./sync.ts";
import type { RepoState } from "../types.ts";

const state = (extra: Partial<RepoState>): RepoState => ({
  branch: "feat/x",
  upstream: "origin/feat/x",
  ahead: 0,
  behind: 0,
  remote: "origin",
  operation: null,
  conflicts: [],
  refs: "",
  index: "",
  undo: null,
  ...extra,
});

test("in step: nothing to press", () => {
  assert.deepEqual(syncButtons(state({})), []);
  assert.equal(branchStatus(state({})).tone, "ok");
});

test("ahead pushes, behind pulls", () => {
  assert.deepEqual(syncButtons(state({ ahead: 2 })), ["push"]);
  assert.deepEqual(syncButtons(state({ behind: 3 })), ["pull"]);
});

test("diverged from someone else's commits: pull and push in one go", () => {
  const s = state({ ahead: 1, behind: 2 });
  assert.deepEqual(syncButtons(s), ["sync", "pull"]);
  assert.match(branchStatus(s).text, /Diverged/);
});

test("diverged because the branch was rewritten: force push, not pull", () => {
  const s = state({ ahead: 1, behind: 1, rewritten: true });
  assert.deepEqual(syncButtons(s), ["force-push"]);
  assert.equal(branchStatus(s).tone, "warn");
  assert.match(branchStatus(s).text, /force-push/);
});

test("not published: publish; on the remote but unlinked: pull links it", () => {
  assert.deepEqual(syncButtons(state({ upstream: null })), ["push"]);
  const s = state({ upstream: null, unlinked: "origin/feat/x" });
  assert.deepEqual(syncButtons(s), ["pull"]);
  assert.match(branchStatus(s).text, /pull to link/);
});

test("no remote, or detached: no sync buttons", () => {
  const s = state({ upstream: null, remote: null });
  assert.deepEqual(syncButtons(s), []);
  assert.match(branchStatus(s).text, /no remote/);
  assert.deepEqual(syncButtons(state({ branch: null, ahead: 1 })), []);
});

test("an older backend without the new fields still gets buttons", () => {
  const s = state({ upstream: null });
  delete s.remote;
  assert.deepEqual(syncButtons(s), ["push"]);
});
