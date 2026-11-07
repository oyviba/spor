// How many agents may run at once in this repository. Past the limit a
// start waits in line and goes by itself when an agent finishes; subagents
// an agent starts are part of it and aren't counted.

import { useEffect, useState } from "react";
import { api } from "../../api";
import { useShell } from "../../shared/app";

export function AgentLimit() {
  const shell = useShell();
  const [saved, setSaved] = useState<number | null | undefined>(undefined);
  const [text, setText] = useState("");

  useEffect(() => {
    api.maxAgents().then(
      (n) => {
        setSaved(n);
        setText(n == null ? "" : String(n));
      },
      shell.fail,
    );
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const parsed = text.trim() === "" ? null : Number(text);
  const valid = parsed === null || (Number.isInteger(parsed) && parsed > 0);
  const save = () => {
    if (!valid || saved === undefined || parsed === saved) return;
    api.setMaxAgents(parsed).then(
      () => {
        setSaved(parsed);
        shell.toast(parsed == null ? "No limit on agents at once" : `At most ${parsed} ${parsed === 1 ? "agent" : "agents"} at once`);
        shell.reloadTasks();
      },
      shell.fail,
    );
  };

  return (
    <div className="settings-page">
      <h3>Agents at once</h3>
      <p className="muted small-text">
        Limit how many agents run at the same time in this repository, so they don&apos;t take all of the machine. A start
        past the limit waits in line and begins when an agent finishes. The subagents an agent starts don&apos;t count.
        Leave it empty for no limit.
      </p>
      <label className="inline-form">
        <input
          type="number"
          min={1}
          step={1}
          inputMode="numeric"
          placeholder="No limit"
          value={text}
          disabled={saved === undefined}
          onChange={(e) => setText(e.target.value)}
          onBlur={save}
          onKeyDown={(e) => e.key === "Enter" && save()}
          aria-label="Agents at once"
          aria-invalid={!valid}
        />
        <span className="muted small-text">agents</span>
      </label>
      {!valid && <p className="small-text">Use a whole number, 1 or more, or leave it empty.</p>}
    </div>
  );
}
