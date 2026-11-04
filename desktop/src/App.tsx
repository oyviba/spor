import { useCallback, useEffect, useMemo, useState } from "react";
import { api, errorText } from "./api";
import type { RepoInfo, RepoState } from "./types";
import { usePoll, useStored } from "./shared/hooks";
import { ShellContext, useShell, type SectionId, type Shell, type Target, type ToastAction } from "./shared/app";
import { Alert, Bot, Branch, FileIcon, History as HistoryIcon, Play, PullRequest, X } from "./shared/Icons";
import { AgentsSection } from "./sections/agents/AgentsSection";
import { HistorySection } from "./sections/history/HistorySection";
import { ChangesSection } from "./sections/changes/ChangesSection";
import { PullsSection } from "./sections/pulls/PullsSection";
import { ActionsSection } from "./sections/actions/ActionsSection";

interface Toast {
  id: number;
  text: string;
  tone: "ok" | "error";
  undo?: boolean;
  action?: ToastAction;
}

const SECTIONS: { id: SectionId; label: string; icon: React.ReactNode; key: string }[] = [
  { id: "changes", label: "Changes", icon: <FileIcon size={17} />, key: "1" },
  { id: "history", label: "History", icon: <HistoryIcon size={17} />, key: "2" },
  { id: "pulls", label: "Pull requests", icon: <PullRequest size={17} />, key: "3" },
  { id: "actions", label: "Actions", icon: <Play size={17} />, key: "4" },
  { id: "agents", label: "Agents", icon: <Bot size={17} />, key: "5" },
];

export function App() {
  const [repo, setRepo] = useState<RepoInfo | null | undefined>(undefined);
  const [state, setState] = useState<RepoState | null>(null);
  const [section, setSection] = useStored<SectionId>("section", "agents");
  const [target, setTarget] = useState<Target & { seq: number }>({ section: "agents", seq: 0 });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [badges, setBadges] = useState<{ changes: number; working: number }>({ changes: 0, working: 0 });

  const toast = useCallback((text: string, tone: Toast["tone"] = "ok", undo = false, action?: ToastAction) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-2), { id, text, tone, undo, action }]);
    setTimeout(
      () => setToasts((t) => t.filter((x) => x.id !== id)),
      action ? 10000 : undo ? 6000 : tone === "error" ? 7000 : 4000,
    );
  }, []);
  const fail = useCallback((e: unknown) => toast(errorText(e), "error"), [toast]);

  useEffect(() => {
    api.currentRepo().then(setRepo, () => setRepo(null));
  }, []);

  const refresh = useCallback(async () => {
    if (!repo) return;
    try {
      setState(await api.repoState());
    } catch {
      // Shown by whatever section fails next; the chrome just goes quiet.
    }
  }, [repo]);
  usePoll(refresh, 2000, [repo?.root], !!repo);

  // Rail counts: files changed, agents working.
  usePoll(
    async () => {
      const [files, tasks] = await Promise.all([api.status(), api.listTasks()]);
      setBadges({
        changes: new Set(files.map((f) => f.path)).size,
        working: tasks.filter((t) => t.run?.state === "running").length,
      });
    },
    4000,
    [repo?.root, state?.index],
    !!repo,
  );

  const undo = useCallback(() => {
    api
      .undo()
      .then(async (msg) => {
        toast(msg);
        await refresh();
      })
      .catch(fail);
  }, [toast, fail, refresh]);

  const shell: Shell | null = useMemo(
    () =>
      repo
        ? {
            repo,
            state,
            refresh,
            toast,
            fail,
            undo,
            act: async (run, opts) => {
              try {
                const msg = await run();
                if (msg) toast(msg, "ok", opts?.undo ?? true);
                return true;
              } catch (e) {
                fail(e);
                return false;
              } finally {
                await refresh();
              }
            },
            switchBranch: async (run) => {
              try {
                toast(await run(false), "ok", true);
                return true;
              } catch (e) {
                const text = errorText(e);
                if (!text.startsWith("BLOCKED: ")) {
                  fail(e);
                  return false;
                }
                toast(text.slice(9), "error", false, {
                  label: "Stash and switch",
                  run: () =>
                    run(true)
                      .then((msg) => toast(msg, "ok", true), fail)
                      .finally(refresh),
                });
                return false;
              } finally {
                await refresh();
              }
            },
            go: (t) => {
              setSection(t.section);
              setTarget((old) => ({ ...t, seq: old.seq + 1 }));
            },
            target,
            openRepo: (path) =>
              api
                .openRepo(path)
                .then((r) => {
                  setRepo(r);
                  setState(null);
                })
                .catch(fail),
          }
        : null,
    [repo, state, refresh, toast, fail, undo, target], // eslint-disable-line react-hooks/exhaustive-deps
  );

  // ⌘1–⌘5 switch sections; ⌘Z undoes the last change anywhere (not while
  // typing, where it undoes the typing).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const typing = (e.target as HTMLElement)?.closest?.("input, textarea, select");
      const s = SECTIONS.find((x) => x.key === e.key);
      if (s) {
        e.preventDefault();
        setSection(s.id);
      } else if (e.key === "z" && !e.shiftKey && !typing) {
        e.preventDefault();
        undo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (repo === undefined) return <div className="boot" />;
  if (repo === null || !shell)
    return <OpenRepo onOpen={(path) => api.openRepo(path).then(setRepo, fail)} toasts={toasts} />;

  return (
    <ShellContext.Provider value={shell}>
      <div className="app">
        <nav className="rail" aria-label="Sections">
          {SECTIONS.map((s) => {
            const count = s.id === "changes" ? badges.changes : s.id === "agents" ? badges.working : 0;
            return (
              <button
                key={s.id}
                className={`rail-item${section === s.id ? " active" : ""}`}
                onClick={() => setSection(s.id)}
                title={`${s.label} (⌘${s.key})`}
                aria-label={s.label}
                aria-current={section === s.id ? "page" : undefined}
              >
                {s.icon}
                {count > 0 && <span className={`rail-badge num${s.id === "agents" ? " live" : ""}`}>{count}</span>}
              </button>
            );
          })}
        </nav>
        <div className="section">
          {state?.operation && <OperationBanner state={state} />}
          <div className="section-body">
            {section === "agents" && <AgentsSection />}
            {section === "history" && <HistorySection />}
            {section === "changes" && <ChangesSection />}
            {section === "pulls" && <PullsSection />}
            {section === "actions" && <ActionsSection />}
          </div>
        </div>
        <Toasts toasts={toasts} onUndo={undo} onClose={(id) => setToasts((t) => t.filter((x) => x.id !== id))} />
      </div>
    </ShellContext.Provider>
  );
}

