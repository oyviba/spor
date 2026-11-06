import { forwardRef } from "react";
import type { Ask, LineComment, Mode, RepoInfo, TaskDetail } from "../../types";
import { isOpen, statusOf } from "./status";
import { useNow } from "../../shared/hooks";
import { DiffView } from "../../shared/DiffView";
import { Composer, type AgentSettings, type ComposerHandle } from "./Composer";
import { Conversation } from "./Conversation";
import { Inspector, type GithubActions } from "./Inspector";
import { MessageSquare, PanelRight } from "../../shared/Icons";
import { StatusPill } from "../../shared/StatusPill";

export type Tab = "conversation" | "changes";

interface Props {
  repo: RepoInfo;
  detail: TaskDetail;
  seenAt: number | undefined;
  tab: Tab;
  onTab: (t: Tab) => void;
  patch: string | null;
  inspector: boolean;
  onInspector: (open: boolean) => void;
  settings: AgentSettings;
  onSettings: (s: AgentSettings) => void;
  comments: LineComment[];
  onComment: (file: string, line: number, side: LineComment["side"], text: string) => void;
  onRemoveComment: (id: string) => void;
  focus: { file: string; line: number; side: LineComment["side"] } | null;
  onFocus: (c: LineComment) => void;
  queued: string[];
  onUnqueue: (i: number) => void;
  onStart: () => unknown;
  onSend: (text: string) => unknown;
  onStop: () => unknown;
  onLand: (merge: boolean, commitUncommitted: boolean) => unknown;
  onDiscard: () => unknown;
  onAskRebase: () => void;
  onAskRestore: () => void;
  onOpenFolder: () => unknown;
  onCopy: (text: string, what: string) => void;
  onShowBranch: () => void;
  /** What Changes shows: everything, or run N's changes. */
  source: "all" | number;
  onSource: (s: "all" | number) => void;
  /** For one run: files left out because they came with the base. */
  fromBase: string[];
  onRunChanges: (index: number) => void;
  onUndoRun: () => unknown;
  onAnswer: (ask: Ask, allow: boolean, message?: string) => unknown;
  /** Allow it, and don't ask again about this kind of action this session. */
  onAllowForSession: (ask: Ask) => unknown;
  onLiveMode: (mode: Mode) => void;
  onSteer: (text: string) => unknown;
  onRetrySetup: () => unknown;
  onSkipSetup: () => unknown;
  github: GithubActions;
}

export const TaskPage = forwardRef<ComposerHandle, Props>(function TaskPage(p, composerRef) {
  const t = p.detail.task;
  const latest = p.detail.runs.at(-1) ?? null;
  const now = useNow(1000, latest?.state === "running");
  return (
    <div className={`task-page${p.inspector ? " with-inspector" : ""}`}>
      <header className="task-header">
        <div className="task-title">
          <h2 title={t.prompt}>{t.title}</h2>
          <StatusPill status={statusOf(t, p.seenAt, now, p.github.pull?.pull)} />
        </div>
        <nav className="tabs" role="tablist">
          <button role="tab" aria-selected={p.tab === "conversation"} onClick={() => p.onTab("conversation")}>
            Conversation
          </button>
          <button role="tab" aria-selected={p.tab === "changes"} onClick={() => p.onTab("changes")}>
            Changes
            {t.stats.files > 0 && <span className="tab-count num">{t.stats.files}</span>}
            {p.comments.length > 0 && (
              <span className="tab-count accent num" title={`${p.comments.length} review comments to send`}>
                <MessageSquare size={10} /> {p.comments.length}
              </span>
            )}
          </button>
        </nav>
        <button
          className={`icon-button${p.inspector ? " active" : ""}`}
          onClick={() => p.onInspector(!p.inspector)}
          title="Details (⌘.)"
          aria-label="Toggle details"
        >
          <PanelRight />
        </button>
      </header>
      <div className="task-body">
        {p.tab === "conversation" ? (
          <Conversation
            detail={p.detail}
            onStop={p.onStop}
            onRunChanges={p.onRunChanges}
            onUndoRun={p.onUndoRun}
            onRetrySetup={p.onRetrySetup}
            onSkipSetup={p.onSkipSetup}
          />
        ) : (
          <div className="changes">
            <div className="changes-column">
              {p.detail.runs.length > 0 && (
                <div className="source-bar">
                  <select
                    className="chip-select"
                    value={String(p.source)}
                    onChange={(e) => p.onSource(e.target.value === "all" ? "all" : Number(e.target.value))}
                    aria-label="Which changes"
                  >
                    <option value="all">All changes against {t.base}</option>
                    {p.detail.runs.map((r) => (
                      <option key={r.index} value={r.index}>
                        Run {r.index + 1}: {r.prompt.split("\n")[0].slice(0, 60)}
                        {r.undone ? " (undone)" : ""}
                      </option>
                    ))}
                  </select>
                  {p.fromBase.length > 0 && (
                    <span className="muted small-text" title={p.fromBase.join("\n")}>
                      {p.fromBase.length} {p.fromBase.length === 1 ? "file" : "files"} from {t.base} left out
                    </span>
                  )}
                </div>
              )}
              <DiffView
                patch={p.patch}
                empty={p.source === "all" ? "No changes yet." : "This run changed no files."}
                comments={p.comments}
                onComment={p.onComment}
                focus={p.focus}
              />
            </div>
          </div>
        )}
        {p.inspector && (
          <Inspector
            detail={p.detail}
            baseCheckedOut={p.repo.branch === t.base}
            onOpenFolder={p.onOpenFolder}
            onCopy={p.onCopy}
            onShowChanges={() => p.onTab("changes")}
            onLand={p.onLand}
            onDiscard={p.onDiscard}
            onAskRebase={p.onAskRebase}
            onAskRestore={p.onAskRestore}
            onShowBranch={p.onShowBranch}
            {...p.github}
          />
        )}
      </div>
      {isOpen(t) && (
        <Composer
          ref={composerRef}
          repo={p.repo}
          latest={latest}
          settings={p.settings}
          onSettings={p.onSettings}
          comments={p.comments}
          onRemoveComment={p.onRemoveComment}
          onJumpToComment={p.onFocus}
          queued={p.queued}
          onUnqueue={p.onUnqueue}
          asks={p.detail.asks}
          onAnswer={p.onAnswer}
          onAllowForSession={p.onAllowForSession}
          liveMode={p.detail.listening ? (t.mode ?? null) : null}
          onLiveMode={p.onLiveMode}
          canSteer={p.detail.listening}
          onSteer={p.onSteer}
          onStart={p.onStart}
          onSend={p.onSend}
          onStop={p.onStop}
        />
      )}
    </div>
  );
});
