// Resolve a conflicted file one region at a time (#61): each conflict
// shows the current and incoming sides; pick one, both, or both the other
// way round, then Save and mark resolved.

import { useState } from "react";
import { useChanged } from "../../shared/hooks";
import { api, errorText } from "../../api";
import type { ConflictPart } from "../../types";
import { Check } from "../../shared/Icons";
import { Button } from "../../shared/Button";

type Choice = "ours" | "theirs" | "both" | "both_theirs_first";

const CONTEXT = 3;

export function ConflictRegions({
  path,
  operation,
  onSave,
}: {
  path: string;
  operation: string | null;
  onSave: (choices: Choice[]) => unknown;
}) {
  const [parts, setParts] = useState<ConflictPart[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [choices, setChoices] = useState<(Choice | null)[]>([]);

  useChanged(() => {
    setParts(null);
    setError(null);
    api.conflictRegions(path).then(
      (p) => {
        setParts(p);
        setChoices(p.filter((x) => x.kind === "conflict").map(() => null));
      },
      (e) => setError(errorText(e)),
    );
  }, [path]);

  if (error) return <p className="hint">{error}</p>;
  if (!parts) return <p className="hint">Reading the conflicts…</p>;
  const n = choices.length;
  if (n === 0)
    return (
      <p className="hint">
        No conflict markers are left in this file. If it looks right, mark it resolved; or take one side for the whole file.
      </p>
    );

  // During a rebase "ours" is the branch being rebased onto, and "theirs" the
  // commit being replayed.
  const sides =
    operation === "rebase"
      ? { ours: "Upstream", theirs: "Your commit" }
      : { ours: "Current", theirs: "Incoming" };
  const picked = choices.filter(Boolean).length;
  let k = -1;

  return (
    <div className="conflicts">
      <div className="conflicts-head">
        <span>
          {n} {n === 1 ? "conflict" : "conflicts"} in this file · {picked} of {n} picked
        </span>
        <span className="flex-1" />
        <button className="ghost small" onClick={() => setChoices(choices.map(() => "ours"))}>
          All {sides.ours.toLowerCase()}
        </button>
        <button className="ghost small" onClick={() => setChoices(choices.map(() => "theirs"))}>
          All {sides.theirs.toLowerCase()}
        </button>
        <Button className="primary small" disabled={picked < n} onClick={() => onSave(choices as Choice[])}>
          <Check size={12} /> Save and mark resolved
        </Button>
      </div>
      {parts.map((part, i) => {
        if (part.kind === "text") return <Context key={i} text={part.text} first={i === 0} last={i === parts.length - 1} />;
        const index = ++k;
        const choice = choices[index];
        const pick = (c: Choice) => setChoices(choices.map((x, j) => (j === index ? (x === c ? null : c) : x)));
        const kept = (side: "ours" | "theirs") =>
          choice === side || choice === "both" || choice === "both_theirs_first" ? " kept" : choice ? " dropped" : "";
        return (
          <div key={i} className={`conflict-region${choice ? " decided" : ""}`}>
            <div className="conflict-sides">
              <div className={`conflict-side ours${kept("ours")}`}>
                <div className="conflict-label">
                  <span>{sides.ours}</span>
                  <span className="mono muted truncate">{part.ours_label}</span>
                </div>
                <pre>{part.ours || <span className="muted">(nothing)</span>}</pre>
              </div>
              <div className={`conflict-side theirs${kept("theirs")}`}>
                <div className="conflict-label">
                  <span>{sides.theirs}</span>
                  <span className="mono muted truncate">{part.theirs_label}</span>
                </div>
                <pre>{part.theirs || <span className="muted">(nothing)</span>}</pre>
              </div>
            </div>
            {part.base != null && (
              <details className="conflict-base">
                <summary>What it was before both changes</summary>
                <pre>{part.base || "(nothing)"}</pre>
              </details>
            )}
            <div className="conflict-picks" role="radiogroup" aria-label={`Conflict ${index + 1}`}>
              {(
                [
                  ["ours", sides.ours],
                  ["theirs", sides.theirs],
                  ["both", "Both"],
                  ["both_theirs_first", `Both, ${sides.theirs.toLowerCase()} first`],
                ] as [Choice, string][]
              ).map(([c, label]) => (
                <button key={c} role="radio" aria-checked={choice === c} className={`chip-toggle${choice === c ? " on" : ""}`} onClick={() => pick(c)}>
                  {label}
                </button>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** Text between conflicts: a few lines around each, the rest folded. */
function Context({ text, first, last }: { text: string; first: boolean; last: boolean }) {
  const [open, setOpen] = useState(false);
  const lines = text.replace(/\n$/, "").split("\n");
  const head = first ? 0 : CONTEXT;
  const tail = last ? 0 : CONTEXT;
  if (open || lines.length <= head + tail + 1) return <pre className="conflict-context">{lines.join("\n")}</pre>;
  return (
    <div className="conflict-context">
      {head > 0 && <pre>{lines.slice(0, head).join("\n")}</pre>}
      <button className="link small-text conflict-fold" onClick={() => setOpen(true)}>
        {lines.length - head - tail} unchanged lines
      </button>
      {tail > 0 && <pre>{lines.slice(-tail).join("\n")}</pre>}
    </div>
  );
}
