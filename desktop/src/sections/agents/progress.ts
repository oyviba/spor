// What a running agent is doing, in words (#96): how long it has run, where
// it is in its plan, and its latest action, together.

import type { Progress } from "../../types";
import { duration } from "./transcript.ts";

/** "running `pnpm test`", "editing src/auth.rs": a tool call, as it happens. */
export function action(tool: string | null, detail: string | null): string | null {
  if (!tool) return null;
  const d = detail?.trim() ?? "";
  switch (tool) {
    case "Bash":
      return d ? `running ${d}` : "running a command";
    case "BashOutput":
      return "reading a command's output";
    case "Edit":
    case "MultiEdit":
    case "Write":
    case "NotebookEdit":
      return d ? `editing ${d}` : "editing a file";
    case "Read":
    case "LS":
      return d ? `reading ${d}` : "reading";
    case "Grep":
    case "Glob":
      return d ? `searching for ${d}` : "searching the code";
    case "Task":
    case "Agent":
      return "running a subagent";
    case "WebFetch":
      return d ? `fetching ${d}` : "fetching a page";
    case "WebSearch":
      return "searching the web";
    default:
      return `using ${tool}`;
  }
}

/** "5/7" for a row, "5 of 7" for a page; null without a plan. */
export function steps(p: Progress | null, short: boolean): string | null {
  if (!p || p.total === 0) return null;
  return short ? `${p.done}/${p.total}` : `${p.done} of ${p.total}`;
}

/** The row's second line while it works: the step, or else what it's doing. */
export function doing(p: Progress | null): string | null {
  if (!p) return null;
  return p.step ?? capitalize(action(p.tool, p.detail));
}

/** The whole line: "Working 12m · 5 of 7 · Migrating the tests — running pnpm test". */
export function liveLine(p: Progress | null, seconds: number): string {
  const parts = [`Working ${duration(seconds)}`];
  const s = steps(p, false);
  if (s) parts.push(s);
  const now = action(p?.tool ?? null, p?.detail ?? null);
  if (p?.step) parts.push(now ? `${p.step} — ${now}` : p.step);
  else if (now) parts.push(capitalize(now)!);
  return parts.join(" · ");
}

function capitalize(s: string | null): string | null {
  return s ? s.charAt(0).toUpperCase() + s.slice(1) : null;
}
