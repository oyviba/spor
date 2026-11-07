// Rules worth writing down (#107): corrections people keep giving agents,
// found across the repository's tasks, offered as lines for CLAUDE.md.

import { useState } from "react";
import { api } from "../../api";
import { Button } from "../../shared/Button";
import { useShell } from "../../shared/app";
import type { RuleSuggestion } from "../../types";

export function RulesPanel() {
  const shell = useShell();
  const [found, setFound] = useState<RuleSuggestion[] | null>(null);
  const [looking, setLooking] = useState(false);
  const [edited, setEdited] = useState<Record<string, string>>({});

  const look = async () => {
    setLooking(true);
    try {
      setFound(await api.suggestRules());
      setEdited({});
    } catch (e) {
      shell.fail(e);
    } finally {
      setLooking(false);
    }
  };
  const drop = (rule: string) => setFound((f) => f?.filter((s) => s.rule !== rule) ?? null);
  const add = (s: RuleSuggestion) =>
    shell
      .act(() => api.addRule(edited[s.rule] ?? s.rule), { undo: false })
      .then((ok) => ok && drop(s.rule));
  const decline = (s: RuleSuggestion) => api.declineRule(s.rule).then(() => drop(s.rule), shell.fail);

  return (
    <div className="rules-panel">
      <button
        className="link small-text"
        onClick={look}
        disabled={looking}
        title="Ask Claude to look through the follow-ups you've sent in every task, for instructions you keep repeating"
      >
        {looking ? "Looking through your follow-ups…" : "Look for rules to write down"}
      </button>
      {found && found.length === 0 && <p className="small-text muted">Nothing is repeated across tasks yet.</p>}
      {found?.map((s) => (
        <div key={s.rule} className="rule-suggestion">
          <p className="small-text">
            You've told agents this in {s.tasks.length} tasks. Add to CLAUDE.md?
          </p>
          <textarea
            value={edited[s.rule] ?? s.rule}
            onChange={(e) => setEdited((m) => ({ ...m, [s.rule]: e.target.value }))}
            rows={2}
            aria-label="The line to add"
          />
          <div className="rule-actions">
            <Button className="small" onClick={() => add(s)}>
              Add
            </Button>
            <Button className="ghost small" onClick={() => decline(s)}>
              Not this one
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
}
