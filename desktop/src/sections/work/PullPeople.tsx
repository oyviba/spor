// Who a pull request waits on and how it's filed (#149): the reviewers
// asked, its assignees and labels, each added from a list of the
// repository's collaborators, teams and labels (or typed), and taken off
// with ×. An open one can go back to draft from here; a draft is marked
// ready where it's landed.

import { useEffect, useRef, useState } from "react";
import { api } from "../../api";
import type { PullCandidates, PullDetail, PullEdit } from "../../types";
import { useShell } from "../../shared/app";
import { useChanged } from "../../shared/hooks";
import { Plus, Tag, Users, X } from "../../shared/Icons";

/** Each repository's candidates, asked for once per session (they rarely
 * change, and listing collaborators is a paginated call). */
const candidates = new Map<string, Promise<PullCandidates>>();

export function useCandidates(root: string, wanted: boolean): PullCandidates | null {
  const [found, setFound] = useState<PullCandidates | null>(null);
  useEffect(() => {
    if (!wanted) return;
    let live = true;
    let promise = candidates.get(root);
    if (!promise) {
      promise = api.pullCandidates();
      candidates.set(root, promise);
      // Don't keep a failure: try again next time.
      promise.catch(() => candidates.delete(root));
    }
    promise.then(
      (c) => live && setFound(c),
      () => live && setFound({ people: [], labels: [] }),
    );
    return () => {
      live = false;
    };
  }, [root, wanted]);
  return found;
}

type Kind = "reviewers" | "assignees" | "labels";

const ROWS: { kind: Kind; label: string; empty: string; add: string }[] = [
  { kind: "reviewers", label: "Reviewers", empty: "No review requested", add: "Request a review" },
  { kind: "assignees", label: "Assignees", empty: "Nobody assigned", add: "Assign someone" },
  { kind: "labels", label: "Labels", empty: "No labels", add: "Add a label" },
];

function editFor(kind: Kind, add: string[], remove: string[]): PullEdit {
  switch (kind) {
    case "reviewers":
      return { add_reviewers: add, remove_reviewers: remove };
    case "assignees":
      return { add_assignees: add, remove_assignees: remove };
    case "labels":
      return { add_labels: add, remove_labels: remove };
  }
}

export function PullPeople({ pr, onChanged }: { pr: PullDetail; onChanged: () => void }) {
  const shell = useShell();
  const p = pr.pull;
  const open = p.state === "open" || p.state === "draft";
  const [picking, setPicking] = useState<Kind | null>(null);
  const [busy, setBusy] = useState(false);
  useChanged(() => setPicking(null), [p.number]);
  const options = useCandidates(shell.repo.root, picking != null);
  const have: Record<Kind, string[]> = {
    reviewers: p.reviewers ?? [],
    assignees: pr.assignees ?? [],
    labels: pr.labels ?? [],
  };

  const change = (kind: Kind, add: string[], remove: string[]) => {
    if (busy) return;
    setBusy(true);
    setPicking(null);
    return shell
      .act(() => api.pullEdit(p.number, editFor(kind, add, remove)), { undo: false, pending: `Updating #${p.number}…` })
      .then((ok) => ok && onChanged())
      .finally(() => setBusy(false));
  };

  const toDraft = () =>
    shell
      .act(() => api.pullAction(p.number, "draft"), { undo: false, pending: "Converting it to a draft…" })
      .then((ok) => ok && onChanged());

  return (
    <div className="inspector-section pull-people">
      {ROWS.map((row) => (
        <div key={row.kind} className="people-row">
          <span className="people-label muted small-text">
            {row.kind === "labels" ? <Tag size={11} /> : <Users size={11} />} {row.label}
          </span>
          <div className="people-values">
            {have[row.kind].length === 0 && <span className="muted small-text">{row.empty}</span>}
            {have[row.kind].map((name) => (
              <span key={name} className="chip">
                <span className="chip-main">{name}</span>
                {open && (
                  <button
                    className="chip-x"
                    disabled={busy}
                    onClick={() => change(row.kind, [], [name])}
                    aria-label={`Take ${name} off`}
                    title={row.kind === "reviewers" ? `Stop asking ${name} to review` : `Take ${name} off`}
                  >
                    <X size={10} />
                  </button>
                )}
              </span>
            ))}
            {open && (
              <span className="people-add">
                <button
                  className="icon-button"
                  disabled={busy}
                  aria-label={row.add}
                  title={row.add}
                  onClick={() => setPicking((k) => (k === row.kind ? null : row.kind))}
                >
                  <Plus size={12} />
                </button>
                {picking === row.kind && (
                  <Picker
                    choices={row.kind === "labels" ? options?.labels : options?.people.filter((n) => n !== p.author)}
                    taken={have[row.kind]}
                    placeholder={row.kind === "labels" ? "Label…" : row.kind === "reviewers" ? "Login or org/team…" : "Login…"}
                    onPick={(name) => change(row.kind, [name], [])}
                    onClose={() => setPicking(null)}
                  />
                )}
              </span>
            )}
          </div>
        </div>
      ))}
      {p.state === "open" && (
        <div className="people-row">
          <span className="people-label" />
          <button className="link small-text" onClick={toDraft} title="Take it back to draft while it's not ready to review">
            Convert to draft
          </button>
        </div>
      )}
    </div>
  );
}

/** A filterable list to pick one from; Enter takes what's typed when
 * nothing in the list matches it. */
function Picker({
  choices,
  taken,
  placeholder,
  onPick,
  onClose,
}: {
  choices: string[] | undefined;
  taken: string[];
  placeholder: string;
  onPick: (name: string) => void;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const [index, setIndex] = useState(0);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const away = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [onClose]);
  const q = text.trim().toLowerCase();
  const shown = (choices ?? []).filter((c) => !taken.includes(c) && c.toLowerCase().includes(q)).slice(0, 50);
  const pick = () => {
    const chosen = shown[index] ?? text.trim();
    if (chosen) onPick(chosen);
  };
  return (
    <div className="menu people-picker" ref={ref}>
      <input
        autoFocus
        value={text}
        placeholder={placeholder}
        onChange={(e) => {
          setText(e.target.value);
          setIndex(0);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
          else if (e.key === "Enter") {
            e.preventDefault();
            pick();
          } else if (e.key === "ArrowDown") {
            e.preventDefault();
            setIndex((i) => Math.min(i + 1, shown.length - 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setIndex((i) => Math.max(i - 1, 0));
          }
        }}
      />
      <div className="people-choices">
        {choices == null && <div className="muted small-text people-note">Loading…</div>}
        {shown.map((c, i) => (
          <button key={c} className={i === index ? "active" : undefined} onMouseEnter={() => setIndex(i)} onClick={() => onPick(c)}>
            {c}
          </button>
        ))}
        {choices != null && shown.length === 0 && (
          <div className="muted small-text people-note">{q ? `Enter adds “${text.trim()}”` : "Type a name"}</div>
        )}
      </div>
    </div>
  );
}
