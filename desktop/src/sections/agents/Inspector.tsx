import { useState } from "react";
import { useChanged } from "../../shared/hooks";
import type { SetupInfo, TaskDetail, TaskPull } from "../../types";
import { duration } from "./transcript";
import { useShell } from "../../shared/app";
import { Button } from "../../shared/Button";
import { WorkCard } from "../../shared/WorkCard";
import { findWork, itemFromTask } from "../../shared/work";
import {
  AgentIcon,
  Alert,
  ArrowUp,
  ChevronDown,
  Copy,
  Merge,
  MessageSquare,
  PullRequest,
  Refresh,
  Terminal,
  Trash,
} from "../../shared/Icons";

/** What the card does with the task's pull request. */
export interface GithubActions {
  /** Its pull request, once it has one (#24, #47). */
  pull?: TaskPull;
  onPublish: (draft: boolean, commitUncommitted: boolean) => unknown;
  /** Push to its PR; with `commitUncommitted`, uncommitted work goes too. */
  onPush: (commitUncommitted: boolean) => unknown;
  onSendReview: () => unknown;
  /** Send its PR's failed Actions jobs to the agent. */
  onSendFailure: () => unknown;
  onSettle: (commit: string) => unknown;
  /** Land its PR (squash and merge on GitHub), then clean up as `onSettle` does. */
  onMerge: () => unknown;
  /** Mark its draft PR ready for review. */
  onReady: () => unknown;
  /** Its PR was closed unmerged: close it and remove its folder and branch. */
  onClose: () => unknown;
}

interface Props extends GithubActions {
  detail: TaskDetail;
  baseCheckedOut: boolean;
  onCopy: (text: string, what: string) => void;
  onShowChanges: () => void;
  onLand: (merge: boolean, commitUncommitted: boolean) => unknown;
  onDiscard: () => unknown;
  onAskRebase: () => void;
  /** Ask the agent to put back what it would undo on the base. */
  onAskRestore: () => void;
  onRerunSetup: () => unknown;
}

/** The task's details and landing actions, in a small floating card, under
 * the work card that links its issue, branch, folder, PR and checks. */
