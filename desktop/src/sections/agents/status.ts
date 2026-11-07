// One status per task, in priority order, as in issue #30: what needs you
// first; finished and failed only until you've looked.

import type { Pull, TaskSummary } from "../../types";
import { duration } from "./transcript.ts";
import { steps } from "./progress.ts";
import { activityAt } from "../work/items.ts";

import type { Status } from "../../shared/StatusPill";

/** Not landed or closed yet. */
export function isOpen(t: TaskSummary): boolean {
  return !t.landed && !t.closed;
}

/** Once it's a pull request and you've seen the agent's work, what the PR
 * is waiting on (#24). */
function pullStatus(pr: Pull): Status {
  if (pr.state === "merged") return { label: "PR landed", tone: "violet", live: false };
  if (pr.state === "closed") return { label: "PR closed", tone: "muted", live: false };
  if (pr.checks === "failing") return { label: "Checks failing", tone: "red", live: false };
  if (pr.review === "changesrequested") return { label: "Changes requested", tone: "amber", live: false };
  if (pr.checks === "pending") return { label: "Checks running", tone: "sky", live: true };
  if (pr.review === "approved") return { label: "Approved", tone: "emerald", live: false };
  return { label: `#${pr.number}`, tone: "muted", live: false };
}

export function statusOf(t: TaskSummary, seenAt: number | undefined, nowMs: number, pull?: Pull): Status | null {
  if (!isOpen(t)) return null;
  if (t.stats.missing) return { label: "Folder gone", tone: "amber", live: false };
  const run = t.run;
  if (t.asks > 0) return { label: "Needs you", tone: "amber", live: false };
  // The repository limits agents at once, and this one's start is in line.
  if (t.waiting != null) {
    return { label: t.waiting > 1 ? `Waiting for a slot · #${t.waiting} in line` : "Waiting for a slot", tone: "muted", live: false };
  }
  if (run?.state === "running") {
    // How long, and how far through its plan (#96): "Working 12m · 5/7".
    const plan = steps(t.progress, true);
    const time = `Working ${duration(nowMs / 1000 - run.started)}`;
    return { label: plan ? `${time} · ${plan}` : time, tone: "sky", live: true };
  }
  const unseen = seenAt == null || seenAt < activityAt(t);
  if (run?.state === "failed" && unseen) return { label: "Failed", tone: "red", live: false };
  if (run?.state === "stopped" && unseen) return { label: "Stopped", tone: "muted", live: false };
  if (run?.state === "finished" && unseen) return { label: "Ready", tone: "emerald", live: false };
  return pull ? pullStatus(pull) : null;
}
