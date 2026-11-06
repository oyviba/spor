import { test } from "node:test";
import assert from "node:assert/strict";
import { catchUpPrompt, restorePrompt } from "./prompts.ts";

test("catchUpPrompt rebases a local branch and merges into a pushed one", () => {
  assert.match(catchUpPrompt("main", ["a.rs"], false), /conflicts with it in a\.rs\. Rebase onto main/);
  const pushed = catchUpPrompt("main", [], true);
  assert.match(pushed, /conflicts with it\. Merge origin\/main into this branch/);
  assert.doesNotMatch(pushed, /git rebase/);
});

test("restorePrompt names the files and how much of each would go", () => {
  const p = restorePrompt("main", [
    { path: "src/App.tsx", lines: 109 },
    { path: "README.md", lines: 1 },
  ]);
  assert.match(p, /reached main after you started, in src\/App\.tsx \(109 lines\), README\.md \(1 line\)/);
  assert.match(p, /git diff main -- <file>/);
});
