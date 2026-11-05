// The app's settings: one object in localStorage, read anywhere with
// `useSettings()` and changed with `setSettings()`. Every window and
// section sees a change at once.

import { useSyncExternalStore } from "react";
import type { Keymap } from "./keys";

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

/** Colors as CSS variable values, by variable name without the `--`. */
export type Palette = Record<string, string>;

/** One mode's theme: a preset and the user's changes to it. */
export interface ModeTheme {
  preset: string;
  overrides: Palette;
}

export interface ThemeSettings {
  appearance: "system" | "light" | "dark";
  light: ModeTheme;
  dark: ModeTheme;
  /** Corner radius of cards and panels, px. */
  radius: number;
  /** Base text size, px. */
  fontSize: number;
}

export const DEFAULT_THEME: ThemeSettings = {
  appearance: "system",
  light: { preset: "spor-light", overrides: {} },
  dark: { preset: "spor-dark", overrides: {} },
  radius: 8,
  fontSize: 13,
};

export interface Settings {
  diffLayout: "unified" | "split";
  graph: GraphOptions;
  /** Changes to the default shortcuts, by command id (null removes one). */
  keymap: Keymap;
  theme: ThemeSettings;
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
  keymap: {},
  theme: DEFAULT_THEME,
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
      theme: {
        ...DEFAULT_THEME,
        ...saved.theme,
        light: { ...DEFAULT_THEME.light, ...saved.theme?.light },
        dark: { ...DEFAULT_THEME.dark, ...saved.theme?.dark },
      },
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
