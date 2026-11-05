// Themes (#54): Light or Dark (or the system's), a preset for each, and the
// user's own edits on top. A theme is values for the CSS variables in
// styles.css; applying one sets them on the document root.

import { useEffect, useSyncExternalStore } from "react";
import { useSettings, type ModeTheme } from "./settings";

export type Mode = "light" | "dark";
export type { ModeTheme, ThemeSettings } from "./settings";
export { DEFAULT_THEME } from "./settings";

/** The variables a theme can set, grouped as the editor shows them. */
export const TOKENS: { group: string; tokens: { id: string; label: string }[] }[] = [
  {
    group: "Surfaces",
    tokens: [
      { id: "bg", label: "Background" },
      { id: "sidebar", label: "Sidebar" },
      { id: "card", label: "Cards" },
      { id: "raised", label: "Raised" },
      { id: "bubble", label: "Message bubbles" },
    ],
  },
  {
    group: "Text",
    tokens: [
      { id: "fg", label: "Text" },
      { id: "fg-2", label: "Secondary text" },
      { id: "muted", label: "Muted" },
      { id: "faint", label: "Faint" },
    ],
  },
  { group: "Accent", tokens: [{ id: "accent", label: "Accent" }] },
  {
    group: "Lines",
    tokens: [
      { id: "border", label: "Borders" },
      { id: "border-strong", label: "Strong borders" },
    ],
  },
  {
    group: "Diffs",
    tokens: [
      { id: "add", label: "Added text" },
      { id: "add-bg", label: "Added background" },
      { id: "del", label: "Removed text" },
      { id: "del-bg", label: "Removed background" },
    ],
  },
  {
    group: "Status",
    tokens: [
      { id: "sky", label: "Info" },
      { id: "emerald", label: "Success" },
      { id: "amber", label: "Warning" },
      { id: "red", label: "Error" },
    ],
  },
];

import type { Palette } from "./settings";
export type { Palette };

export interface Preset {
  id: string;
  label: string;
  mode: Mode;
  colors: Palette;
}

export const PRESETS: Preset[] = [
  // Spor's own, as in styles.css (which is what shows before a theme applies).
  { id: "spor-dark", label: "Spor", mode: "dark", colors: { "bg": "#000000", "sidebar": "#0b0b0c", "card": "#111113", "raised": "#161618", "bubble": "#1c1c1f", "fg": "#f4f4f5", "fg-2": "#d4d4d8", "muted": "#a1a1aa", "faint": "#71717a", "accent": "oklch(0.6 0.2 264)", "border": "rgba(255, 255, 255, 0.08)", "border-strong": "rgba(255, 255, 255, 0.13)", "add": "#86efac", "add-bg": "rgba(34, 197, 94, 0.1)", "del": "#fca5a5", "del-bg": "rgba(239, 68, 68, 0.1)", "sky": "#7dd3fc", "emerald": "#6ee7b7", "amber": "#fcd34d", "red": "#fca5a5" } },
  {
    id: "nord",
    label: "Nord",
    mode: "dark",
    colors: {
      bg: "#2e3440",
      sidebar: "#2b303b",
      card: "#3b4252",
      raised: "#434c5e",
      bubble: "#434c5e",
      fg: "#eceff4",
      "fg-2": "#d8dee9",
      muted: "#a3acbd",
      faint: "#7b8496",
      accent: "#88c0d0",
      border: "#434c5e",
      "border-strong": "#4c566a",
      add: "#a3be8c",
      "add-bg": "#a3be8c22",
      del: "#bf616a",
      "del-bg": "#bf616a22",
      sky: "#81a1c1",
      emerald: "#a3be8c",
      amber: "#ebcb8b",
      red: "#bf616a",
    },
  },
  {
    id: "dracula",
    label: "Dracula",
    mode: "dark",
    colors: {
      bg: "#282a36",
      sidebar: "#21222c",
      card: "#303241",
      raised: "#383a4a",
      bubble: "#44475a",
      fg: "#f8f8f2",
      "fg-2": "#e2e2dc",
      muted: "#a4a8c4",
      faint: "#6272a4",
      accent: "#bd93f9",
      border: "#3c3f52",
      "border-strong": "#4b4e66",
      add: "#50fa7b",
      "add-bg": "#50fa7b1a",
      del: "#ff5555",
      "del-bg": "#ff55551a",
      sky: "#8be9fd",
      emerald: "#50fa7b",
      amber: "#f1fa8c",
      red: "#ff5555",
    },
  },
  {
    id: "midnight",
    label: "Midnight",
    mode: "dark",
    colors: {
      bg: "#0b1020",
      sidebar: "#0e1428",
      card: "#121a33",
      raised: "#17203d",
      bubble: "#1c2747",
      fg: "#e6ebff",
      "fg-2": "#c5cdf0",
      muted: "#8f9bc7",
      faint: "#5f6a96",
      accent: "#6d8bff",
      border: "#1f2a4d",
      "border-strong": "#2a3762",
      add: "#7ee2a8",
      "add-bg": "#22c55e1a",
      del: "#ff8a8a",
      "del-bg": "#ef44441a",
      sky: "#7cc7ff",
      emerald: "#7ee2a8",
      amber: "#ffd479",
      red: "#ff8a8a",
    },
  },
  { id: "spor-light", label: "Spor", mode: "light", colors: { "bg": "#fcfcfd", "sidebar": "#f6f6f7", "card": "#ffffff", "raised": "#f1f1f3", "bubble": "#ececef", "fg": "#18181b", "fg-2": "#3f3f46", "muted": "#71717a", "faint": "#a1a1aa", "accent": "oklch(0.49 0.22 264)", "border": "#e4e4e7", "border-strong": "#d4d4d8", "add": "#15803d", "add-bg": "rgba(22, 163, 74, 0.09)", "del": "#b91c1c", "del-bg": "rgba(220, 38, 38, 0.08)", "sky": "#0284c7", "emerald": "#059669", "amber": "#d97706", "red": "#dc2626" } },
  {
    id: "solarized",
    label: "Solarized",
    mode: "light",
    colors: {
      bg: "#fdf6e3",
      sidebar: "#f5efdc",
      card: "#fffbee",
      raised: "#eee8d5",
      bubble: "#eee8d5",
      fg: "#073642",
      "fg-2": "#355961",
      muted: "#657b83",
      faint: "#93a1a1",
      accent: "#268bd2",
      border: "#e4dcc3",
      "border-strong": "#d6cdb0",
      add: "#5f7f00",
      "add-bg": "#85990018",
      del: "#c0302d",
      "del-bg": "#dc322f14",
      sky: "#268bd2",
      emerald: "#859900",
      amber: "#b58900",
      red: "#dc322f",
    },
  },
  {
    id: "paper",
    label: "Paper",
    mode: "light",
    colors: {
      bg: "#f7f5f0",
      sidebar: "#efece5",
      card: "#fffefb",
      raised: "#ebe8e0",
      bubble: "#e8e4da",
      fg: "#24211c",
      "fg-2": "#47423a",
      muted: "#7a7368",
      faint: "#a39c90",
      accent: "#b0502f",
      border: "#e2ddd2",
      "border-strong": "#d3ccbe",
      add: "#3b7a2f",
      "add-bg": "#3b7a2f14",
      del: "#a83a2b",
      "del-bg": "#a83a2b12",
      sky: "#2f6f9f",
      emerald: "#3b7a2f",
      amber: "#a8740a",
      red: "#a83a2b",
    },
  },
];

