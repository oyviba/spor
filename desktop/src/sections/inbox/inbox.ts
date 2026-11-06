// The Inbox's rows (#42): what's waiting for you in every known repository,
// worked out from what the sections already load. Nothing is stored, so a
// row goes away as soon as the thing behind it is dealt with.

import type { InboxGithub, InboxTask } from "../../types";
import type { Target } from "../../shared/app";

export type InboxKind = "ask" | "failed" | "ready" | "review";

export interface InboxItem {
  key: string;
  kind: InboxKind;
  /** The repository's root. */
  repo: string;
  title: string;
  detail: string;
  /** Unix seconds: since when it's been waiting. */
  since: number;
  /** Where it opens, in its repository. */
  target: Target;
}

/** The groups, in the order they're shown: what blocks an agent first. */
export const GROUPS: { kind: InboxKind; label: string }[] = [
  { kind: "ask", label: "Waiting on an answer" },
  { kind: "failed", label: "Failed runs" },
  { kind: "ready", label: "Ready for review" },
  { kind: "review", label: "Review requested" },
];

/** When the task last did something, as the Agents section counts "seen". */
export function taskActivity(t: InboxTask): number {
  return t.ended ?? t.started ?? t.created;
}

/**
 * The rows: agents asking a question, finished (or failed) agent runs you
 * haven't looked at, review requests and failing runs on your branches.
 * `seen` is the Agents section's record of when you last looked at each
 * task.
 */
export function inboxItems(
  tasks: InboxTask[],
  github: InboxGithub | null,
  seen: Record<string, number>,
): InboxItem[] {
  const items: InboxItem[] = [];
  for (const t of tasks) {
    const target: Target = { section: "agents", taskId: t.id };
    if (t.asks > 0)
      items.push({
        key: `ask:${t.repo}:${t.id}`,
        kind: "ask",
        repo: t.repo,
        title: t.title,
        detail: t.asks === 1 ? "The agent is asking before it goes on" : `The agent has ${t.asks} questions`,
        since: t.started ?? t.created,
        target,
      });
    else if ((t.run === "finished" || t.run === "failed") && (seen[t.id] ?? 0) < taskActivity(t))
      items.push({
        key: `ready:${t.repo}:${t.id}`,
        kind: "ready",
        repo: t.repo,
        title: t.title,
        detail: t.run === "failed" ? "The agent's run failed" : "The agent finished",
        since: taskActivity(t),
        target,
      });
  }
  for (const r of github?.runs ?? [])
    items.push({
      key: `failed:${r.repo}:${r.id}`,
      kind: "failed",
      repo: r.repo,
      title: r.title,
      detail: `${r.workflow} failed on ${r.branch}`,
      since: r.updated,
      // On a work item's branch it opens the item (#83).
      target: { section: "actions", runId: r.id, branch: r.branch },
    });
  for (const p of github?.reviews ?? [])
    items.push({
      key: `review:${p.repo}:${p.number}`,
      kind: "review",
      repo: p.repo,
      title: p.title,
      detail: `#${p.number} by ${p.author}`,
      since: p.updated,
      target: { section: "pulls", number: p.number },
    });
  const order = GROUPS.map((g) => g.kind);
  return items.sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind) || b.since - a.since);
}
