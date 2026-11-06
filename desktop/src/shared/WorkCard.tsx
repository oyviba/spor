// The work card (#79): one work item's whole chain, the same in every
// inspector — issue → branch · folder → pull request → checks — with its
// stage, who it waits on, and the one next step. Each link goes where that
// part lives. What only one section does (landing options, re-runs,
// labels) stays in that section's own card, below this one.

import type { ReactNode } from "react";
import { api } from "../api";
import type { WorkItemInfo } from "../types";
import { useShell, type SectionId, type Target } from "./app";
import { Button } from "./Button";
import { CiIcon } from "./ci";
import { Bot, Branch, Folder, IssueIcon, PullRequest, Users } from "./Icons";
import { StatusPill, type Status } from "./StatusPill";
import { nextStep, stageStatus, waitingText } from "./work";

function pullPill(p: NonNullable<WorkItemInfo["pull"]>): Status {
  if (p.state === "merged") return { label: "Landed", tone: "violet", live: false };
  if (p.state === "closed") return { label: "Closed", tone: "muted", live: false };
  if (p.review === "changesrequested") return { label: "Changes requested", tone: "amber", live: false };
  if (p.state === "draft") return { label: "Draft", tone: "muted", live: false };
  if (p.review === "approved") return { label: "Approved", tone: "emerald", live: false };
  return { label: "Open", tone: "sky", live: false };
}

const CHECKS = {
  passed: { ci: "success", label: "Checks passed" },
  failed: { ci: "failure", label: "Checks failed" },
  running: { ci: "running", label: "Checks running" },
} as const;

/** A row that links somewhere, unless it's where the card is shown. */
function Row({
  here,
  to,
  onClick,
  title,
  className = "",
  children,
}: {
  here: boolean;
  to?: Target;
  onClick?: () => void;
  title?: string;
  className?: string;
  children: ReactNode;
}) {
  const shell = useShell();
  if (here || (!to && !onClick))
    return (
      <div className={`inspector-row ${className}`} title={title}>
        {children}
      </div>
    );
  return (
    <button
      className={`inspector-row action ${className}`}
      title={title}
      onClick={onClick ?? (() => shell.go(to!, { browse: to!.section !== "work" }))}
    >
      {children}
    </button>
  );
}

export function WorkCard({
  item,
  from,
  actions = true,
}: {
  item: WorkItemInfo;
  /** The section showing the card: its own part isn't a link, and a next
   * step back to it is left to its own buttons. */
  from: SectionId;
  /** Show the next step (off where the section's own buttons are it). */
  actions?: boolean;
}) {
  const shell = useShell();
  const waiting = waitingText(item);
  const step = actions ? nextStep(item, from, shell.state?.branch) : null;
  const pr = item.pull;
  const checks = item.checks ? CHECKS[item.checks] : null;
  return (
    <>
      <div className="inspector-section work-card">
        <div className="inspector-row work-stage">
          <StatusPill status={stageStatus(item)} />
          {waiting && <span className="muted">{waiting}</span>}
        </div>
        {item.issue != null && (
          <Row here={from === "issues"} to={{ section: "issues", number: item.issue }} title="Show the issue">
            <IssueIcon size={13} />
            <span>
              Issue <span className="num">#{item.issue}</span>
            </span>
          </Row>
        )}
        {item.branch && (
          <Row here={false} to={{ section: "history", branch: item.branch }} title="Show it in History">
            <Branch size={13} />
            <span className="truncate">
              <code>{item.branch}</code>
              {item.base && <span className="muted"> → {item.base}</span>}
            </span>
          </Row>
        )}
        {item.worktree && (
          <Row
            here={false}
            onClick={item.task ? () => api.openFolder(item.task!).catch(shell.fail) : undefined}
            title={item.worktree}
          >
            <Folder size={13} />
            <span className="truncate">
              {item.task ? item.worktree.split("/").slice(-2).join("/") : item.worktree === shell.repo.root ? "Your checkout" : item.worktree}
            </span>
          </Row>
        )}
        {/* On Work's own page, the page's title is the task. */}
        {item.task && from !== "work" && (
          <Row here={false} to={{ section: "work", taskId: item.task }}>
            <Bot size={13} />
            <span className="truncate">
              Task: <b>{item.title}</b>
              {item.owner.kind === "agent" && <span className="muted"> · {item.owner.name}</span>}
            </span>
          </Row>
        )}
        {!item.task && item.owner.kind !== "you" && (
          <div className="inspector-row">
            {item.owner.kind === "agent" ? <Bot size={13} /> : <Users size={13} />}
            <span className="truncate">{item.owner.kind === "agent" ? item.owner.name : item.owner.login}</span>
          </div>
        )}
        {pr && (
          <Row here={from === "pulls"} to={{ section: "pulls", number: pr.number }} title={pr.title}>
            <PullRequest size={13} />
            <span>
              Pull request <span className="num">#{pr.number}</span>
            </span>
            <span className="flex-1" />
            <StatusPill status={pullPill(pr)} />
          </Row>
        )}
        {checks && item.branch && (
          <Row
            here={from === "actions"}
            to={{ section: "actions", branch: item.branch }}
            className={item.checks === "failed" ? "bad" : ""}
          >
            <CiIcon status={checks.ci} size={13} />
            <span>{checks.label}</span>
          </Row>
        )}
      </div>
      {step && (
        <div className="inspector-actions">
          <Button className="primary" onClick={() => shell.go(step.target, { browse: step.browse })}>
            {step.label}
          </Button>
        </div>
      )}
    </>
  );
}
