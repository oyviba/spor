import { forwardRef, useImperativeHandle, useRef, useState } from "react";
import type { AgentChoice, Ask, LineComment, Mode, RepoInfo, RunInfo } from "../../types";
import { useStored } from "../../shared/hooks";
import { AgentIcon, Alert, ArrowUp, Check, Clock, CornerDownRight, MessageSquare, Square, X } from "../../shared/Icons";

export interface AgentSettings {
  agent: "claude" | "command" | "none";
  template: string;
  mode: Mode;
}

export const MODES: { id: Mode; label: string; hint: string }[] = [
  { id: "supervised", label: "Supervised", hint: "Asks before commands and file edits" },
  { id: "acceptEdits", label: "Auto-accept edits", hint: "Edits freely, asks before commands" },
  { id: "auto", label: "Auto", hint: "Claude's own review approves routine actions and asks about the rest" },
  { id: "full", label: "Full access", hint: "Never asks. git push is refused in every mode" },
];

/** The agent settings new runs use, kept between sessions. Settings saved
 * before modes existed ("Edits + commands") carry over. */
export function useAgentSettings(): [AgentSettings, (s: AgentSettings) => void] {
  const [stored, set] = useStored<AgentSettings & { allowCommands?: boolean }>("agent", {
    agent: "claude",
    template: "",
    mode: "acceptEdits",
  });
  const mode = stored.mode ?? (stored.allowCommands ? "full" : "acceptEdits");
  return [{ agent: stored.agent, template: stored.template, mode }, set];
}

export function ModePicker({ mode, onChange }: { mode: Mode; onChange: (m: Mode) => void }) {
  const current = MODES.find((m) => m.id === mode) ?? MODES[1];
  return (
    <select
      className={`chip-select mode-${mode}`}
      value={mode}
      onChange={(e) => onChange(e.target.value as Mode)}
      title={`${current.label}: ${current.hint}`}
      aria-label="Permission mode"
    >
      {MODES.map((m) => (
        <option key={m.id} value={m.id} title={m.hint}>
          {m.label}
        </option>
      ))}
    </select>
  );
}

export function choiceOf(s: AgentSettings): AgentChoice {
  return s.agent === "command" ? { kind: "command", template: s.template } : { kind: s.agent };
}

interface Props {
  repo: RepoInfo;
  latest: RunInfo | null;
  settings: AgentSettings;
  onSettings: (s: AgentSettings) => void;
  comments: LineComment[];
  onRemoveComment: (id: string) => void;
  onJumpToComment: (c: LineComment) => void;
  queued: string[];
  onUnqueue: (index: number) => void;
  /** Questions the agent is waiting on (it runs under the host). */
  asks: Ask[];
  onAnswer: (ask: Ask, allow: boolean, message?: string) => void;
  onAllowForSession: (ask: Ask) => void;
  /** The running agent's mode, changeable while it works; null otherwise. */
  liveMode: Mode | null;
  onLiveMode: (m: Mode) => void;
  /** Start the task's own prompt (no runs yet). */
  onStart: () => void;
  /** A follow-up; queued by the caller when the agent is working. */
  onSend: (text: string) => void;
  /** The running agent can take a message into the turn it's on (#22). */
  canSteer: boolean;
  onSteer: (text: string) => void;
  onStop: () => void;
}

export interface ComposerHandle {
  focus: () => void;
}

/** "Comments on the changes:", as the agent is sent them. */
export function withComments(text: string, comments: LineComment[]): string {
  if (comments.length === 0) return text.trim();
  const notes = comments
    .map((c) => `- ${c.file}:${c.line}${c.side === "old" ? " (removed line)" : ""}: ${c.text}`)
    .join("\n");
  return `${text.trim()}${text.trim() ? "\n\n" : ""}Comments on the changes:\n${notes}`;
}

/** "run `make test`", "edit greet.py": what a tool use is, for a question. */
export function verbOf(tool: string): string {
  switch (tool) {
    case "Bash":
      return "run a command";
    case "Edit":
    case "MultiEdit":
    case "Write":
    case "NotebookEdit":
      return "edit a file";
    case "WebFetch":
      return "fetch a page";
    case "WebSearch":
      return "search the web";
    default:
      return `use ${tool}`;
  }
}

