import type { TaskDetail } from "../../types";
import { Alert, AgentIcon, X } from "../../shared/Icons";
import { duration, tokens } from "./transcript";
import { PANE_LINES, runFacts, type PaneOptions } from "./panel";

interface Props {
  detail: TaskDetail;
  /** What the pane's lines are set to; with `onOptions`, the dialog offers to change them. */
  options: PaneOptions;
  onOptions: (o: PaneOptions) => void;
  /** Which part of the dialog to open on. */
  focus: "details" | "panel";
  onClose: () => void;
  setup: React.ReactNode;
}

/** Everything about the runs the pane leaves out (cost, model, tools that
 * didn't connect, the setup script), and what the pane shows. */
export function RunDetails({ detail, options, onOptions, focus, onClose, setup }: Props) {
  const t = detail.task;
  const running = t.run?.state === "running";
  const facts = runFacts(detail, running);
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className="dialog run-details"
        role="dialog"
        aria-label={focus === "panel" ? "What the panel shows" : "Run details"}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <div className="dialog-head">
          <h3>{focus === "panel" ? "What the panel shows" : "Run details"}</h3>
          <button className="icon-button" onClick={onClose} aria-label="Close">
            <X size={13} />
          </button>
        </div>
        {focus === "details" && (
          <>
            {facts.mcpFailed.length > 0 && (
              <div className="detail-row warn">
                <Alert size={13} />
                <span>
                  {facts.mcpFailed.join(", ")} didn't connect on the last run, so the agent had no {facts.mcpFailed.length === 1 ? "tool" : "tools"} from{" "}
                  {facts.mcpFailed.length === 1 ? "it" : "them"}
                </span>
              </div>
            )}
            {t.stats.base_moved && (
              <div className="detail-row warn">
                <Alert size={13} />
                <span>{t.from ?? t.base} has moved on — landing rebases first</span>
              </div>
            )}
            <dl className="detail-list">
              {t.from && (
                <>
                  <dt>Started from</dt>
                  <dd>{t.from}</dd>
                </>
              )}
              {facts.model && (
                <>
                  <dt>Model</dt>
                  <dd className="num">{facts.model}</dd>
                </>
              )}
              {facts.checks && (
                <>
                  <dt>Checked</dt>
                  <dd className="num" title="Read from the commands the agent ran">
                    {facts.checks}
                  </dd>
                </>
              )}
              {detail.setup && (
                <>
                  <dt>Setup</dt>
                  <dd>{setup}</dd>
                </>
              )}
            </dl>
            {detail.runs.length > 0 && (
              <ul className="run-list">
                {detail.runs.map((r) => (
                  <li key={r.index} className={r.undone ? "undone" : ""}>
                    <AgentIcon agent={r.agent} />
                    <span className="truncate" title={r.prompt}>
                      {r.index + 1}. {r.prompt.split("\n")[0] || r.label}
                    </span>
                    <span className="num muted">
                      {r.ended ? duration(r.ended - r.started) : "running"}
                      {r.cost_usd ? ` · $${r.cost_usd.toFixed(2)}` : ""}
                      {r.tokens_in + r.tokens_out > 0 ? ` · ${tokens(r.tokens_in + r.tokens_out)}` : ""}
                      {r.undone ? " · undone" : ""}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
        {focus === "panel" && (
          <>
            <p className="muted small-text">
              The panel keeps to what you act on: the work card, what needs you, the changes and the next step. These you can add or take away.
            </p>
            {PANE_LINES.map((l) => (
              <label key={l.id} className="pane-option">
                <input type="checkbox" checked={options[l.id]} onChange={(e) => onOptions({ ...options, [l.id]: e.target.checked })} />
                <span>
                  {l.label}
                  <span className="muted small-text">{l.hint}</span>
                </span>
              </label>
            ))}
          </>
        )}
      </div>
    </div>
  );
}
