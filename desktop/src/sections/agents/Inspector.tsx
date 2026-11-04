import { useEffect, useState } from "react";
import type { SetupInfo, TaskDetail } from "../../types";
import { duration } from "./transcript";
import { AgentIcon, Alert, Branch, ChevronDown, Copy, Folder, Merge, Refresh, Terminal, Trash } from "../../shared/Icons";

interface Props {
  detail: TaskDetail;
  baseCheckedOut: boolean;
  onOpenFolder: () => void;
  onCopy: (text: string, what: string) => void;
  onShowChanges: () => void;
  onLand: (merge: boolean, commitUncommitted: boolean) => void;
  onDiscard: () => void;
  onAskRebase: () => void;
  onShowBranch: () => void;
  onRerunSetup: () => void;
}

/** The task's details and landing actions, in a small floating card. */
export function Inspector({ detail, ...p }: Props) {
  const t = detail.task;
  const s = t.stats;
  const running = t.run?.state === "running";
  const [includeUncommitted, setIncludeUncommitted] = useState(true);
  const [landMenu, setLandMenu] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  useEffect(() => {
    setConfirmDiscard(false);
    setLandMenu(false);
  }, [t.id]);

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

  if (t.landed) {
    return (
      <aside className="inspector">
        <div className="inspector-row">
          <Merge size={13} />
          <span>
            Landed on <b>{t.base}</b> as <code>{t.landed.slice(0, 7)}</code>
          </span>
        </div>
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
        {detail.setup && <SetupRow setup={detail.setup} running={running} onRerun={p.onRerunSetup} />}
      </div>
      <div className="inspector-section">
        <button className="inspector-row action" onClick={p.onShowBranch} title="Show it in History">
          <Branch size={13} />
          <span className="truncate">
            <code>{t.branch}</code> <span className="muted">from {t.base}</span>
          </span>
        </button>
        {s.base_moved && (
          <div className="inspector-row warn">
            <Alert size={13} />
            <span>{t.base} has moved on — landing rebases first</span>
          </div>
        )}
        {detail.conflicts.length > 0 && (
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
      <div className="inspector-actions">
        {s.uncommitted > 0 && !running && (
          <label className="check">
            <input
              type="checkbox"
              checked={includeUncommitted}
              onChange={(e) => setIncludeUncommitted(e.target.checked)}
            />
            Commit its uncommitted changes when landing
          </label>
        )}
        <div className="split">
          <button
            className="primary"
            disabled={landBlocked != null}
            title={landBlocked ?? "One commit with the task as its message (⌘Z undoes)"}
            onClick={() => p.onLand(false, includeUncommitted)}
          >
            Land on {t.base}
          </button>
          <button
            className="primary split-more"
            disabled={landBlocked != null}
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
                  p.onLand(true, includeUncommitted);
                }}
              >
                Land with a merge commit
              </button>
            </div>
          )}
        </div>
        <button
          className={confirmDiscard ? "danger" : "ghost"}
          disabled={running}
          title={running ? "Stop the agent first" : "Remove its folder and branch (⌘Z undoes)"}
          onClick={() => (confirmDiscard ? p.onDiscard() : setConfirmDiscard(true))}
          onBlur={() => setConfirmDiscard(false)}
        >
          <Trash size={12} /> {confirmDiscard ? "Discard?" : "Discard"}
        </button>
      </div>
      {landBlocked && hasWork && !running && <div className="inspector-note">{landBlocked}</div>}
    </aside>
  );
}

/** The folder's setup script (#23): which, how it went, and Re-run. */
function SetupRow({ setup, running, onRerun }: { setup: SetupInfo; running: boolean; onRerun: () => void }) {
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
