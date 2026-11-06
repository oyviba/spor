// "Rewrite after here": an interactive rebase planned on screen. The
// commits after the chosen one, oldest first; reorder them, reword, squash,
// fix up or drop, then Rewrite history (⌘Z puts the branch back).

import { useState } from "react";
import { useChanged } from "../../shared/hooks";
import { api } from "../../api";
import type { PlanAction, PlanCommit } from "../../types";
import { useShell } from "../../shared/app";
import { Button } from "../../shared/Button";
import { ArrowDown, ArrowUp } from "../../shared/Icons";

interface Step {
  commit: PlanCommit;
  action: PlanAction;
  message: string;
  /** Its place in git's order, to tell whether anything changed. */
  original: number;
}

const ACTIONS: { id: PlanAction; label: string; title: string }[] = [
  { id: "pick", label: "Pick", title: "Keep it as it is" },
  { id: "reword", label: "Reword", title: "Keep it, with a new message" },
  { id: "squash", label: "Squash", title: "Fold into the commit above, keeping both messages" },
  { id: "fixup", label: "Fix up", title: "Fold into the commit above, keeping only its message" },
  { id: "drop", label: "Drop", title: "Leave it out" },
];

export function Rewrite({ base, onDone, onCancel }: { base: string; onDone: () => void; onCancel: () => void }) {
  const shell = useShell();
  const [steps, setSteps] = useState<Step[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useChanged(() => {
    setSteps(null);
    setError(null);
    api.rewritePlan(base).then(
      (commits) => setSteps(commits.map((commit, original) => ({ commit, action: "pick", message: commit.message, original }))),
      (e) => setError(typeof e === "string" ? e : String(e)),
    );
  }, [base]);

  if (error)
    return (
      <div className="rewrite">
        <p className="hint">{error}</p>
        <button className="ghost" onClick={onCancel}>
          Back to the graph
        </button>
      </div>
    );
  if (!steps) return <div className="boot" />;
  if (steps.length === 0)
    return (
      <div className="rewrite">
        <p className="hint">There are no commits after {base.slice(0, 7)} on this branch.</p>
        <button className="ghost" onClick={onCancel}>
          Back to the graph
        </button>
      </div>
    );

  const set = (i: number, patch: Partial<Step>) => setSteps(steps.map((s, k) => (k === i ? { ...s, ...patch } : s)));
  const move = (i: number, by: number) => {
    const j = i + by;
    if (j < 0 || j >= steps.length) return;
    const next = [...steps];
    [next[i], next[j]] = [next[j], next[i]];
    setSteps(next);
  };
  const firstKept = steps.find((s) => s.action !== "drop");
  const problem =
    firstKept && (firstKept.action === "squash" || firstKept.action === "fixup")
      ? "The first commit can't be folded — there's nothing above it on this list."
      : steps.some((s) => s.action === "reword" && !s.message.trim())
        ? "A reworded commit needs a message."
        : null;
  const unchanged = steps.every((s, i) => s.action === "pick" && i === s.original);

  const run = async () => {
    setBusy(true);
    const ok = await shell.act(
      () =>
        api.rewrite(
          base,
          steps.map((s) => ({ hash: s.commit.hash, action: s.action, message: s.action === "reword" ? s.message : null })),
        ),
      { pending: "Rewriting history…" },
    );
    setBusy(false);
    if (ok) onDone();
  };

  return (
    <div className="rewrite">
      <p className="lede">
        Rewriting the {steps.length} {steps.length === 1 ? "commit" : "commits"} after{" "}
        <span className="mono">{base.slice(0, 7)}</span> on <b>{shell.state?.branch}</b>, oldest first. Nothing changes until
        you press Rewrite history, and ⌘Z puts the branch back.
      </p>
      <ol className="rewrite-list">
        {steps.map((s, i) => (
          <li key={s.commit.hash} className={`rewrite-step a-${s.action}`}>
            <div className="rewrite-row">
              <span className="rewrite-move">
                <button className="icon-button tiny" disabled={i === 0} onClick={() => move(i, -1)} aria-label="Move up">
                  <ArrowUp size={11} />
                </button>
                <button
                  className="icon-button tiny"
                  disabled={i === steps.length - 1}
                  onClick={() => move(i, 1)}
                  aria-label="Move down"
                >
                  <ArrowDown size={11} />
                </button>
              </span>
              <select value={s.action} onChange={(e) => set(i, { action: e.target.value as PlanAction })} title={ACTIONS.find((a) => a.id === s.action)?.title}>
                {ACTIONS.map((a) => (
                  <option key={a.id} value={a.id} title={a.title}>
                    {a.label}
                  </option>
                ))}
              </select>
              <span className="mono muted">{s.commit.short}</span>
              <span className="truncate rewrite-subject">{s.commit.subject}</span>
              <span className="muted small-text">{s.commit.author}</span>
            </div>
            {s.action === "reword" && (
              <textarea
                className="rewrite-message"
                rows={Math.min(8, Math.max(2, s.message.split("\n").length))}
                value={s.message}
                onChange={(e) => set(i, { message: e.target.value })}
              />
            )}
          </li>
        ))}
      </ol>
      <div className="rewrite-actions">
        {problem && <span className="bad-text small-text">{problem}</span>}
        <span className="flex-1" />
        <button className="ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        <Button className="primary" disabled={!!problem || unchanged} busy={busy} onClick={run}>
          Rewrite history
        </Button>
      </div>
    </div>
  );
}
