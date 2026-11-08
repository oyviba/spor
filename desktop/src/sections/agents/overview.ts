// The Agents view: every agent task in every repository of every workspace,
// grouped by what it's doing or by where it lives. Worked out from the same
// cheap poll as the Inbox, so it adds no reads of its own.

import type { InboxTask } from "../../types.ts";
import { askLine, taskActivity } from "../inbox/inbox.ts";

export type AgentState = "asking" | "working" | "ready" | "failed" | "idle";

/** The states, in the order they're shown: what blocks an agent first. */
export const STATES: { state: AgentState; label: string; tone: "amber" | "sky" | "emerald" | "red" | "muted" }[] = [
  { state: "asking", label: "Needs you", tone: "amber" },
  { state: "working", label: "Working", tone: "sky" },
  { state: "ready", label: "Ready for review", tone: "emerald" },
  { state: "failed", label: "Failed or stopped", tone: "red" },
  { state: "idle", label: "Idle", tone: "muted" },
];

/** One task's state. Finished, failed and stopped count only until you've
 * looked (`seen`, Work's record by task id); after that it's idle. */
export function agentState(t: InboxTask, seen: Record<string, number>): AgentState {
  if (t.asks > 0) return "asking";
  if (t.run === "running") return "working";
  const unseen = (seen[t.id] ?? 0) < taskActivity(t);
  if (t.run === "finished" && unseen) return "ready";
  if ((t.run === "failed" || t.run === "stopped") && unseen) return "failed";
  return "idle";
}

/** The line under a task's title. */
export function agentDetail(t: InboxTask, state: AgentState): string {
  switch (state) {
    case "asking":
      return askLine(t.ask, t.asks);
    case "working":
      return "Working";
    case "ready":
      return t.reason ? `Finished · ${t.reason}` : "Finished";
    case "failed":
      return t.run === "stopped" ? "Stopped" : "The run failed";
    case "idle":
      return t.run == null ? "Not started" : t.run === "finished" ? "Finished, looked at" : `Last run ${t.run}`;
  }
}

export interface AgentRow {
  task: InboxTask;
  state: AgentState;
  detail: string;
  /** Unix seconds: when it last did something. */
  at: number;
}

export interface AgentGroup {
  key: string;
  label: string;
  rows: AgentRow[];
}

const name = (root: string) => root.split("/").pop() ?? root;

/** The rows, grouped by state (in STATES' order) or by repository (by name),
 * newest first within each group. Empty groups are left out. */
export function agentGroups(tasks: InboxTask[], seen: Record<string, number>, by: "state" | "repo"): AgentGroup[] {
  const rows: AgentRow[] = tasks
    .map((task) => {
      const state = agentState(task, seen);
      return { task, state, detail: agentDetail(task, state), at: taskActivity(task) };
    })
    .sort((a, b) => b.at - a.at);
  if (by === "state")
    return STATES.map((s) => ({ key: s.state, label: s.label, rows: rows.filter((r) => r.state === s.state) })).filter(
      (g) => g.rows.length > 0,
    );
  const repos = [...new Set(rows.map((r) => r.task.repo))].sort((a, b) => name(a).localeCompare(name(b)));
  // In a repository, what needs you still comes first.
  const order = STATES.map((s) => s.state);
  return repos.map((repo) => ({
    key: repo,
    label: name(repo),
    rows: rows.filter((r) => r.task.repo === repo).sort((a, b) => order.indexOf(a.state) - order.indexOf(b.state) || b.at - a.at),
  }));
}

/** How many tasks are in each state, per repository, for the overview. */
export function countsByRepo(tasks: InboxTask[], seen: Record<string, number>): { repo: string; counts: Record<AgentState, number> }[] {
  const out = new Map<string, Record<AgentState, number>>();
  for (const t of tasks) {
    const c = out.get(t.repo) ?? { asking: 0, working: 0, ready: 0, failed: 0, idle: 0 };
    c[agentState(t, seen)]++;
    out.set(t.repo, c);
  }
  return [...out].map(([repo, counts]) => ({ repo, counts })).sort((a, b) => name(a.repo).localeCompare(name(b.repo)));
}
