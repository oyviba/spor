import { test } from "node:test";
import assert from "node:assert/strict";
import { catchUpPrompt } from "./prompts.ts";

test("catchUpPrompt rebases a local branch and merges into a pushed one", () => {
  assert.match(catchUpPrompt("main", ["a.rs"], false), /conflicts with it in a\.rs\. Rebase onto main/);
  const pushed = catchUpPrompt("main", [], true);
  assert.match(pushed, /conflicts with it\. Merge origin\/main into this branch/);
  assert.doesNotMatch(pushed, /git rebase/);
});
