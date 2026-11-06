import { test } from "node:test";
import assert from "node:assert/strict";
import { bind, display, effective, isSafe, keepChanges, lookup, resetOne, shortcutOf } from "./keys.ts";

const ev = (key: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}, code?: string) => ({
  key,
  code,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

test("shortcutOf reads the physical key for letters and digits", () => {
  assert.equal(shortcutOf(ev("K", { metaKey: true, shiftKey: true }, "KeyK"), true), "Mod+Shift+K");
  // ⌥ changes the character on a Mac; the key is still K.
  assert.equal(shortcutOf(ev("˚", { metaKey: true, altKey: true }, "KeyK"), true), "Mod+Alt+K");
  assert.equal(shortcutOf(ev("!", { ctrlKey: true, shiftKey: true }, "Digit1"), false), "Mod+Shift+1");
  assert.equal(shortcutOf(ev(",", { metaKey: true }, "Comma"), true), "Mod+,");
  assert.equal(shortcutOf(ev("Enter", { metaKey: true }, "Enter"), true), "Mod+Enter");
  assert.equal(shortcutOf(ev("Shift", { shiftKey: true }, "ShiftLeft"), true), null);
});

test("Mod is ⌘ on a Mac and Ctrl elsewhere", () => {
  assert.equal(shortcutOf(ev("k", { ctrlKey: true }, "KeyK"), true), "Ctrl+K");
  assert.equal(shortcutOf(ev("k", { ctrlKey: true }, "KeyK"), false), "Mod+K");
  assert.equal(shortcutOf(ev("k", { metaKey: true }, "KeyK"), false), "K");
  assert.equal(display("Mod+Shift+K", true), "⌘⇧K");
  assert.equal(display("Mod+Shift+K", false), "Ctrl+Shift+K");
  assert.equal(display("Mod+Enter", true), "⌘↩");
});

test("a shortcut needs a modifier that isn't Shift", () => {
  assert.ok(isSafe("Mod+K"));
  assert.ok(isSafe("Alt+F"));
  assert.ok(!isSafe("Shift+K"));
  assert.ok(!isSafe("K"));
});

const commands = [
  { id: "fetch" },
  { id: "palette", keys: "Mod+K" },
  { id: "undo", keys: "Mod+Z" },
];

test("binding a taken key moves it, and says from where", () => {
  const { keymap, took } = bind(commands, {}, "fetch", "Mod+K");
  assert.equal(took, "palette");
  assert.deepEqual(keymap, { fetch: "Mod+K", palette: null });
  assert.equal(lookup(commands, keymap, "Mod+K"), "fetch");
  assert.equal(effective(commands, keymap).get("palette"), undefined);
});

test("removing and resetting", () => {
  let { keymap } = bind(commands, {}, "undo", null);
  assert.equal(lookup(commands, keymap, "Mod+Z"), null);
  ({ keymap } = resetOne(commands, keymap, "undo"));
  assert.deepEqual(keymap, {});
  // Resetting takes the default back from whoever has it now.
  ({ keymap } = bind(commands, {}, "fetch", "Mod+Z"));
  const r = resetOne(commands, keymap, "undo");
  assert.equal(r.took, "fetch");
  assert.equal(lookup(commands, r.keymap, "Mod+Z"), "undo");
});

test("moved defaults don't take a shortcut the user chose", () => {
  // Before: History was Mod+2 and the user gave Fetch Mod+3, which was Issues'.
  const before = { "go:issues": null, fetch: "Mod+3", "go:history": "Mod+H" };
  // After: Changes' new default is Mod+3.
  const after = [
    { id: "go:changes", keys: "Mod+3" },
    { id: "go:history", keys: "Mod+4" },
    { id: "go:issues", keys: "Mod+5" },
    { id: "fetch" },
  ];
  const keymap = keepChanges(after, before);
  assert.equal(lookup(after, keymap, "Mod+3"), "fetch");
  assert.equal(lookup(after, keymap, "Mod+H"), "go:history");
  assert.equal(effective(after, keymap).get("go:changes"), undefined);
  // What the user didn't touch keeps its new default.
  assert.deepEqual(keepChanges(after, {}), {});
});
