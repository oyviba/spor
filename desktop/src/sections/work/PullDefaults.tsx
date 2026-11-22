// What a pull request opened from spor starts with in this repository
// (#149): a draft or not, the reviewers it asks, its assignees and labels.
// Kept in the repository's git config (spor.pullDraft, spor.pullReviewers,
// spor.pullAssignees, spor.pullLabels), so it's per repository.

import { useEffect, useState } from "react";
import { api } from "../../api";
import type { PullPreset } from "../../types";
import { useShell } from "../../shared/app";

const split = (text: string) =>
  text
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

/** The preset's one-line summary, for the Publish button's menu. */
export function presetSummary(p: PullPreset): string | null {
  const parts = [
    p.reviewers.length ? `asks ${p.reviewers.join(", ")} to review` : null,
    p.assignees.length ? `assigns ${p.assignees.join(", ")}` : null,
    p.labels.length ? `labels ${p.labels.join(", ")}` : null,
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : null;
}

export function PullDefaults() {
  const shell = useShell();
  const [saved, setSaved] = useState<PullPreset | null>(null);
  const [text, setText] = useState({ reviewers: "", assignees: "", labels: "" });

  useEffect(() => {
    api.pullPreset().then((p) => {
      setSaved(p);
      setText({ reviewers: p.reviewers.join(", "), assignees: p.assignees.join(", "), labels: p.labels.join(", ") });
    }, shell.fail);
  }, [shell.repo.root]); // eslint-disable-line react-hooks/exhaustive-deps

  const save = (next: PullPreset) => {
    if (!saved || JSON.stringify(next) === JSON.stringify(saved)) return;
    api.setPullPreset(next).then(() => {
      setSaved(next);
      shell.toast("Saved the pull request defaults");
    }, shell.fail);
  };
  const fromText = (): PullPreset => ({
    draft: saved?.draft ?? false,
    reviewers: split(text.reviewers),
    assignees: split(text.assignees),
    labels: split(text.labels),
  });

  const field = (key: keyof typeof text, label: string, placeholder: string) => (
    <label className="preset-field">
      <span className="small-text">{label}</span>
      <input
        value={text[key]}
        placeholder={placeholder}
        disabled={!saved}
        onChange={(e) => setText((t) => ({ ...t, [key]: e.target.value }))}
        onBlur={() => save(fromText())}
        onKeyDown={(e) => e.key === "Enter" && save(fromText())}
      />
    </label>
  );

  return (
    <div className="settings-page">
      <h3>Pull requests</h3>
      <p className="muted small-text">
        What a pull request starts with when you publish a task from <b>{shell.repo.name}</b>. Names are separated by
        commas; a team is <code>org/team</code>. Kept in this repository&apos;s git config.
      </p>
      <label className="check">
        <input
          type="checkbox"
          checked={saved?.draft ?? false}
          disabled={!saved}
          onChange={(e) => save({ ...fromText(), draft: e.target.checked })}
        />
        Open as a draft (mark it ready when it&apos;s done)
      </label>
      {field("reviewers", "Ask to review", "ada, acme/web")}
      {field("assignees", "Assign", "ada")}
      {field("labels", "Labels", "agent")}
    </div>
  );
}
