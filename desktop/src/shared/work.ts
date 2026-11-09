// Work items as every inspector shows them (#79): which item a task, pull
// request, issue or run belongs to, what to show when the backend's list
// doesn't have it (a closed pull request, say), and the one next step for
// its stage. The words are docs/concepts.md's.

import type { Pull, TaskSummary, WorkItemInfo } from "../types";
import type { Status } from "./StatusPill";
import type { SectionId, Target } from "./app";

/** What an inspector knows about the thing it shows. */
export interface WorkRef {
  task?: string;
  pull?: number;
  branch?: string;
  issue?: number;
}

/** The work item `ref` belongs to: by task, then pull request, then branch,
 * then issue (a branch's item before a Planned one for the same issue). */
export function findWork(items: WorkItemInfo[], ref: WorkRef): WorkItemInfo | undefined {
  if (ref.task != null) {
    const hit = items.find((i) => i.task === ref.task);
    if (hit) return hit;
  }
  if (ref.pull != null) {
    const hit = items.find((i) => i.pull?.number === ref.pull);
    if (hit) return hit;
  }
  if (ref.branch != null) {
    const hit = items.find((i) => i.branch === ref.branch);
    if (hit) return hit;
  }
  if (ref.issue != null) {
    const linked = items.filter((i) => i.issue === ref.issue);
    return linked.find((i) => i.branch != null) ?? linked[0];
  }
  return undefined;
}

const blank = (key: string): WorkItemInfo => ({
  key,
  title: "",
  branch: null,
  base: null,
  worktree: null,
  task: null,
  issue: null,
  pull: null,
  checks: null,
  owner: { kind: "you" },
  stage: "inprogress",
  waiting_on: null,
  updated: 0,
});

function pullChecks(p: Pull): WorkItemInfo["checks"] {
  return p.checks === "passing" ? "passed" : p.checks === "failing" ? "failed" : p.checks === "pending" ? "running" : null;
}

/** A pull request's item, when the list doesn't have it (closed ones
 * aren't listed). */
export function itemFromPull(p: Pull, me?: string): WorkItemInfo {
  return {
    ...blank(`branch:${p.head}`),
    title: p.title,
    branch: p.head,
    base: p.base,
    pull: { number: p.number, title: p.title, state: p.state as never, review: p.review as never, author: p.author, url: p.url },
    checks: pullChecks(p),
    owner: me && p.author !== me ? { kind: "teammate", login: p.author } : { kind: "you" },
    stage: p.state === "merged" ? "landed" : p.state === "closed" ? "closed" : p.state === "draft" ? "inprogress" : "inreview",
  };
}

/** A task's item, before the list has caught up with it. */
export function itemFromTask(t: TaskSummary, pull?: Pull): WorkItemInfo {
  const settled = t.landed != null || t.closed;
  return {
    ...(pull ? itemFromPull(pull) : blank(`branch:${t.branch}`)),
    title: t.title,
    branch: t.branch,
    base: t.base,
    worktree: settled ? null : t.worktree,
    task: t.id,
    issue: t.issue,
    owner: t.run ? { kind: "agent", name: t.run.agent === "claude" ? "Claude Code" : "Agent" } : { kind: "you" },
    stage: t.landed != null ? "landed" : t.closed ? "closed" : pull?.state === "open" ? "inreview" : "inprogress",
  };
}

/** An issue's item when nothing is linked to it: Planned while open. */
export function itemFromIssue(number: number, title: string, open: boolean): WorkItemInfo {
  return { ...blank(`issue:${number}`), title, issue: number, stage: open ? "planned" : "closed" };
}

/** A branch's item when the list doesn't have it (a run on `main`). */
export function itemFromBranch(branch: string): WorkItemInfo {
  return { ...blank(`branch:${branch}`), title: branch, branch };
}

const STAGES: Record<WorkItemInfo["stage"], Status> = {
  planned: { label: "Planned", tone: "muted", live: false },
  inprogress: { label: "In progress", tone: "sky", live: false },
  inreview: { label: "In review", tone: "amber", live: false },
  landed: { label: "Landed", tone: "violet", live: false },
  closed: { label: "Closed", tone: "muted", live: false },
};

export function stageStatus(item: WorkItemInfo): Status {
  const s = STAGES[item.stage];
  return { ...s, live: item.waiting_on?.who === "agent" || item.waiting_on?.who === "ci" };
}

const WHY: Record<NonNullable<NonNullable<WorkItemInfo["waiting_on"]>["why"]>, string> = {
  question: "the agent asked something",
  unseen: "the agent finished",
  review: "your review is requested",
  checksfailed: "checks failed",
  changesrequested: "changes were requested",
  ready: "approved and ready to land",
};

/** Who's next, in a few words: "Waiting on you: checks failed". */
export function waitingText(item: WorkItemInfo): string | null {
  const w = item.waiting_on;
  if (!w) return null;
  switch (w.who) {
    case "you":
      return w.why ? `Waiting on you: ${WHY[w.why]}` : "Waiting on you";
    case "agent":
      return item.owner.kind === "agent" ? `Waiting on ${item.owner.name}` : "Waiting on the agent";
    case "teammate":
      return item.owner.kind === "teammate" ? `Waiting on ${item.owner.login}` : "Waiting on a reviewer";
    case "ci":
      return "Waiting on checks";
  }
}

export interface Step {
  label: string;
  target: Target;
  /** Open the browser section itself, not the work item. */
  browse?: boolean;
}

/** Where a target goes, to tell whether a step would go nowhere. */
function sameSection(t: Target, from: SectionId | undefined): boolean {
  return from != null && t.section === from;
}

/**
 * The one thing to do next, as a step to where it's done: Start, Publish,
 * Land, answering or reviewing. `from` is the section showing the card; a
 * step that would only lead back there is left out, since that section
 * shows its own buttons for it.
 */
export function nextStep(item: WorkItemInfo, from?: SectionId, current?: string | null): Step | null {
  const task = item.task;
  const pr = item.pull;
  const w = item.waiting_on;
  const step = ((): Step | null => {
    if (item.stage === "planned" && item.issue != null) return { label: "Start", target: { section: "issues", number: item.issue }, browse: true };
    if (item.stage === "landed" || item.stage === "closed") return null;
    if (w?.who === "you") {
      switch (w.why) {
        case "question":
          return task ? { label: "Answer the agent", target: { section: "work", taskId: task } } : null;
        case "unseen":
          return task ? { label: "Review the agent's work", target: { section: "work", taskId: task } } : null;
        case "review":
          return pr ? { label: `Review #${pr.number}`, target: { section: "pulls", number: pr.number }, browse: true } : null;
        case "checksfailed":
          return item.branch ? { label: "See failing checks", target: { section: "actions", branch: item.branch }, browse: true } : null;
        case "changesrequested":
          return task
            ? { label: "Send review to agent", target: { section: "work", taskId: task } }
            : pr
              ? { label: "See requested changes", target: { section: "pulls", number: pr.number }, browse: true }
              : null;
        case "ready":
          return pr
            ? task
              ? { label: `Land #${pr.number}`, target: { section: "work", taskId: task } }
              : { label: `Land #${pr.number}`, target: { section: "pulls", number: pr.number }, browse: true }
            : null;
      }
    }
    // Your work with nothing published yet.
    if (!pr && item.branch && item.owner.kind !== "teammate") {
      if (task) return { label: "Publish", target: { section: "work", taskId: task } };
      if (current === item.branch) return { label: "Publish", target: { section: "history", working: true } };
    }
    return null;
  })();
  if (!step) return null;
  return sameSection(step.target, from) ? null : step;
}
