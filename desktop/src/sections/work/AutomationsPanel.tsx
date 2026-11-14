// Automations (#151): tasks that start themselves, from the repository's
// `.spor/automations`. They run while spor is open (see `useAutomations`).

import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { Button } from "../../shared/Button";
import { useShell } from "../../shared/app";
import { ago } from "../agents/transcript";
import type { Automations } from "../../types";

export function AutomationsPanel() {
  const shell = useShell();
  const root = shell.repo.root;
  const [found, setFound] = useState<Automations | null>(null);
  const [open, setOpen] = useState(false);

  const load = useCallback(() => api.automations(root).then(setFound, () => setFound(null)), [root]);
  useEffect(() => {
    load();
  }, [load, shell.tasks.length]);

  const count = found?.items.length ?? 0;
  if (!found || (count === 0 && found.errors.length === 0)) return null;

  return (
    <div className="automations-panel">
      <button className="link small-text" onClick={() => setOpen(!open)} aria-expanded={open}>
        Automations ({count})
      </button>
      {open && (
        <>
          <p className="small-text muted">
            From <code>.spor/automations</code>. They run while spor is open.
          </p>
          {found.errors.map((e) => (
            <p key={e} className="small-text" style={{ color: "var(--red, #e5534b)" }}>
              {e}
            </p>
          ))}
          {found.items.map((a) => (
            <div key={a.name} className="automation-row">
              <div className="automation-head">
                <strong>{a.name}</strong>
                <label className="small-text">
                  <input
                    type="checkbox"
                    checked={a.enabled}
                    onChange={(e) =>
                      api.automationEnable(root, a.name, e.target.checked).then(load, shell.fail)
                    }
                  />{" "}
                  On
                </label>
              </div>
              <div className="small-text muted">{a.on}</div>
              <div className="small-text muted">
                {a.started ? `Last started ${ago(a.started, Date.now())}` : "Hasn't started a task yet"}
                {a.task_open && a.task ? (
                  <>
                    {" · "}
                    <button className="link" onClick={() => shell.go({ section: "work", taskId: a.task! })}>
                      its task is open
                    </button>
                  </>
                ) : null}
              </div>
              {a.note && <div className="small-text muted">{a.note}</div>}
              <Button
                className="small"
                disabled={a.task_open}
                title={a.task_open ? "Its last task is still open" : "Start its task now"}
                onClick={() =>
                  shell
                    .act(async () => {
                      const s = await api.automationRun(root, a.name);
                      await shell.reloadTasks();
                      return `Started ${s.task}`;
                    }, { undo: false })
                    .then(load)
                }
              >
                Run now
              </Button>
            </div>
          ))}
        </>
      )}
    </div>
  );
}
