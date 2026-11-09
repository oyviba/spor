// What the agent's details pane shows. The pane keeps to what you act on:
// the work card, what needs you, the changes and the next step. The rest
// of a run (cost, model, tools that didn't connect, the setup script) is a
// menu away, in "Run details". A few lines are yours to turn on or off.

import type { TaskDetail } from "../../types";
import { checksLabel, duration, latestChecks, runs as toRuns, tokens } from "./transcript.ts";

/** The lines of the pane you can turn on or off. */
export interface PaneOptions {
  /** The agent's debrief: what it changed, risks, open questions. */
  debrief: boolean;
  /** What the agent ran to check itself: "Tests ✓ · Build ✓". */
  checks: boolean;
  /** The run in a line: runs, how long, what it cost. */
  summary: boolean;
}

export const DEFAULT_PANE: PaneOptions = { debrief: true, checks: true, summary: false };

export const PANE_LINES: { id: keyof PaneOptions; label: string; hint: string }[] = [
  { id: "debrief", label: "Debrief", hint: "What the agent says it changed, what to look at, and what it asks of you" },
  { id: "checks", label: "What the agent checked", hint: "Whether it ran the tests and the build, read from its commands" },
  { id: "summary", label: "Run summary", hint: "How many runs, how long the last took, what it cost" },
];

/** Facts about a task's runs, for the pane's optional lines and the run details. */
export interface RunFacts {
  runs: number;
  /** "Claude · 3 runs · last 4m · $0.42 · 12.3k tokens" */
  summary: string | null;
  /** "Tests ✓ · Build ✓" */
  checks: string | null;
  model: string | null;
  /** MCP servers the last run couldn't reach. */
  mcpFailed: string[];
}

export function runFacts(detail: TaskDetail, running: boolean): RunFacts {
  const last = detail.runs.at(-1);
  if (!last) return { runs: 0, summary: null, checks: null, model: null, mcpFailed: [] };
  const views = toRuns(detail.transcript, running);
  const spent = detail.runs.reduce((n, r) => n + (r.cost_usd ?? 0), 0);
  const used = detail.runs.reduce((n, r) => n + r.tokens_in + r.tokens_out, 0);
  const summary =
    `${last.label} · ${detail.runs.length} ${detail.runs.length === 1 ? "run" : "runs"}` +
    (last.ended ? ` · last ${duration(last.ended - last.started)}` : "") +
    (spent > 0 ? ` · $${spent.toFixed(2)}` : "") +
    (used > 0 ? ` · ${tokens(used)} tokens` : "");
  return {
    runs: detail.runs.length,
    summary,
    checks: checksLabel(
      latestChecks(views),
      views.some((r) => r.edited),
    ),
    model: last.model,
    mcpFailed: last.mcp_failed,
  };
}
