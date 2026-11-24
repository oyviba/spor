import { test } from "node:test";
import assert from "node:assert/strict";
import { groupBranches } from "./groups.ts";

const id = (s: string) => s;

test("branches group by the part before the first slash, in list order", () => {
  const g = groupBranches(["feat/x", "dev", "chore/a", "feat/y/z", "chore/b"], id);
  assert.deepEqual(g, [
    {
      kind: "group",
      prefix: "feat",
      entries: [
        { item: "feat/x", label: "x" },
        { item: "feat/y/z", label: "y/z" },
      ],
    },
    { kind: "branch", entry: { item: "dev", label: "dev" } },
    {
      kind: "group",
      prefix: "chore",
      entries: [
        { item: "chore/a", label: "a" },
        { item: "chore/b", label: "b" },
      ],
    },
  ]);
});

test("a remote branch groups by what follows the remote, per remote", () => {
  const g = groupBranches(["origin/main", "origin/feat/x", "upstream/feat/x", "origin/feat/y"], id, 1);
  assert.deepEqual(g, [
    { kind: "branch", entry: { item: "origin/main", label: "origin/main" } },
    {
      kind: "group",
      prefix: "origin/feat",
      entries: [
        { item: "origin/feat/x", label: "x" },
        { item: "origin/feat/y", label: "y" },
      ],
    },
    { kind: "group", prefix: "upstream/feat", entries: [{ item: "upstream/feat/x", label: "x" }] },
  ]);
});

test("a name starting with a slash is left alone", () => {
  assert.deepEqual(groupBranches(["/odd"], id), [{ kind: "branch", entry: { item: "/odd", label: "/odd" } }]);
});
