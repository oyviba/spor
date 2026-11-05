// What every section gets from the shell: the repository and its live
// state, toasts, the shared undo, and a way to send the user to another
// section ("open this run in Actions").

import { createContext, useContext } from "react";
import type { RepoInfo, RepoState } from "../types";
import type { MenuItem } from "./Overlay";

export type SectionId = "changes" | "history" | "issues" | "pulls" | "actions" | "agents";

/** Where to go: a section, and what to open there. */
export type Target =
  | { section: "changes"; path?: string }
  | { section: "history"; hash?: string; branch?: string }
  | { section: "issues"; number?: number }
  | { section: "pulls"; number?: number }
  | { section: "actions"; runId?: number; jobId?: number }
  | { section: "agents"; taskId?: string };

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Shell {
  repo: RepoInfo;
  state: RepoState | null;
  /** Re-read the repository's state now (after a change). */
  refresh: () => Promise<void>;
  toast: (text: string, tone?: "ok" | "error", undo?: boolean, action?: ToastAction) => void;
  fail: (e: unknown) => void;
  /**
   * Run a change, then toast its message with Undo and refresh. Resolves to
   * whether it worked.
   */
  act: (run: () => Promise<string | void>, opts?: { undo?: boolean }) => Promise<boolean>;
  /**
   * Switch branches. When uncommitted changes are in the way, the error
   * toast offers to stash them and switch anyway.
   */
  switchBranch: (run: (stash: boolean) => Promise<string>) => Promise<boolean>;
  undo: () => void;
  go: (target: Target) => void;
  /** The last target sent to a section, with a counter so a repeat still lands. */
  target: Target & { seq: number };
  openRepo: (path: string) => void;
  /** Repositories opened before, most recent first (this one included). */
  recent: string[];
  forgetRepo: (path: string) => void;
  /** A context menu at the pointer (right-click or a ⋯ button). */
  menu: (e: { clientX: number; clientY: number; preventDefault?: () => void }, items: MenuItem[]) => void;
  /** Ask for a name; null when cancelled. */
  prompt: (spec: {
    title: string;
    detail?: string;
    placeholder?: string;
    initial?: string;
    action: string;
    extra?: { placeholder: string };
  }) => Promise<{ value: string; extra: string } | null>;
  /** Ask before something that can't be undone. */
  confirm: (spec: { title: string; detail?: string; action: string; danger?: boolean }) => Promise<boolean>;
}

export const ShellContext = createContext<Shell | null>(null);

export function useShell(): Shell {
  const shell = useContext(ShellContext);
  if (!shell) throw new Error("useShell outside the app shell");
  return shell;
}
