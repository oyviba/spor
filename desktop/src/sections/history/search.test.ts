import { test } from "node:test";
import assert from "node:assert/strict";
import { highlight, searchTerms, stepHit } from "./search.ts";

test("searchTerms are the lowercase words, once each", () => {
  assert.deepEqual(searchTerms("  Fix  login fix "), ["fix", "login"]);
  assert.deepEqual(searchTerms(""), []);
  assert.deepEqual(searchTerms("path:src/main.rs"), []);
  assert.deepEqual(searchTerms("code:foo bar"), []);
});

test("highlight marks every term, whatever the case", () => {
  assert.deepEqual(highlight("Fix the Login page", ["login", "fix"]), [
    { text: "Fix", match: true },
    { text: " the ", match: false },
    { text: "Login", match: true },
    { text: " page", match: false },
  ]);
  assert.deepEqual(highlight("nothing here", ["x.y"]), [{ text: "nothing here", match: false }]);
  assert.deepEqual(highlight("a.b", ["a.b"]), [{ text: "a.b", match: true }]);
  assert.deepEqual(highlight("plain", []), [{ text: "plain", match: false }]);
});

test("stepHit goes to the next hit and wraps", () => {
  assert.equal(stepHit([], 3, 1), null);
  assert.equal(stepHit([2, 5, 9], -1, 1), 2);
  assert.equal(stepHit([2, 5, 9], 5, 1), 9);
  assert.equal(stepHit([2, 5, 9], 9, 1), 2);
  assert.equal(stepHit([2, 5, 9], 5, -1), 2);
  assert.equal(stepHit([2, 5, 9], 2, -1), 9);
  assert.equal(stepHit([2, 5, 9], 7, -1), 5);
});
