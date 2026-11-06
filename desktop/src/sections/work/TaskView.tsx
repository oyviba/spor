// An agent's task on the work page (#80): the agent's transcript and the
// composer, its changes (each run's, or all of them), and once it has a
// pull request, that request's discussion and checks too. The details card
// is the task's (landing, publishing, discarding) under the work card.

import { forwardRef, useState } from "react";
import type { Ask, LineComment, Mode, RepoInfo, TaskDetail } from "../../types";
import { isOpen, statusOf } from "../agents/status";
import { CONTEXT_HIGH } from "../agents/transcript";
import { useShell } from "../../shared/app";
import { useNow } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { DiffView } from "../../shared/DiffView";
import { Folder, MessageSquare } from "../../shared/Icons";
import { findWork, itemFromTask } from "../../shared/work";
import { Composer, type AgentSettings, type ComposerHandle } from "../agents/Composer";
import { Conversation } from "../agents/Conversation";
import { Inspector, type GithubActions } from "../agents/Inspector";
import { WorkPage, shownTab, type WorkTab } from "./WorkPage";
import { PullChecks, ReviewBox, Timeline, usePullDetail } from "./pull";

interface Props {
  repo: RepoInfo;
  detail: TaskDetail;
  seenAt: number | undefined;
  tab: WorkTab;
  onTab: (t: WorkTab) => void;
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
  onCopy: (text: string, what: string) => void;
  /** Open its folder, to work in it yourself. */
  onOpenFolder: () => unknown;
  /** What Changes shows: everything, or run N's changes. */
  source: "all" | number;
  onSource: (s: "all" | number) => void;
  /** For one run: files left out because they came with the base. */
  fromBase: string[];
  onRunChanges: (index: number) => void;
  /** Undo the last run, or (`from`) that run and every run after it. */
  onUndoRun: (from: number | null) => unknown;
  /** Run the setup script again before the next agent. */
  onRerunSetup: () => unknown;
  onAnswer: (ask: Ask, allow: boolean, message?: string) => unknown;
  /** Allow it, and don't ask again about this kind of action this session. */
  onAllowForSession: (ask: Ask) => unknown;
  onAnswerQuestions: (ask: Ask, answers: string[][]) => unknown;
  onApprovePlan: (ask: Ask, mode: Mode) => unknown;
  onLiveMode: (mode: Mode) => void;
  onSteer: (text: string) => unknown;
  onRetrySetup: () => unknown;
  onSkipSetup: () => unknown;
  github: GithubActions;
}

export const TaskView = forwardRef<ComposerHandle, Props>(function TaskView(p, composerRef) {
  const shell = useShell();
  const t = p.detail.task;
  const latest = p.detail.runs.at(-1) ?? null;
  const now = useNow(1000, latest?.state === "running");
  const pull = p.github.pull?.pull;
  const item = findWork(shell.work, { task: t.id }) ?? itemFromTask(t, pull);
  // The pull request's own comments, apart from the ones for the agent.
  const [prComments, setPrComments] = useState<LineComment[]>([]);
  const tabs: { id: WorkTab; extra?: React.ReactNode }[] = [
    { id: "agent" },
    {
      id: "changes",
      extra: (
        <>
          {t.stats.files > 0 && <span className="tab-count num">{t.stats.files}</span>}
          {p.comments.length > 0 && (
            <span className="tab-count accent num" title={`${p.comments.length} review comments to send`}>
              <MessageSquare size={10} /> {p.comments.length}
            </span>
          )}
        </>
      ),
    },
    ...(pull ? [{ id: "discussion" as const }, { id: "checks" as const }] : []),
  ];
  const tab = shownTab(tabs, p.tab);
  const { pr, reload } = usePullDetail(pull && (tab === "discussion" || tab === "checks") ? pull.number : null, false);

  const composer = isOpen(t) && (
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
      onAnswerQuestions={p.onAnswerQuestions}
      onApprovePlan={p.onApprovePlan}
      liveMode={p.detail.listening ? (t.mode ?? null) : null}
      onLiveMode={p.onLiveMode}
      canSteer={p.detail.listening}
      onSteer={p.onSteer}
      onStart={p.onStart}
      onSend={p.onSend}
      onStop={p.onStop}
    />
  );
  // On Discussion you talk to the pull request; everywhere else, to the agent.
  const dock =
    tab === "discussion" && pr ? (
      pr.pull.state !== "merged" && (
        <ReviewBox
          pr={pr}
          comments={prComments}
          onRemoveComment={(id) => setPrComments((c) => c.filter((x) => x.id !== id))}
          onSent={() => {
            setPrComments([]);
            reload();
          }}
        />
      )
    ) : (
      composer
    );

  return (
    <WorkPage
      item={item}
      title={t.title}
      hint={t.prompt}
      status={statusOf(t, p.seenAt, now, pull)}
      action={
        isOpen(t) && (
          <>
            {latest?.state === "running" && latest.context != null && (
              <span
                className={`context-chip num${latest.context >= CONTEXT_HIGH ? " high" : ""}`}
                title={
                  latest.context >= CONTEXT_HIGH
                    ? "The agent's context is nearly full. Claude Code will compact the conversation soon, and the agent may forget earlier decisions"
                    : "How much of the agent's context window is in use"
                }
              >
                context {latest.context}%
              </span>
            )}
            <Button className="ghost" onClick={p.onOpenFolder} title={`Open ${t.worktree} and work in it yourself`}>
              <Folder size={12} /> Take over
            </Button>
          </>
        )
      }
      tabs={tabs}
      tab={tab}
      onTab={p.onTab}
      inspector={p.inspector}
      onInspector={p.onInspector}
      details={
        <Inspector
          detail={p.detail}
          baseCheckedOut={p.repo.branch === t.base}
          onCopy={p.onCopy}
          onShowChanges={() => p.onTab("changes")}
          onLand={p.onLand}
          onDiscard={p.onDiscard}
          onAskRebase={p.onAskRebase}
          onAskRestore={p.onAskRestore}
          onRerunSetup={p.onRerunSetup}
          {...p.github}
        />
      }
      dock={dock}
    >
      {tab === "agent" ? (
        <Conversation
          detail={p.detail}
          onStop={p.onStop}
          onRunChanges={p.onRunChanges}
          onUndoRun={p.onUndoRun}
          onRetrySetup={p.onRetrySetup}
          onSkipSetup={p.onSkipSetup}
        />
      ) : tab === "changes" ? (
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
      ) : !pr ? (
        <div className="boot" />
      ) : tab === "discussion" ? (
        <div className="conversation">
          <div className="column">
            <Timeline pr={pr} />
          </div>
        </div>
      ) : (
        <div className="conversation">
          <div className="column">
            <PullChecks pr={pr} />
          </div>
        </div>
      )}
    </WorkPage>
  );
});
