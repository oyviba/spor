// One status per task, in priority order, as in issue #30: what needs you
// first; finished and failed only until you've looked.

import type { TaskSummary } from "../../types";
import { duration } from "./transcript";

import type { Status } from "../../shared/StatusPill";

/** When the task last did something you haven't seen, for "unseen". */
export function activityAt(t: TaskSummary): number {
  return t.run?.ended ?? t.run?.started ?? t.created;
}

export function statusOf(t: TaskSummary, seenAt: number | undefined, nowMs: number): Status | null {
  if (t.landed) return null;
  if (t.stats.missing) return { label: "Folder gone", tone: "amber", live: false };
  const run = t.run;
  if (t.asks > 0) return { label: "Needs you", tone: "amber", live: false };
  if (run?.state === "running")
    return { label: `Working ${duration(nowMs / 1000 - run.started)}`, tone: "sky", live: true };
  const unseen = seenAt == null || seenAt < activityAt(t);
  if (run?.state === "failed" && unseen) return { label: "Failed", tone: "red", live: false };
  if (run?.state === "stopped" && unseen) return { label: "Stopped", tone: "muted", live: false };
  if (run?.state === "finished" && unseen) return { label: "Ready", tone: "emerald", live: false };
  return null;
}
