// The work page (#80): one page for any work item, an agent's task, a pull
// request (from a task, a teammate or you) or your own branch. The header
// has the item's stages as a track with the current one lit, who it's
// waiting on, and what to do; the tabs are only the ones with something in
// them:
//
//   Agent       the transcript and composer (a task)
//   Discussion  the pull request's conversation, reviews, comment / approve
//   Changes     the diff against the base (a task's includes uncommitted work)
//   Checks      the pull request's checks, or the Actions runs on the branch
//
// What fills each tab comes from the view for the item's kind (TaskView,
// PullView, BranchPage); this is the frame they share.

import type { ReactNode } from "react";
import type { WorkItemInfo } from "../../types";
import { PanelRight } from "../../shared/Icons";
import { StatusPill, type Status } from "../../shared/StatusPill";
import { Tabs } from "../../shared/Layout";
import { waitingText } from "../../shared/work";

export type WorkTab = "agent" | "discussion" | "changes" | "checks";

export const TAB_LABELS: Record<WorkTab, string> = {
  agent: "Agent",
  discussion: "Discussion",
  changes: "Changes",
  checks: "Checks",
};

const TRACK: { stage: WorkItemInfo["stage"]; label: string }[] = [
  { stage: "planned", label: "Planned" },
  { stage: "inprogress", label: "In progress" },
  { stage: "inreview", label: "In review" },
  { stage: "landed", label: "Landed" },
];

/** Planned → In progress → In review → Landed, with the current stage lit
 * and the ones before it done. A closed item ends in Closed instead. */
export function StageTrack({ stage }: { stage: WorkItemInfo["stage"] }) {
  const steps = stage === "closed" ? [...TRACK.slice(0, 3), { stage: "closed" as const, label: "Closed" }] : TRACK;
  const at = steps.findIndex((s) => s.stage === stage);
  return (
    <ol className="stage-track" aria-label="Stage">
      {steps.map((s, i) => (
        <li
          key={s.stage}
          className={i < at ? "done" : i === at ? `on${stage === "closed" ? " closed" : ""}` : ""}
          aria-current={i === at ? "step" : undefined}
        >
          {s.label}
        </li>
      ))}
    </ol>
  );
}

/** The work page's header: title and status, the stage track and who it's
 * waiting on, the main action, and whatever else goes on the right. Changes
 * shows it alone for your checkout's branch (#82). */
export function WorkHeader({
  item,
  title,
  hint,
  status,
  action,
  children,
}: {
  item: WorkItemInfo;
  title: ReactNode;
  /** The title's tooltip: a task's whole prompt, say. */
  hint?: string;
  status?: Status | null;
  action?: ReactNode;
  children?: ReactNode;
}) {
  const waiting = waitingText(item);
  return (
    <header className="task-header work-header">
      <div className="work-heading">
        <div className="task-title">
          <h2 title={hint}>{title}</h2>
          {status && <StatusPill status={status} />}
        </div>
        <div className="work-progress">
          <StageTrack stage={item.stage} />
          {waiting && <span className="muted small-text">{waiting}</span>}
        </div>
      </div>
      {action && <div className="work-action">{action}</div>}
      {children}
    </header>
  );
}

export function WorkPage({
  item,
  title,
  hint,
  status,
  action,
  tabs,
  tab,
  onTab,
  inspector,
  onInspector,
  details,
  dock,
  children,
}: {
  item: WorkItemInfo;
  title: ReactNode;
  hint?: string;
  status?: Status | null;
  action?: ReactNode;
  /** The tabs with something in them, and an extra label for each (a count). */
  tabs: { id: WorkTab; extra?: ReactNode }[];
  tab: WorkTab;
  onTab: (t: WorkTab) => void;
  inspector: boolean;
  onInspector?: (open: boolean) => void;
  /** The details card. */
  details: ReactNode;
  /** Under the page: the composer, or the comment box. */
  dock?: ReactNode;
  /** The open tab. */
  children: ReactNode;
}) {
  return (
    <div className={`task-page${inspector ? " with-inspector" : ""}`}>
      <WorkHeader item={item} title={title} hint={hint} status={status} action={action}>
        {tabs.length > 1 && (
          <Tabs
            value={tab}
            onChange={onTab}
            tabs={tabs.map((t) => ({
              id: t.id,
              label: (
                <>
                  {TAB_LABELS[t.id]}
                  {t.extra}
                </>
              ),
            }))}
          />
        )}
        {onInspector && (
          <button
            className={`icon-button${inspector ? " active" : ""}`}
            onClick={() => onInspector(!inspector)}
            title="Details (⌘.)"
            aria-label="Toggle details"
          >
            <PanelRight />
          </button>
        )}
      </WorkHeader>
      <div className="task-body">
        {children}
        {inspector && details}
      </div>
      {dock}
    </div>
  );
}

/** The tab to show: the one asked for if the page has it, else its first. */
export function shownTab(tabs: { id: WorkTab }[], wanted: WorkTab): WorkTab {
  return tabs.some((t) => t.id === wanted) ? wanted : (tabs[0]?.id ?? "changes");
}
