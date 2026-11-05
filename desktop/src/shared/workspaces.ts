// Workspaces (#56), like Slack's: named, colored groups of repositories
// ("Work", "Side projects"), each remembering the repository last open in
// it. Kept in localStorage; the first run imports the old native app's.

import { useSyncExternalStore } from "react";

export interface Workspace {
  id: string;
  name: string;
  /** An index into WORKSPACE_COLORS. */
  color: number;
  repos: string[];
  last: string | null;
}

export interface Workspaces {
  list: Workspace[];
  active: string;
}

export const WORKSPACE_COLORS = ["#4f6bed", "#16a34a", "#d97706", "#db2777", "#7c3aed", "#0891b2", "#dc2626", "#65a30d"];

const KEY = "spor.workspaces";
const listeners = new Set<() => void>();
const newId = () => Math.random().toString(36).slice(2, 10);

function fresh(): Workspaces {
  const id = newId();
  return { list: [{ id, name: "Personal", color: 0, repos: [], last: null }], active: id };
}

function load(): Workspaces | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const w = JSON.parse(raw) as Workspaces;
    if (!w.list?.length) return null;
    if (!w.list.some((x) => x.id === w.active)) w.active = w.list[0].id;
    return w;
  } catch {
    return null;
  }
}

let current: Workspaces = load() ?? fresh();
/** Was anything saved before this run? The first run imports. */
export const firstRun = load() === null;

function save(next: Workspaces) {
  current = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // Storage blocked: keep them for this session.
  }
  listeners.forEach((l) => l());
}

export function useWorkspaces(): Workspaces {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
  );
}

export const getWorkspaces = () => current;
export const activeWorkspace = (w: Workspaces = current) => w.list.find((x) => x.id === w.active) ?? w.list[0];

/** "Side projects" → "SP", "work" → "W". */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  const letters = words.length === 1 ? words[0][0] : words[0][0] + words[words.length - 1][0];
  return letters.toUpperCase();
}

/** The repository to open when switching to a workspace. */
export function landing(ws: Workspace): string | null {
  return ws.last && ws.repos.includes(ws.last) ? ws.last : (ws.repos[0] ?? null);
}

const update = (change: (w: Workspaces) => Workspaces) => save(change(current));
const mapWs = (id: string, f: (ws: Workspace) => Workspace) => (w: Workspaces) => ({
  ...w,
  list: w.list.map((x) => (x.id === id ? f(x) : x)),
});

/**
 * A repository was opened. It stays in the workspace that has it, which
 * becomes the active one; a new one joins the active workspace. Either
 * way it's that workspace's last-open repository now.
 */
export function noteOpened(path: string) {
  update((w) => {
    const owner = w.list.find((x) => x.repos.includes(path)) ?? activeWorkspace(w);
    return {
      active: owner.id,
      list: w.list.map((x) =>
        x.id === owner.id ? { ...x, last: path, repos: x.repos.includes(path) ? x.repos : [...x.repos, path] } : x,
      ),
    };
  });
}

export function switchTo(id: string) {
  update((w) => ({ ...w, active: id }));
}

export function create(name: string): Workspace {
  const ws: Workspace = { id: newId(), name, color: current.list.length % WORKSPACE_COLORS.length, repos: [], last: null };
  update((w) => ({ list: [...w.list, ws], active: ws.id }));
  return ws;
}

export function rename(id: string, name: string) {
  update(mapWs(id, (x) => ({ ...x, name })));
}

export function recolor(id: string, color: number) {
  update(mapWs(id, (x) => ({ ...x, color })));
}

/** Delete a workspace; its repositories go to the first remaining one. */
export function remove(id: string) {
  update((w) => {
    if (w.list.length === 1) return w;
    const gone = w.list.find((x) => x.id === id);
    const rest = w.list.filter((x) => x.id !== id);
    const heir = rest[0];
    const list = rest.map((x) =>
      x.id === heir.id && gone ? { ...x, repos: [...x.repos, ...gone.repos.filter((r) => !x.repos.includes(r))] } : x,
    );
    return { list, active: w.active === id ? heir.id : w.active };
  });
}

export function moveRepo(path: string, to: string) {
  update((w) => ({
    ...w,
    list: w.list.map((x) => {
      if (x.id === to) return x.repos.includes(path) ? x : { ...x, repos: [...x.repos, path] };
      return x.repos.includes(path) ? { ...x, repos: x.repos.filter((r) => r !== path), last: x.last === path ? null : x.last } : x;
    }),
  }));
}

export function removeRepo(path: string) {
  update((w) => ({
    ...w,
    list: w.list.map((x) => ({ ...x, repos: x.repos.filter((r) => r !== path), last: x.last === path ? null : x.last })),
  }));
}

/** Replace everything (the first run's import). */
export function replaceAll(next: Workspaces) {
  save(next);
}

/** For imports: workspaces from a list of names, colors and repositories. */
export function fromList(list: { name: string; color: number; repos: string[]; last: string | null }[], active: number): Workspaces {
  const withIds = list.map((x) => ({ ...x, id: newId(), color: x.color % WORKSPACE_COLORS.length }));
  return { list: withIds, active: withIds[Math.min(active, withIds.length - 1)].id };
}