export const Composer = forwardRef<ComposerHandle, Props>(function Composer(p, ref) {
  const [text, setText] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  useImperativeHandle(ref, () => ({ focus: () => input.current?.focus() }));

  const running = p.latest?.state === "running";
  const hasContent = text.trim().length > 0 || p.comments.length > 0;
  const neverRan = p.latest == null;
  const ask = p.asks[0];
  // While it works, a message waits for the next turn (queue) or goes into
  // this one (steer): your default on ↩, the other on ⌘↩ (#22).
  const [followUp, setFollowUp] = useStored<"queue" | "steer">("followUp", "queue");
  const later = followUp === "steer" && p.canSteer ? "steer" : "queue";
  const other = later === "steer" ? "queue" : p.canSteer ? "steer" : "queue";
  // One button that follows the task's state. A question waiting: typing
  // answers it with a "no, do this instead".
  const actionFor = (flip: boolean): "start" | "send" | "queue" | "steer" | "stop" | "reply" =>
    ask && hasContent
      ? "reply"
      : running
        ? hasContent
          ? flip
            ? other
            : later
          : "stop"
        : neverRan && !hasContent
          ? "start"
          : "send";
  const action = actionFor(false);
  const disabled = (action === "send" && !hasContent) || (action === "start" && p.settings.agent === "none");

  const submit = (flip = false) => {
    const act = actionFor(flip);
    if (disabled) return;
    if (act === "stop") return p.onStop();
    if (act === "start") return p.onStart();
    const message = withComments(text, p.comments);
    if (act === "reply") p.onAnswer(ask!, false, message);
    else if (act === "steer") p.onSteer(message);
    else p.onSend(message);
    setText("");
  };

  const placeholder = ask
    ? "Or say what to do instead — that answers no"
    : running
      ? p.canSteer
        ? "Steer the agent, or queue a follow-up for when it's done"
        : "Add a follow-up — it's sent when the agent finishes"
      : neverRan
        ? "Start the agent on this task, or add to the prompt first…"
        : "Tell the agent what to change…";

  const followUpAgent = p.latest?.agent;

  return (
    <div className="composer-dock">
      <div className="column">
        {p.queued.length > 0 && (
          <div className="queued">
            {p.queued.map((q, i) => (
              <div key={i} className="queued-item">
                <Clock size={12} />
                <span className="truncate">{q}</span>
                <span className="queued-note">sends when done</span>
                <button className="icon-button tiny" onClick={() => p.onUnqueue(i)} aria-label="Remove queued message">
                  <X size={11} />
                </button>
              </div>
            ))}
          </div>
        )}
        <div className={`composer${ask ? " asking" : ""}`}>
          {ask && (
            <div className="ask-card" role="alert">
              <Alert size={14} />
              <div className="ask-text">
                <div>
                  <b>Wants to {verbOf(ask.tool)}</b>
                  {p.asks.length > 1 && <span className="muted"> · {p.asks.length - 1} more waiting</span>}
                </div>
                {ask.detail && <code className="ask-detail">{ask.detail}</code>}
              </div>
              <button className="ghost small" onClick={() => p.onAnswer(ask, false)}>
                <X size={11} /> Deny
              </button>
              {ask.session && (
                <button
                  className="ghost small"
                  onClick={() => p.onAllowForSession(ask)}
                  title={`Allow it, and don't ask again this session about ${ask.session.replace(/`/g, "")}`}
                >
                  Allow for this session
                </button>
              )}
              <button className="primary small" onClick={() => p.onAnswer(ask, true)}>
                <Check size={11} /> Allow
              </button>
            </div>
          )}
          {p.comments.length > 0 && (
            <div className="chips">
              {p.comments.map((c) => (
                <span key={c.id} className="chip" title={c.text}>
                  <button className="chip-main" onClick={() => p.onJumpToComment(c)}>
                    <MessageSquare size={11} />
                    <span className="mono">
                      {c.file.split("/").pop()}:{c.line}
                    </span>
                    <span className="truncate">{c.text}</span>
                  </button>
                  <button className="chip-x" onClick={() => p.onRemoveComment(c.id)} aria-label="Remove comment">
                    <X size={10} />
                  </button>
                </span>
              ))}
            </div>
          )}
          <textarea
            ref={input}
            value={text}
            rows={1}
            placeholder={placeholder}
            onChange={(e) => {
              setText(e.target.value);
              const el = e.currentTarget;
              el.style.height = "auto";
              el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                // ⌘↩ / Ctrl+↩: the other of queue and steer.
                submit(e.metaKey || e.ctrlKey);
              }
            }}
          />
          <div className="composer-footer">
            {followUpAgent ? (
              <span className="chip-static" title="Follow-ups go to the agent that did the work">
                <AgentIcon agent={followUpAgent} size={12} />
                {followUpAgent === "claude" ? "Claude Code" : "Custom command"}
              </span>
            ) : (
              <select
                className="chip-select"
                value={p.settings.agent}
                onChange={(e) => p.onSettings({ ...p.settings, agent: e.target.value as AgentSettings["agent"] })}
              >
                <option value="claude" disabled={!p.repo.claude}>
                  Claude Code{p.repo.claude ? "" : " (not installed)"}
                </option>
                <option value="command">Custom command</option>
                <option value="none">No agent</option>
              </select>
            )}
            <ModePicker
              mode={p.liveMode ?? p.settings.mode}
              onChange={(mode) => {
                p.onSettings({ ...p.settings, mode });
                if (p.liveMode) p.onLiveMode(mode);
              }}
            />
            {running && p.canSteer && !ask && (
              <button
                className="chip-toggle"
                onClick={() => setFollowUp(followUp === "steer" ? "queue" : "steer")}
                title="What ↩ does while the agent works: queue the message for its next turn, or steer the turn it's on. ⌘↩ does the other."
              >
                ↩ {later === "steer" ? "steers" : "queues"}
              </button>
            )}
            <span className="flex-1" />
            <span className="hint-keys">
              {action === "queue" || action === "steer"
                ? `↩ ${action}${other !== action ? ` · ⌘↩ ${other}` : ""}`
                : action === "reply"
                  ? "↩ deny with this reply"
                  : "↩ send · ⇧↩ new line"}
            </span>
            <button
              className={`send ${action}`}
              onClick={() => submit()}
              disabled={disabled}
              title={
                action === "stop"
                  ? "Stop the agent: it ends the turn and keeps the conversation"
                  : action === "queue"
                    ? "Queue: sent when the agent finishes"
                    : action === "steer"
                      ? "Steer: the agent takes it into the turn it's on"
                      : action === "start"
                        ? "Start the agent on the task's prompt"
                        : action === "reply"
                          ? "Deny, and tell the agent this instead"
                          : "Send to the agent"
              }
            >
              {action === "stop" ? (
                <Square size={11} />
              ) : action === "start" ? (
                <span className="send-label">Start</span>
              ) : action === "queue" ? (
                <Clock size={13} />
              ) : action === "steer" ? (
                <CornerDownRight size={14} />
              ) : (
                <ArrowUp size={14} />
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
});
