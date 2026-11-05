import { useEffect, useState } from "react";
import type { TaskDetail, TaskPull } from "../../types";
import { duration } from "./transcript";
import { pullStatus } from "../pulls/PullsSection";
import { StatusPill } from "../../shared/StatusPill";
import { Button } from "../../shared/Button";
import {
  AgentIcon,
  Alert,
  ArrowUp,
  Branch,
  ChevronDown,
  Copy,
  Folder,
  IssueIcon,
  Merge,
  MessageSquare,
  PullRequest,
  Trash,
} from "../../shared/Icons";

/** What the card does with the task's issue and pull request. */
export interface GithubActions {
  /** Its pull request, once it has one (#24, #47). */
  pull?: TaskPull;
  onShowIssue: (n: number) => void;
  onShowPull: (n: number) => void;
  onPublish: (draft: boolean, commitUncommitted: boolean) => unknown;
  onPush: () => unknown;
  onSendReview: () => unknown;
  /** Send its PR's failed Actions jobs to the agent. */
  onSendFailure: () => unknown;
  onSettle: (commit: string) => unknown;
  /** Its PR was closed unmerged: close it and remove its folder and branch. */
  onClose: () => unknown;
}

interface Props extends GithubActions {
  detail: TaskDetail;
  baseCheckedOut: boolean;
  onOpenFolder: () => unknown;
  onCopy: (text: string, what: string) => void;
  onShowChanges: () => void;
  onLand: (merge: boolean, commitUncommitted: boolean) => unknown;
  onDiscard: () => unknown;
  onAskRebase: () => void;
  onShowBranch: () => void;
}