/** A mode's colors: its preset's, then the user's edits. */
export function colorsOf(t: ModeTheme): Palette {
  const preset = PRESETS.find((p) => p.id === t.preset);
  return { ...(preset?.colors ?? {}), ...t.overrides };
}

// ---- The system's mode, as a store ----------------------------------------

const QUERY = "(prefers-color-scheme: dark)";
const systemListeners = new Set<() => void>();
const media = typeof window !== "undefined" ? window.matchMedia?.(QUERY) : undefined;
media?.addEventListener("change", () => systemListeners.forEach((l) => l()));

function useSystemMode(): Mode {
  return useSyncExternalStore(
    (l) => {
      systemListeners.add(l);
      return () => systemListeners.delete(l);
    },
    () => (media?.matches ? "dark" : "light"),
  );
}

// The theme editor previews the mode it edits, whatever Appearance says.
let preview: Mode | null = null;
const previewListeners = new Set<() => void>();
export function setPreviewMode(m: Mode | null) {
  preview = m;
  previewListeners.forEach((l) => l());
}
function usePreviewMode(): Mode | null {
  return useSyncExternalStore(
    (l) => {
      previewListeners.add(l);
      return () => previewListeners.delete(l);
    },
    () => preview,
  );
}

/** Light or dark, as the app is showing it now. */
export function useMode(): Mode {
  const { theme } = useSettings();
  const system = useSystemMode();
  const previewing = usePreviewMode();
  return previewing ?? (theme.appearance === "system" ? system : theme.appearance);
}

/** Put the theme on the document; call once, in the shell. */
export function useApplyTheme() {
  const { theme } = useSettings();
  const mode = useMode();
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = mode;
    // Clear what the last theme set, then set this one's.
    for (const name of [...root.style]) if (name.startsWith("--")) root.style.removeProperty(name);
    for (const [k, v] of Object.entries(colorsOf(theme[mode]))) root.style.setProperty(`--${k}`, v);
    root.style.setProperty("--radius", `${theme.radius}px`);
    root.style.setProperty("--font-size", `${theme.fontSize}px`);
  }, [theme, mode]);
}