/** A merge, rebase, cherry-pick or revert that stopped: shown on every
 * section until it's finished or abandoned. */
function OperationBanner({ state }: { state: RepoState }) {
  const shell = useShell();
  const what = { merge: "Merge", rebase: "Rebase", cherrypick: "Cherry-pick", revert: "Revert" }[state.operation!];
  const n = state.conflicts.length;
  return (
    <div className={`op-banner${n > 0 ? " bad" : ""}`}>
      <Alert size={14} />
      <span>
        {n > 0 ? (
          <>
            {what} stopped on conflicts in {n} {n === 1 ? "file" : "files"}
          </>
        ) : (
          <>{what} is ready to continue</>
        )}
      </span>
      <span className="flex-1" />
      {n > 0 ? (
        <button className="ghost small" onClick={() => shell.go({ section: "changes", path: state.conflicts[0] })}>
          Resolve in Changes
        </button>
      ) : (
        <button className="primary small" onClick={() => shell.act(api.continueOperation, { undo: false })}>
          Continue {what.toLowerCase()}
        </button>
      )}
      <button className="ghost small" onClick={() => shell.act(api.abortOperation, { undo: false })}>
        Abort
      </button>
    </div>
  );
}

function Toasts({ toasts, onUndo, onClose }: { toasts: Toast[]; onUndo: () => void; onClose: (id: number) => void }) {
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.tone}`}>
          <span>{t.text}</span>
          {t.action && (
            <button
              className="link"
              onClick={() => {
                onClose(t.id);
                t.action!.run();
              }}
            >
              {t.action.label}
            </button>
          )}
          {t.undo && (
            <button
              className="link"
              onClick={() => {
                onClose(t.id);
                onUndo();
              }}
            >
              Undo <kbd>⌘Z</kbd>
            </button>
          )}
          <button className="icon-button tiny" onClick={() => onClose(t.id)} aria-label="Dismiss">
            <X size={11} />
          </button>
        </div>
      ))}
    </div>
  );
}

function OpenRepo({ onOpen, toasts }: { onOpen: (path: string) => void; toasts: Toast[] }) {
  return (
    <div className="open-repo">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          onOpen(new FormData(e.currentTarget).get("path") as string);
        }}
      >
        <h1>Open a repository</h1>
        <p className="lede">
          <Branch size={12} /> Spor shows its history, changes, pull requests, Actions and agent tasks.
        </p>
        <input name="path" autoFocus placeholder="~/code/my-project" />
        <button className="primary" type="submit">
          Open
        </button>
      </form>
      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
