// Where a link really goes (#83): a pull request, an issue or a run that
// belongs to a work item opens that item's page in Work. Only what belongs
// to none (an issue nobody has started, a run on `main`) stays in its
// browser section.

import type { TaskSummary } from "../types";
import type { Target } from "./app";

/** The first task that matches, one still open before a landed one
 * (the list is newest first). */
function pick(tasks: TaskSummary[], match: (t: TaskSummary) => boolean): TaskSummary | undefined {
  return tasks.find((t) => match(t) && !t.landed) ?? tasks.find(match);
}

/** The work item a target belongs to, if any. */
export function workItemOf(target: Target, tasks: TaskSummary[]): TaskSummary | undefined {
  switch (target.section) {
    case "pulls":
      if (target.number == null) return undefined;
      return pick(tasks, (t) => t.pull === target.number || (target.branch != null && t.branch === target.branch));
    case "issues":
      return target.number == null ? undefined : pick(tasks, (t) => t.issue === target.number);
    case "actions":
      return target.branch == null ? undefined : pick(tasks, (t) => t.branch === target.branch);
    default:
      return undefined;
  }
}

/** The target, sent to its work item's page when it has one. */
export function resolveTarget(target: Target, tasks: TaskSummary[]): Target {
  const task = workItemOf(target, tasks);
  return task ? { section: "agents", taskId: task.id } : target;
}