export function Inspector({ detail, ...p }: Props) {
  const shell = useShell();
  const t = detail.task;
  const item = findWork(shell.work, { task: t.id }) ?? itemFromTask(t, p.pull?.pull);
  const s = t.stats;
  const running = t.run?.state === "running";
  const [includeUncommitted, setIncludeUncommitted] = useState(true);
  const [landMenu, setLandMenu] = useState(false);
  const [prMenu, setPrMenu] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  // What it's doing (publishing, landing…): that button spins, the others
  // that would change the task wait.
  const [working, setWorking] = useState<"publish" | "land" | "discard" | "merge" | null>(null);
  const [confirmMerge, setConfirmMerge] = useState(false);
  const work = (what: NonNullable<typeof working>, run: () => unknown) => {
    if (working) return;
    setWorking(what);
    return Promise.resolve(run()).finally(() => setWorking(null));
  };
  useChanged(() => {
    setConfirmDiscard(false);
    setConfirmMerge(false);
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
  const undoneLines = detail.undoes.reduce((n, u) => n + u.lines, 0);
  const lastRun = detail.runs.at(-1);
  const spent = detail.runs.reduce((n, r) => n + (r.cost_usd ?? 0), 0);

  // What merging its PR from here waits for. GitHub has the final say
  // (required reviews and checks); this is what would lose work.
  const mergeBlocked = !prOpen
    ? null
    : running
      ? "The agent is still working"
      : pr.state === "draft"
        ? "It's a draft — mark it ready for review first"
        : s.unpushed > 0 || s.uncommitted > 0
          ? `Push what isn't on #${pr.number} yet first`
          : detail.conflicts.length > 0
              ? `Resolve the conflicts with ${t.base} first`
              : null;

  const publishBlocked = running
    ? "The agent is still working"
    : !hasWork
      ? "Nothing to publish yet"
      : s.uncommitted > 0 && !includeUncommitted
        ? "It has uncommitted changes"
        : null;

  // Work the agent did that its pull request doesn't have yet.
  const unpushed = prOpen && (s.unpushed > 0 || s.uncommitted > 0) && (
    <div className="inspector-section">
      <div className="inspector-row warn">
        <ArrowUp size={13} />
        <span>
          {[
            s.unpushed > 0 && `${s.unpushed} ${s.unpushed === 1 ? "commit" : "commits"}`,
            s.uncommitted > 0 && `${s.uncommitted} uncommitted ${s.uncommitted === 1 ? "file" : "files"}`,
          ]
            .filter(Boolean)
            .join(" and ")}{" "}
          not on #{pr.number} yet
          <Button
            className="link"
            onClick={() => p.onPush(s.uncommitted > 0)}
            disabled={running}
            title={s.uncommitted > 0 ? "Commit what it left uncommitted, and push" : undefined}
          >
            {s.uncommitted > 0 ? "Commit and push" : "Push"}
          </Button>
        </span>
      </div>
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
            <span>Closed — its pull request didn't land</span>
          )}
        </div>
        <WorkCard item={item} from="work" actions={false} />
      </aside>
    );
  }

  return (
    <aside className="inspector">
      <WorkCard item={item} from="work" actions={false} />
      <div className="inspector-section">
        <button className="inspector-row action" onClick={() => p.onCopy(t.command, "Agent command")}>
          <Copy size={13} />
          <span>Copy agent command</span>
        </button>
        {detail.setup && <SetupRow setup={detail.setup} running={running} onRerun={p.onRerunSetup} />}
      </div>
      <div className="inspector-section">
        {t.from && (
          <div className="inspector-row muted">
            <span className="truncate">Started from {t.from}</span>
          </div>
        )}
        {s.base_moved && !done && (
          <div className="inspector-row warn">
            <Alert size={13} />
            <span>{t.from ?? t.base} has moved on — landing rebases first</span>
          </div>
        )}
        {detail.conflicts.length > 0 && !done && (
          <div className="inspector-row bad">
            <Alert size={13} />
            <span>
              Conflicts with {t.base} in {detail.conflicts.join(", ")}
              <button className="link" onClick={p.onAskRebase} disabled={running}>
                {t.pull != null ? `Ask agent to merge ${t.base} in` : "Ask agent to rebase"}
              </button>
            </span>
          </div>
        )}
        {detail.undoes.length > 0 && !done && (
          <div className="inspector-row bad" title={detail.undoes.map((u) => `${u.path}: ${u.lines}`).join("\n")}>
            <Alert size={13} />
            <span>
              Would undo {undoneLines} {undoneLines === 1 ? "line" : "lines"} that reached {t.base} after it started, in{" "}
              {detail.undoes
                .slice(0, 2)
                .map((u) => u.path.split("/").pop())
                .join(", ")}
              {detail.undoes.length > 2 && ` and ${detail.undoes.length - 2} more`}
              <button className="link" onClick={p.onAskRestore} disabled={running}>
                Ask agent to put them back
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
      {unpushed}
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
          <div className="inspector-note">Its pull request landed. Its folder and branch can go.</div>
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
            <Trash size={12} /> Clean up
          </Button>
        </div>
      )}
      {prOpen && (
        <div className="inspector-actions">
          {pr.state === "draft" && (
            <Button className="ghost" disabled={running} onClick={p.onReady}>
              Ready for review
            </Button>
          )}
          <Button
            className={confirmMerge ? "danger" : "primary"}
            disabled={mergeBlocked != null || (working != null && working !== "merge")}
            busy={working === "merge"}
            title={
              mergeBlocked ??
              (pr.checks === "failing"
                ? "Its checks are failing — land it anyway: squash and merge on GitHub, then clean up"
                : pr.checks === "pending"
                  ? "Its checks are still running — land it anyway: squash and merge on GitHub, then clean up"
                  : "Squash and merge on GitHub, then remove its folder and branch (⌘Z brings them back)")
            }
            onClick={() => (confirmMerge ? work("merge", p.onMerge) : setConfirmMerge(true))}
            onBlur={() => setConfirmMerge(false)}
          >
            <Merge size={12} /> {confirmMerge ? `Land #${pr.number} on ${t.base}?` : `Land #${pr.number}`}
          </Button>
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
        {s.uncommitted > 0 && !running && !prOpen && (
          <label className="check">
            <input
              type="checkbox"
              checked={includeUncommitted}
              onChange={(e) => setIncludeUncommitted(e.target.checked)}
            />
            Commit its uncommitted changes when landing or publishing
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
              <PullRequest size={12} /> Publish
            </Button>
            <button
              className="ghost split-more"
              disabled={publishBlocked != null || working != null}
              aria-label="More ways to publish"
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
                  Publish as a draft
                </button>
              </div>
            )}
          </div>
        )}
        {!prOpen && (
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
        )}
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
      {!done && !prOpen && landBlocked && hasWork && !running && <div className="inspector-note">{landBlocked}</div>}
      {mergeBlocked && !running && <div className="inspector-note">{mergeBlocked}</div>}
    </aside>
  );
}

/** The folder's setup script (#23): which, how it went, and Re-run. */
function SetupRow({ setup, running, onRerun }: { setup: SetupInfo; running: boolean; onRerun: () => unknown }) {
  const [label, tone] = ((): [string, string] => {
    switch (setup.state) {
      case "running":
        return ["running", ""];
      case "worked":
        return ["worked", "ok"];
      case "failed":
        return [setup.code != null ? `failed (exit ${setup.code})` : "failed", "bad"];
      case "skipped":
        return ["skipped", "muted"];
      case "due":
        return ["runs before the next agent", "muted"];
      default:
        return ["runs before the first agent", "muted"];
    }
  })();
  const canRerun = setup.state === "worked" || setup.state === "skipped";
  return (
    <div className={`inspector-row setup-row ${tone}`}>
      <Terminal size={13} />
      <span className="truncate" title={setup.script}>
        <code>{setup.script}</code> <span className="setup-state">{label}</span>
      </span>
      {canRerun && (
        <button
          className="icon-button tiny"
          onClick={onRerun}
          disabled={running}
          title="Run it again before the next agent"
          aria-label="Re-run setup"
        >
          <Refresh size={11} />
        </button>
      )}
    </div>
  );
}
