import { forwardRef, useImperativeHandle, useRef, useState } from "react";
import type { AgentChoice, LineComment, RepoInfo, RunInfo } from "../../types";
import { AgentIcon, ArrowUp, Clock, MessageSquare, Square, X } from "../../shared/Icons";

export interface AgentSettings {
  agent: "claude" | "command" | "none";
  template: string;
  allowCommands: boolean;
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
  /** Start the task's own prompt (no runs yet). */
  onStart: () => void;
  /** A follow-up; queued by the caller when the agent is working. */
  onSend: (text: string) => void;
  onStop: () => void;
}

export interface ComposerHandle {
  focus: () => void;
}

/** "Comments on the changes:" as the GPUI app sends them. */
export function withComments(text: string, comments: LineComment[]): string {
  if (comments.length === 0) return text.trim();
  const notes = comments
    .map((c) => `- ${c.file}:${c.line}${c.side === "old" ? " (removed line)" : ""}: ${c.text}`)
    .join("\n");
  return `${text.trim()}${text.trim() ? "\n\n" : ""}Comments on the changes:\n${notes}`;
}

export const Composer = forwardRef<ComposerHandle, Props>(function Composer(p, ref) {
  const [text, setText] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  useImperativeHandle(ref, () => ({ focus: () => input.current?.focus() }));

  const running = p.latest?.state === "running";
  const hasContent = text.trim().length > 0 || p.comments.length > 0;
  const neverRan = p.latest == null;
  // One button that follows the task's state.
  const action: "start" | "send" | "queue" | "stop" = running
    ? hasContent
      ? "queue"
      : "stop"
    : neverRan && !hasContent
      ? "start"
      : "send";
  const disabled = (action === "send" && !hasContent) || (action === "start" && p.settings.agent === "none");

  const submit = () => {
    if (disabled) return;
    if (action === "stop") return p.onStop();
    if (action === "start") return p.onStart();
    p.onSend(withComments(text, p.comments));
    setText("");
  };

  const placeholder = running
    ? "Add a follow-up — it's sent when the agent finishes"
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
        <div className="composer">
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
                submit();
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
            <button
              className={`chip-toggle${p.settings.allowCommands ? " on" : ""}`}
              onClick={() => p.onSettings({ ...p.settings, allowCommands: !p.settings.allowCommands })}
              title="Claude Code may always edit files. Shell commands are refused unless this is on; git push never runs."
            >
              {p.settings.allowCommands ? "Edits + commands" : "Edits only"}
            </button>
            <span className="flex-1" />
            <span className="hint-keys">{action === "queue" ? "↩ queue" : "↩ send · ⇧↩ new line"}</span>
            <button
              className={`send ${action}`}
              onClick={submit}
              disabled={disabled}
              title={
                action === "stop"
                  ? "Stop the agent"
                  : action === "queue"
                    ? "Queue: sent when the agent finishes"
                    : action === "start"
                      ? "Start the agent on the task's prompt"
                      : "Send to the agent"
              }
            >
              {action === "stop" ? (
                <Square size={11} />
              ) : action === "start" ? (
                <span className="send-label">Start</span>
              ) : action === "queue" ? (
                <Clock size={13} />
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
