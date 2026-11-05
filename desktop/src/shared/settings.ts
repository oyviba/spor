// The app's settings: one object in localStorage, read anywhere with
// `useSettings()` and changed with `setSettings()`. Every window and
// section sees a change at once.

import { useSyncExternalStore } from "react";

export interface GraphOptions {
  /** Row height in px. */
  rowHeight: number;
  /** Distance between lanes in px. */
  laneWidth: number;
  /** Which columns the graph shows besides the subject. */
  columns: { author: boolean; date: boolean; hash: boolean };
  /** Branch and tag labels in a column of their own instead of before the subject. */
  refColumn: boolean;
  /** Column widths in px, set by dragging the header. */
  widths: { refs: number; author: number; date: number; hash: number };
}

export interface Settings {
  diffLayout: "unified" | "split";
  graph: GraphOptions;
}

export const DEFAULT_GRAPH: GraphOptions = {
  rowHeight: 30,
  laneWidth: 14,
  columns: { author: true, date: true, hash: false },
  refColumn: false,
  widths: { refs: 200, author: 120, date: 56, hash: 70 },
};

const DEFAULTS: Settings = {
  diffLayout: "unified",
  graph: DEFAULT_GRAPH,
};

const KEY = "spor.settings";
const listeners = new Set<() => void>();

function load(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS;
    const saved = JSON.parse(raw) as Partial<Settings>;
    // Fill in anything added since it was saved.
    return {
      ...DEFAULTS,
      ...saved,
      graph: {
        ...DEFAULT_GRAPH,
        ...saved.graph,
        columns: { ...DEFAULT_GRAPH.columns, ...saved.graph?.columns },
        widths: { ...DEFAULT_GRAPH.widths, ...saved.graph?.widths },
      },
    };
  } catch {
    return DEFAULTS;
  }
}

let current: Settings = load();

export function getSettings(): Settings {
  return current;
}

export function setSettings(change: Partial<Settings> | ((old: Settings) => Settings)) {
  current = typeof change === "function" ? change(current) : { ...current, ...change };
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Storage blocked: keep it for this session.
  }
  listeners.forEach((l) => l());
}

export function useSettings(): Settings {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
  );
}

// Another window (or a reload elsewhere) changed them.
if (typeof window !== "undefined")
  window.addEventListener("storage", (e) => {
    if (e.key === KEY) {
      current = load();
      listeners.forEach((l) => l());
    }
  });
