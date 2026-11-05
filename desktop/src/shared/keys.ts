// Keyboard shortcuts as strings — "Mod+Shift+K", where Mod is ⌘ on a Mac
// and Ctrl elsewhere — read from key events, shown the platform's way, and
// resolved against the user's changes to the defaults (#55).

/** The parts of a key event a shortcut cares about. */
export interface KeyLike {
  key: string;
  code?: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

const MODIFIER_KEYS = new Set(["Meta", "Control", "Alt", "Shift", "OS", "Hyper", "Super", "AltGraph"]);

/**
 * The shortcut an event is, or null for a lone modifier. Letters and digits
 * come from the physical key (`code`), so ⇧ or ⌥ don't change them; other
 * keys from `key`.
 */
export function shortcutOf(e: KeyLike, mac: boolean): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null;
  let key: string;
  if (e.code?.startsWith("Key")) key = e.code.slice(3);
  else if (e.code?.startsWith("Digit")) key = e.code.slice(5);
  else if (e.key === " ") key = "Space";
  else key = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  const parts: string[] = [];
  // On a Mac ⌃ is its own modifier; elsewhere Ctrl is Mod and the Windows key is ignored.
  if (mac ? e.metaKey : e.ctrlKey) parts.push("Mod");
  if (mac && e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

/** Does it have a modifier that stops it swallowing typing (⌘/Ctrl, ⌃ or ⌥)? */
export function isSafe(shortcut: string): boolean {
  const parts = shortcut.split("+");
  return parts.some((p) => p === "Mod" || p === "Ctrl" || p === "Alt");
}

const MAC_SYMBOLS: Record<string, string> = {
  Mod: "⌘",
  Ctrl: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  Enter: "↩",
  Backspace: "⌫",
  Delete: "⌦",
  Escape: "⎋",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Tab: "⇥",
  Space: "Space",
};
const PC_NAMES: Record<string, string> = { Mod: "Ctrl", Alt: "Alt", Shift: "Shift", ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→" };

/** "⌘⇧K" on a Mac, "Ctrl+Shift+K" elsewhere. */
export function display(shortcut: string, mac: boolean): string {
  const parts = shortcut.split("+");
  return mac ? parts.map((p) => MAC_SYMBOLS[p] ?? p).join("") : parts.map((p) => PC_NAMES[p] ?? p).join("+");
}

export const isMac = () => typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** A command's default shortcut and the user's change to it. */
export interface Bindable {
  id: string;
  keys?: string;
}

/** null removes a default; a missing entry keeps it. */
export type Keymap = Record<string, string | null>;

/** Each command's shortcut after the user's changes. */
export function effective(commands: Bindable[], keymap: Keymap): Map<string, string> {
  const out = new Map<string, string>();
  for (const c of commands) {
    const k = c.id in keymap ? keymap[c.id] : c.keys;
    if (k) out.set(c.id, k);
  }
  return out;
}

/** Which command a shortcut runs, after the user's changes. */
export function lookup(commands: Bindable[], keymap: Keymap, shortcut: string): string | null {
  for (const [id, k] of effective(commands, keymap)) if (k === shortcut) return id;
  return null;
}

/**
 * Give `id` the shortcut `keys` (null removes it). A shortcut another
 * command had moves: that one loses it, and its id is returned so the UI
 * can say so.
 */
export function bind(commands: Bindable[], keymap: Keymap, id: string, keys: string | null): { keymap: Keymap; took: string | null } {
  const next: Keymap = { ...keymap };
  let took: string | null = null;
  if (keys) {
    for (const [other, k] of effective(commands, keymap))
      if (other !== id && k === keys) {
        took = other;
        next[other] = null;
      }
  }
  const def = commands.find((c) => c.id === id)?.keys ?? null;
  // Back to its default: no need to remember a change.
  if ((keys ?? null) === def) delete next[id];
  else next[id] = keys;
  return { keymap: next, took };
}

/** Put one command back to its default (taking the key back if it moved). */
export function resetOne(commands: Bindable[], keymap: Keymap, id: string): { keymap: Keymap; took: string | null } {
  const def = commands.find((c) => c.id === id)?.keys ?? null;
  return bind(commands, keymap, id, def);
}
