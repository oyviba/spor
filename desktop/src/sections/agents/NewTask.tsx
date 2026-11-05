import { useEffect, useRef, useState } from "react";
import type { RepoInfo } from "../../types";
import { ModePicker, type AgentSettings } from "./Composer";
import { ArrowUp, Branch } from "../../shared/Icons";
import { Button } from "../../shared/Button";
import { useShell } from "../../shared/app";
import { AttachButton, AttachmentStrip, useAttachments, withImages } from "../../shared/Attachments";

interface Props {
  repo: RepoInfo;
  settings: AgentSettings;
  onSettings: (s: AgentSettings) => void;
  /** `background`: start it and stay here for the next one (⌘↩). */
  onCreate: (prompt: string, base: string, background: boolean) => Promise<boolean>;
}

/** A blank task: what to do, from which branch, with which agent. */
export function NewTask({ repo, settings, onSettings, onCreate }: Props) {
  const [prompt, setPrompt] = useState("");
  const [base, setBase] = useState(repo.branch ?? repo.default_base ?? repo.branches[0] ?? "main");
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  useEffect(() => input.current?.focus(), []);
  const attachments = useAttachments(useShell().fail);

  const submit = async (background: boolean) => {
    if (!prompt.trim() || busy || attachments.saving) return;
    setBusy(true);
    const ok = await onCreate(withImages(prompt, attachments.images), base, background);
    setBusy(false);
    if (ok && background) {
      setPrompt("");
      attachments.clear();
      input.current?.focus();
    }
  };
  const verb = settings.agent === "none" ? "Create" : "Start";

  return (
    <div className="new-task">
      <div className="new-task-inner">
        <h1>What should an agent work on?</h1>
        <p className="lede">
          It gets its own branch and folder from <b>{base}</b>, so it never touches your checkout.
        </p>
        <div className={`composer big${attachments.dragging ? " dropping" : ""}`} {...attachments.handlers}>
          <AttachmentStrip images={attachments.images} onRemove={attachments.remove} />
          <textarea
            ref={input}
            rows={4}
            value={prompt}
            placeholder="Describe the task. The first line becomes its title."
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                submit(e.metaKey || e.ctrlKey);
              }
            }}
          />
          <div className="composer-footer">
            <label className="chip-select-wrap" title="The branch it starts from and lands on">
              <Branch size={12} />
              <select className="chip-select bare" value={base} onChange={(e) => setBase(e.target.value)}>
                {repo.branches.map((b) => (
                  <option key={b}>{b}</option>
                ))}
              </select>
            </label>
            <select
              className="chip-select"
              value={settings.agent}
              onChange={(e) => onSettings({ ...settings, agent: e.target.value as AgentSettings["agent"] })}
            >
              <option value="claude" disabled={!repo.claude}>
                Claude Code{repo.claude ? "" : " (not installed)"}
              </option>
              <option value="command">Custom command</option>
              <option value="none">No agent — I'll run one</option>
            </select>
            {settings.agent === "claude" && (
              <ModePicker mode={settings.mode} onChange={(mode) => onSettings({ ...settings, mode })} />
            )}
            <span className="flex-1" />
            <AttachButton onFiles={attachments.add} />
            <span className="hint-keys">↩ {verb.toLowerCase()} · ⌘↩ {verb.toLowerCase()} &amp; next</span>
            <Button className="send" disabled={!prompt.trim() || attachments.saving} busy={busy} onClick={() => submit(false)} title={`${verb} (↩)`}>
              <ArrowUp size={14} />
            </Button>
          </div>
        </div>
        {settings.agent === "command" && (
          <label className="field">
            <span>Agent command — <code>{"{prompt}"}</code> is replaced with the task</span>
            <input
              className="mono"
              value={settings.template}
              placeholder="codex exec {prompt}"
              onChange={(e) => onSettings({ ...settings, template: e.target.value })}
            />
          </label>
        )}
      </div>
    </div>
  );
}
