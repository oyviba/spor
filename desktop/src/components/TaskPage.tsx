import { forwardRef } from "react";
import type { LineComment, RepoInfo, TaskDetail } from "../types";
import { statusOf } from "../status";
import { useNow } from "../hooks";
import { ChangesView } from "./ChangesView";
import { Composer, type AgentSettings, type ComposerHandle } from "./Composer";
import { Conversation } from "./Conversation";
import { Inspector } from "./Inspector";
import { MessageSquare, PanelRight } from "./Icons";
import { StatusPill } from "./StatusPill";

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
  onStart: () => void;
  onSend: (text: string) => void;
  onStop: () => void;
  onLand: (merge: boolean, commitUncommitted: boolean) => void;
  onDiscard: () => void;
  onAskRebase: () => void;
  onOpenFolder: () => void;
  onCopy: (text: string, what: string) => void;
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
          <StatusPill status={statusOf(t, p.seenAt, now)} />
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
          <Conversation detail={p.detail} onStop={p.onStop} />
        ) : (
          <ChangesView patch={p.patch} comments={p.comments} onComment={p.onComment} focus={p.focus} />
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
          />
        )}
      </div>
      {!t.landed && (
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
          onStart={p.onStart}
          onSend={p.onSend}
          onStop={p.onStop}
        />
      )}
    </div>
  );
});
