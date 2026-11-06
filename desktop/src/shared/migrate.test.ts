import { test } from "node:test";
import assert from "node:assert/strict";
import { migrate } from "./migrate.ts";

const store = (init: Record<string, string>) => {
  const m = new Map(Object.entries(init));
  return {
    map: m,
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, v),
    removeItem: (k: string) => void m.delete(k),
  };
};

test("a stored Agents section opens Work, on the same task", () => {
  const s = store({
    "spor.section": JSON.stringify("agents"),
    "spor.agents.view": JSON.stringify({ kind: "task", id: "fix-login" }),
    "spor.seen": JSON.stringify({ "fix-login": 120 }),
  });
  migrate(s);
  assert.deepEqual(JSON.parse(s.map.get("spor.work.seen")!), { "task:fix-login": 120 });
  assert.equal(s.map.get("spor.section"), JSON.stringify("work"));
  assert.deepEqual(JSON.parse(s.map.get("spor.work.view")!), { kind: "task", id: "fix-login" });
  assert.equal(s.map.has("spor.agents.view"), false);
});

test("other sections and a newer Work page are left alone", () => {
  const s = store({
    "spor.section": JSON.stringify("history"),
    "spor.agents.view": JSON.stringify({ kind: "new" }),
    "spor.work.view": JSON.stringify({ kind: "pull", number: 3 }),
  });
  migrate(s);
  assert.equal(s.map.get("spor.section"), JSON.stringify("history"));
  assert.deepEqual(JSON.parse(s.map.get("spor.work.view")!), { kind: "pull", number: 3 });
});

test("a changed Agents shortcut moves to Work", () => {
  const s = store({ "spor.settings": JSON.stringify({ keymap: { "go:agents": "Mod+9", undo: null } }) });
  migrate(s);
  assert.deepEqual(JSON.parse(s.map.get("spor.settings")!).keymap, { "go:work": "Mod+9", undo: null });
});