/** The task's details and landing actions, in a small floating card. */
export function Inspector({ detail, ...p }: Props) {
  const t = detail.task;
  const s = t.stats;
  const running = t.run?.state === "running";
  const [includeUncommitted, setIncludeUncommitted] = useState(true);
  const [landMenu, setLandMenu] = useState(false);
  const [prMenu, setPrMenu] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  // What it's doing (publishing, landing…): that button spins, the others
  // that would change the task wait.
  const [working, setWorking] = useState<"publish" | "land" | "discard" | null>(null);
  const work = (what: NonNullable<typeof working>, run: () => unknown) => {
    if (working) return;
    setWorking(what);
    return Promise.resolve(run()).finally(() => setWorking(null));
  };
  useEffect(() => {
    setConfirmDiscard(false);
    setLandMenu(false);
    setPrMenu(false);
  }, [t.id]);
  const pr = p.pull?.pull;
  const prOpen = pr && (pr.state === "open" || pr.state === "draft");
  const merged = pr?.state === "merged";
  const closed = pr?.state === "closed";
  // Merged or closed on GitHub: what's left is to clean up.
  const done = merged || closed;

  const hasWork = s.commits > 0 || s.uncommitted > 0;
  const landBlocked = running
    ? "The agent is still working"
    : !p.baseCheckedOut
      ? `Check out ${t.base} in the main folder to land on it`
      : !hasWork
        ? "Nothing to land yet"
        : s.uncommitted > 0 && !includeUncommitted
          ? "It has uncommitted changes"
          : null;
  const lastRun = detail.runs.at(-1);
  const spent = detail.runs.reduce((n, r) => n + (r.cost_usd ?? 0), 0);

  const publishBlocked = running
    ? "The agent is still working"
    : !hasWork
      ? "Nothing to publish yet"
      : s.uncommitted > 0 && !includeUncommitted
        ? "It has uncommitted changes"
        : null;

  // Where it came from and where it went, on GitHub.
  const github = (t.issue != null || t.pull != null || pr) && (
    <div className="inspector-section">
      {t.issue != null && (
        <button className="inspector-row action" onClick={() => p.onShowIssue(t.issue!)} title="Show the issue">
          <IssueIcon size={13} />
          <span>
            Issue <span className="num">#{t.issue}</span>
          </span>
        </button>
      )}
      {!pr && t.pull != null && (
        <button className="inspector-row action" onClick={() => p.onShowPull(t.pull!)}>
          <PullRequest size={13} />
          <span>
            Pull request <span className="num">#{t.pull}</span>
          </span>
        </button>
      )}
      {pr && (
        <button className="inspector-row action" onClick={() => p.onShowPull(pr.number)} title={pr.title}>
          <PullRequest size={13} />
          <span className="num">#{pr.number}</span>
          <span className="flex-1" />
          <StatusPill status={pullStatus(pr)} />
        </button>
      )}
      {prOpen && s.unpushed > 0 && (
        <div className="inspector-row warn">
          <ArrowUp size={13} />
          <span>
            {s.unpushed} {s.unpushed === 1 ? "commit" : "commits"} not on #{pr.number} yet
            <Button className="link" onClick={p.onPush} disabled={running}>
              Push
            </Button>
          </span>
        </div>
      )}
    </div>
  );

  if (t.landed || t.closed) {
    return (
      <aside className="inspector">
        <div className="inspector-row">
          <Merge size={13} />
          {t.landed ? (
            <span>
              Landed on <b>{t.base}</b> as <code>{t.landed.slice(0, 7)}</code>
            </span>
          ) : (
            <span>Closed — its pull request wasn't merged</span>
          )}
        </div>
        {github}
      </aside>
    );
  }

  return (
    <aside className="inspector">
      <div className="inspector-section">
        <button className="inspector-row action" onClick={p.onOpenFolder} title={t.worktree}>
          <Folder size={13} />
          <span className="truncate">{t.worktree.split("/").slice(-2).join("/")}</span>
        </button>
        <button className="inspector-row action" onClick={() => p.onCopy(t.command, "Agent command")}>
          <Copy size={13} />
          <span>Copy agent command</span>
        </button>
      </div>
      <div className="inspector-section">
        <button className="inspector-row action" onClick={p.onShowBranch} title="Show it in History">
          <Branch size={13} />
          <span className="truncate">
            <code>{t.branch}</code> <span className="muted">from {t.base}</span>
          </span>
        </button>
        {s.base_moved && !done && (
          <div className="inspector-row warn">
            <Alert size={13} />
            <span>{t.base} has moved on — landing rebases first</span>
          </div>
        )}
        {detail.conflicts.length > 0 && !done && (
          <div className="inspector-row bad">
            <Alert size={13} />
            <span>
              Conflicts with {t.base} in {detail.conflicts.join(", ")}
              <button className="link" onClick={p.onAskRebase} disabled={running}>
                Ask agent to rebase
              </button>
            </span>
          </div>
        )}
        <button className="inspector-row action" onClick={p.onShowChanges}>
          <span className="diffstat num">
            <span className="add">+{s.additions}</span> <span className="del">−{s.deletions}</span>
          </span>
          <span>
            {s.files} {s.files === 1 ? "file" : "files"}
            <span className="muted">
              {" "}
              · {s.commits} {s.commits === 1 ? "commit" : "commits"}
              {s.uncommitted > 0 && ` · ${s.uncommitted} uncommitted`}
            </span>
          </span>
        </button>
      </div>
      {github}
      {lastRun && (
        <div className="inspector-section">
          <div className="inspector-row">
            <AgentIcon agent={lastRun.agent} />
            <span className="num">
              {lastRun.label} · {detail.runs.length} {detail.runs.length === 1 ? "run" : "runs"}
              {lastRun.ended && ` · last ${duration(lastRun.ended - lastRun.started)}`}
              {spent > 0 && ` · $${spent.toFixed(2)}`}
            </span>
          </div>
        </div>
      )}
      {merged && (
        <div className="inspector-actions">
          <div className="inspector-note">Its pull request was merged. Its folder and branch can go.</div>
          <Button
            className="primary"
            disabled={running}
            title="Mark it landed and remove its folder and branch (⌘Z undoes)"
            onClick={() => p.onSettle(p.pull!.merge_commit ?? `#${pr!.number}`)}
          >
            <Merge size={12} /> Clean up
          </Button>
        </div>
      )}
      {closed && (
        <div className="inspector-actions">
          <div className="inspector-note">Its pull request was closed without merging.</div>
          <Button
            className="ghost"
            disabled={running}
            title="Mark it closed and remove its folder and branch (⌘Z undoes)"
            onClick={p.onClose}
          >
            <Trash size={12} /> Close task
          </Button>
        </div>
      )}
      {prOpen && (
        <div className="inspector-actions">
          {pr?.checks === "failing" && (
            <Button
              className="ghost"
              onClick={p.onSendFailure}
              title="Send the failed jobs' failing steps and the end of their logs"
            >
              <Alert size={12} /> Send failing checks to agent
            </Button>
          )}
          <Button
            className="ghost"
            onClick={p.onSendReview}
            title="Send reviews and line comments since the agent last ran"
          >
            <MessageSquare size={12} /> Send review to agent
          </Button>
        </div>
      )}
      {!done && (
      <div className="inspector-actions">
        {s.uncommitted > 0 && !running && (
          <label className="check">
            <input
              type="checkbox"
              checked={includeUncommitted}
              onChange={(e) => setIncludeUncommitted(e.target.checked)}
            />
            Commit its uncommitted changes when landing{pr ? "" : " or publishing"}
          </label>
        )}
        {!pr && (
          <div className="split">
            <Button
              className="ghost"
              disabled={publishBlocked != null || (working != null && working !== "publish")}
              busy={working === "publish"}
              title={publishBlocked ?? `Push ${t.branch} and open a pull request into ${t.base}`}
              onClick={() => work("publish", () => p.onPublish(false, includeUncommitted))}
            >
              <PullRequest size={12} /> Open pull request
            </Button>
            <button
              className="ghost split-more"
              disabled={publishBlocked != null || working != null}
              aria-label="More ways to open a pull request"
              onClick={() => setPrMenu((v) => !v)}
            >
              <ChevronDown size={12} />
            </button>
            {prMenu && (
              <div className="menu" onMouseLeave={() => setPrMenu(false)}>
                <button
                  onClick={() => {
                    setPrMenu(false);
                    work("publish", () => p.onPublish(true, includeUncommitted));
                  }}
                >
                  Open as a draft
                </button>
              </div>
            )}
          </div>
        )}
        <div className="split">
          <Button
            className="primary"
            disabled={landBlocked != null || (working != null && working !== "land")}
            busy={working === "land"}
            title={landBlocked ?? "One commit with the task as its message (⌘Z undoes)"}
            onClick={() => work("land", () => p.onLand(false, includeUncommitted))}
          >
            Land on {t.base}
          </Button>
          <button
            className="primary split-more"
            disabled={landBlocked != null || working != null}
            aria-label="More ways to land"
            onClick={() => setLandMenu((v) => !v)}
          >
            <ChevronDown size={12} />
          </button>
          {landMenu && (
            <div className="menu" onMouseLeave={() => setLandMenu(false)}>
              <button
                onClick={() => {
                  setLandMenu(false);
                  work("land", () => p.onLand(true, includeUncommitted));
                }}
              >
                Land with a merge commit
              </button>
            </div>
          )}
        </div>
        <Button
          className={confirmDiscard ? "danger" : "ghost"}
          disabled={running || (working != null && working !== "discard")}
          busy={working === "discard"}
          title={running ? "Stop the agent first" : "Remove its folder and branch (⌘Z undoes)"}
          onClick={() => (confirmDiscard ? work("discard", p.onDiscard) : setConfirmDiscard(true))}
          onBlur={() => setConfirmDiscard(false)}
        >
          <Trash size={12} /> {confirmDiscard ? "Discard?" : "Discard"}
        </Button>
      </div>
      )}
      {!done && landBlocked && hasWork && !running && <div className="inspector-note">{landBlocked}</div>}
    </aside>
  );
}
