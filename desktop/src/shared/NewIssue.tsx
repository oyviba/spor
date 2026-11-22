// The New issue dialog (#148): title, a Markdown body, labels and
// assignees, through `gh issue create`. Opened from the Issues browser, ⌘K,
// or prefilled from what an agent's task turned up.

import { useState } from "react";
import { splitNames, type IssueDraft } from "./newIssue";

interface Props {
  draft: IssueDraft;
  /** Resolves to whether it was opened; the dialog stays up until then. */
  onCreate: (draft: IssueDraft) => Promise<boolean>;
  onClose: () => void;
}

export function NewIssueDialog({ draft, onCreate, onClose }: Props) {
  const [title, setTitle] = useState(draft.title);
  const [body, setBody] = useState(draft.body);
  const [labels, setLabels] = useState(draft.labels.join(", "));
  const [assignees, setAssignees] = useState(draft.assignees.join(", "));
  const [busy, setBusy] = useState(false);
  const ready = !!title.trim() && !busy;

  const submit = async () => {
    if (!ready) return;
    setBusy(true);
    const ok = await onCreate({ title: title.trim(), body, labels: splitNames(labels), assignees: splitNames(assignees) });
    if (ok) onClose();
    else setBusy(false);
  };

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <form
        className="dialog new-issue"
        role="dialog"
        aria-label="New issue"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape" && !busy) onClose();
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void submit();
          }
        }}
      >
        <h3>New issue</h3>
        <input autoFocus aria-label="Title" placeholder="Title" value={title} onChange={(e) => setTitle(e.target.value)} />
        <textarea
          aria-label="Description"
          rows={10}
          placeholder="What's wrong, or what's wanted (Markdown)"
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
        <label className="small-text muted">
          Labels
          <input placeholder="bug, ui" value={labels} onChange={(e) => setLabels(e.target.value)} />
        </label>
        <label className="small-text muted">
          Assignees
          <input placeholder="@me, a login" value={assignees} onChange={(e) => setAssignees(e.target.value)} />
        </label>
        <p className="small-text muted">
          Assigned to you (@me), it shows up in Work as Planned, ready to hand to an agent. Images: add them on GitHub
          afterwards; <code>gh</code> can't upload them.
        </p>
        <div className="dialog-actions">
          <button type="button" className="ghost" disabled={busy} onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={!ready}>
            {busy ? "Opening…" : "Open issue"}
          </button>
        </div>
      </form>
    </div>
  );
}
