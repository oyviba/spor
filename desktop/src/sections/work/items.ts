// Every piece of work in the repository as one list (#81): agents' tasks,
// your own branches, and open pull requests from anyone, each once.
// A task's PR belongs to the task; a branch's PR to the branch.

import type { Pull, TaskPull, TaskSummary, WorkBranch } from "../../types";

export type Owner = "mine" | "agent" | "teammate";
/** Where an item sits in the list: what needs you, then by stage. */
export type Shelf = "needs" | "progress" | "review" | "settled";
export type Filter = "all" | "mine" | "agents" | "teammates";

interface Common {
  /** Unique and stable: `task:<id>`, `branch:<name>`, `pull:<n>`. */
  key: string;
  owner: Owner;
  shelf: Shelf;
  title: string;
  /** When it started, for its place in the list. */
  since: number;
  /** When it last changed, for "unseen". */
  updated: number;
}

export type WorkItem = Common &
  (
    | { kind: "task"; task: TaskSummary; pull?: Pull }
    | { kind: "branch"; branch: WorkBranch; pull?: Pull }
    | { kind: "pull"; pull: Pull }
  );

export interface Sources {
  tasks: TaskSummary[];
  /** Tasks' pull requests, by task id. */
  taskPulls: Record<string, TaskPull>;
  branches: WorkBranch[];
  /** Open pull requests. */
  pulls: Pull[];
  /** Numbers of open pull requests you opened. */
  yours: Set<number>;
  /** Numbers of open pull requests waiting on your review. */
  review: Set<number>;
  /** When you last looked at each item, by key: what it said then. */
  seen: Record<string, number>;
}

/** When a task last did something you may not have seen. */
export function activityAt(t: TaskSummary): number {
  return t.run?.ended ?? t.run?.started ?? t.created;
}

const open = (p?: Pull) => p != null && (p.state === "open" || p.state === "draft");
const troubled = (p: Pull) => p.review === "changesrequested" || p.checks === "failing";

function taskShelf(t: TaskSummary, pull: Pull | undefined, seenAt: number | undefined): Shelf {
  if (t.landed || pull?.state === "merged" || pull?.state === "closed") return "settled";
  if (t.asks > 0) return "needs";
  if (t.run?.state === "running") return "progress";
  const unseen = seenAt == null || seenAt < activityAt(t);
  if (unseen && (t.run?.state === "finished" || t.run?.state === "failed")) return "needs";
  if (open(pull)) return troubled(pull!) ? "needs" : "review";
  return "progress";
}

export function workItems(s: Sources): WorkItem[] {
  const items: WorkItem[] = [];
  const claimed = new Set<number>();
  const pullFor = (branch: string) => s.pulls.find((p) => p.head === branch);

  for (const task of s.tasks) {
    const pull = s.taskPulls[task.id]?.pull ?? pullFor(task.branch);
    if (pull) claimed.add(pull.number);
    items.push({
      kind: "task",
      key: `task:${task.id}`,
      owner: "agent",
      shelf: taskShelf(task, pull, s.seen[`task:${task.id}`]),
      title: task.title,
      since: task.created,
      updated: activityAt(task),
      task,
      pull,
    });
  }

  const taskBranches = new Set(s.tasks.map((t) => t.branch));
  for (const branch of s.branches) {
    if (taskBranches.has(branch.name)) continue;
    const pull = pullFor(branch.name);
    if (pull) claimed.add(pull.number);
    const owner: Owner = pull && !s.yours.has(pull.number) ? "teammate" : "mine";
    const needs = pull && (s.review.has(pull.number) || (owner === "mine" && troubled(pull)));
    items.push({
      kind: "branch",
      key: `branch:${branch.name}`,
      owner,
      shelf: needs ? "needs" : pull ? "review" : "progress",
      title: pull?.title ?? branch.commits[branch.commits.length - 1]?.subject ?? branch.name,
      since: branch.commits[branch.commits.length - 1]?.time ?? branch.updated,
      updated: Math.max(branch.updated, pull?.updated ?? 0),
      branch,
      pull,
    });
  }

  for (const pull of s.pulls) {
    if (claimed.has(pull.number)) continue;
    const owner: Owner = s.yours.has(pull.number) ? "mine" : "teammate";
    const needs = s.review.has(pull.number) || (owner === "mine" && troubled(pull));
    items.push({
      kind: "pull",
      key: `pull:${pull.number}`,
      owner,
      shelf: needs ? "needs" : "review",
      title: pull.title,
      since: pull.updated,
      updated: pull.updated,
      pull,
    });
  }
  return items;
}

export function matches(item: WorkItem, filter: Filter): boolean {
  if (filter === "all") return true;
  if (filter === "mine") return item.owner === "mine";
  if (filter === "agents") return item.owner === "agent";
  return item.owner === "teammate";
}

/** Has it changed since you last opened it? */
export function unseen(item: WorkItem, seen: Record<string, number>): boolean {
  const at = seen[item.key];
  return at == null || at < item.updated;
}

/**
 * The items by shelf, each in the order you keep: `order` holds a rank per
 * key (higher is nearer the top), and an item not ranked yet goes by when
 * it started. Activity never moves an item within its shelf.
 */
export function shelve(items: WorkItem[], order: Record<string, number>): Record<Shelf, WorkItem[]> {
  const rank = (i: WorkItem) => order[i.key] ?? i.since;
  const sorted = [...items].sort((a, b) => rank(b) - rank(a) || a.key.localeCompare(b.key));
  const out: Record<Shelf, WorkItem[]> = { needs: [], progress: [], review: [], settled: [] };
  for (const i of sorted) out[i.shelf].push(i);
  return out;
}

/** Ranks that put `key` above (or below) everything else. */
export function moveTo(items: WorkItem[], order: Record<string, number>, key: string, top: boolean): Record<string, number> {
  const ranks = items.filter((i) => i.key !== key).map((i) => order[i.key] ?? i.since);
  if (ranks.length === 0) return order;
  return { ...order, [key]: top ? Math.max(...ranks) + 1 : Math.min(...ranks) - 1 };
}
