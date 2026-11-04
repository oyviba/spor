import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText } from "./api";
import type { LineComment, RepoInfo, TaskDetail, TaskSummary } from "./types";
import { activityAt } from "./status";
import { usePoll, useStored } from "./hooks";
import { Sidebar } from "./components/Sidebar";
import { NewTask } from "./components/NewTask";
import { TaskPage, type Tab } from "./components/TaskPage";
import { choiceOf, type AgentSettings, type ComposerHandle } from "./components/Composer";
import { X } from "./components/Icons";

type View = { kind: "new" } | { kind: "task"; id: string };

interface Toast {
  id: number;
  text: string;
  tone: "ok" | "error";
  undo?: boolean;
}

export function App() {
  const [repo, setRepo] = useState<RepoInfo | null | undefined>(undefined);
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [view, setView] = useStored<View>("view", { kind: "new" });
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [patch, setPatch] = useState<{ key: string; text: string } | null>(null);
  const [tab, setTab] = useState<Tab>("conversation");
  const [settings, setSettings] = useStored<AgentSettings>("agent", {
    agent: "claude",
    template: "",
    allowCommands: false,
  });
  const [seen, setSeen] = useStored<Record<string, number>>("seen", {});
  const [shelves, setShelves] = useStored("shelves", { working: true, settled: false });
  const [inspector, setInspector] = useStored("inspector", true);
  const [comments, setComments] = useState<Record<string, LineComment[]>>({});
  const [queue, setQueue] = useStored<Record<string, string[]>>("queue", {});
  const [focus, setFocus] = useState<Pick<LineComment, "file" | "line" | "side"> | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const composer = useRef<ComposerHandle>(null);

  const selectedId = view.kind === "task" ? view.id : null;

  const toast = useCallback((text: string, tone: Toast["tone"] = "ok", undo = false) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-2), { id, text, tone, undo }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), undo ? 6000 : 4000);
  }, []);
  const fail = useCallback((e: unknown) => toast(errorText(e), "error"), [toast]);

  // ---- Loading ----

  useEffect(() => {
    api.currentRepo().then(setRepo, () => setRepo(null));
  }, []);

  const refreshTasks = useCallback(async () => {
    if (!repo) return;
    setTasks(await api.listTasks());
  }, [repo]);
  usePoll(refreshTasks, 1500, [repo?.root], !!repo);

  const selected = tasks.find((t) => t.id === selectedId);
  const running = selected?.run?.state === "running";
  const refreshDetail = useCallback(async () => {
    if (!selectedId) return;
    try {
      const d = await api.taskDetail(selectedId);
      setDetail((old) => (old?.task.id !== d.task.id || JSON.stringify(old) !== JSON.stringify(d) ? d : old));
      // The open task polls faster than the list; keep its row in step.
      setTasks((ts) => ts.map((t) => (t.id === d.task.id ? d.task : t)));
    } catch (e) {
      // The task was landed or discarded elsewhere.
      if (String(e).startsWith("No task")) setView({ kind: "new" });
    }
  }, [selectedId]);
  usePoll(refreshDetail, running ? 1000 : 3000, [selectedId], !!repo && !!selectedId);
  useEffect(() => {
    setDetail(null);
    setTab("conversation");
    setFocus(null);
  }, [selectedId]);

  // The diff, reloaded when the task's numbers change.
  const shown = detail && detail.task.id === selectedId ? detail : null;
  const patchKey = shown
    ? `${shown.task.id}:${shown.task.stats.additions}:${shown.task.stats.deletions}:${shown.task.stats.files}`
    : "";
  useEffect(() => {
    if (tab !== "changes" || !shown || patch?.key === patchKey) return;
    api.taskPatch(shown.task.id).then((text) => setPatch({ key: patchKey, text }), fail);
  }, [tab, patchKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Looking at a task marks what it did as seen.
  useEffect(() => {
    if (!shown) return;
    const at = activityAt(shown.task);
    if (seen[shown.task.id] !== at) setSeen((s) => ({ ...s, [shown.task.id]: at }));
  }, [shown?.task.id, shown && activityAt(shown.task)]); // eslint-disable-line react-hooks/exhaustive-deps

  // Queued follow-ups go out, one per run, when their task's agent is idle.
  const sending = useRef(new Set<string>());
  useEffect(() => {
    for (const [id, messages] of Object.entries(queue)) {
      const t = tasks.find((x) => x.id === id);
      if (!t || t.landed) {
        if (!t && tasks.length > 0) setQueue(({ [id]: _, ...rest }) => rest);
        continue;
      }
      if (messages.length === 0 || t.run?.state === "running" || sending.current.has(id)) continue;
      sending.current.add(id);
      const [first, ...rest] = messages;
      api
        .sendToAgent(id, first, choiceOf(settings), settings.allowCommands)
        .then(() => setQueue((q) => ({ ...q, [id]: rest })), fail)
        .then(refreshTasks)
        .finally(() => sending.current.delete(id));
    }
  }, [tasks]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---- Actions ----

  const afterChange = async () => {
    await refreshTasks();
    await refreshDetail();
  };

  const create = async (prompt: string, base: string, background: boolean) => {
    try {
      const t = await api.createTask(prompt, base, choiceOf(settings), settings.allowCommands);
      await refreshTasks();
      if (background) toast(`Started ${t.id}`);
      else setView({ kind: "task", id: t.id });
      return true;
    } catch (e) {
      fail(e);
      return false;
    }
  };

  const send = (text: string) => {
    if (!shown) return;
    const id = shown.task.id;
    setComments((c) => ({ ...c, [id]: [] }));
    if (shown.task.run?.state === "running") {
      setQueue((q) => ({ ...q, [id]: [...(q[id] ?? []), text] }));
      return;
    }
    api
      .sendToAgent(id, text, choiceOf(settings), settings.allowCommands)
      .then(() => setTab("conversation"))
      .then(afterChange, fail);
  };

  const undo = () =>
    api
      .undo()
      .then(async (msg) => {
        toast(msg);
        setRepo(await api.currentRepo());
        await refreshTasks();
      })
      .catch(fail);

  const land = (merge: boolean, commitUncommitted: boolean) => {
    if (!shown) return;
    api
      .landTask(shown.task.id, merge, commitUncommitted)
      .then(async (msg) => {
        toast(msg, "ok", true);
        setRepo(await api.currentRepo());
        await afterChange();
      })
      .catch(fail);
  };

  const discard = () => {
    if (!shown) return;
    api
      .discardTask(shown.task.id)
      .then(async (msg) => {
        toast(msg, "ok", true);
        setView({ kind: "new" });
        await refreshTasks();
      })
      .catch(fail);
  };

  const askRebase = () => {
    if (!shown) return;
    const base = shown.task.base;
    send(
      `${base} has moved on and this branch now conflicts with it in ${shown.conflicts.join(", ")}. ` +
        `Rebase onto ${base} (git rebase ${base}), resolve the conflicts keeping both sides' intent, ` +
        `make sure it still builds and the tests pass, and commit.`,
    );
  };

  const copy = (text: string, what: string) =>
    navigator.clipboard.writeText(text).then(() => toast(`${what} copied`), fail);

  // ---- Keys ----

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      const typing = (e.target as HTMLElement)?.closest?.("input, textarea, select");
      if (e.key === "n") {
        e.preventDefault();
        setView({ kind: "new" });
      } else if (e.key === "z" && !e.shiftKey && !typing) {
        e.preventDefault();
        undo();
      } else if (e.key === "l") {
        e.preventDefault();
        composer.current?.focus();
      } else if (e.key === ".") {
        e.preventDefault();
        setInspector((v) => !v);
      } else if (e.key === "1" || e.key === "2") {
        e.preventDefault();
        setTab(e.key === "1" ? "conversation" : "changes");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // ---- Render ----

  if (repo === undefined) return <div className="boot" />;
  if (repo === null) return <OpenRepo onOpen={(path) => api.openRepo(path).then(setRepo, fail)} toasts={toasts} />;

  const taskComments = selectedId ? (comments[selectedId] ?? []) : [];

  return (
    <div className="app">
      <Sidebar
        repo={repo}
        tasks={tasks}
        selected={selectedId}
        seen={seen}
        shelves={shelves}
        onShelves={setShelves}
        onSelect={(id) => setView({ kind: "task", id })}
        onNew={() => setView({ kind: "new" })}
        onOpenRepo={(path) =>
          api
            .openRepo(path)
            .then((r) => {
              setRepo(r);
              setTasks([]);
              setView({ kind: "new" });
            })
            .catch(fail)
        }
      />
      <main className="main">
        {view.kind === "new" || !selectedId ? (
          <NewTask repo={repo} settings={settings} onSettings={setSettings} onCreate={create} />
        ) : shown ? (
          <TaskPage
            ref={composer}
            repo={repo}
            detail={shown}
            seenAt={seen[shown.task.id]}
            tab={tab}
            onTab={setTab}
            patch={patch?.key === patchKey ? patch.text : null}
            inspector={inspector}
            onInspector={setInspector}
            settings={settings}
            onSettings={setSettings}
            comments={taskComments}
            onComment={(file, line, side, text) =>
              setComments((c) => ({
                ...c,
                [shown.task.id]: [...(c[shown.task.id] ?? []), { id: `${Date.now()}`, file, line, side, text }],
              }))
            }
            onRemoveComment={(cid) =>
              setComments((c) => ({ ...c, [shown.task.id]: (c[shown.task.id] ?? []).filter((x) => x.id !== cid) }))
            }
            focus={focus}
            onFocus={(c) => {
              setTab("changes");
              setFocus({ file: c.file, line: c.line, side: c.side });
            }}
            queued={queue[shown.task.id] ?? []}
            onUnqueue={(i) =>
              setQueue((q) => ({ ...q, [shown.task.id]: (q[shown.task.id] ?? []).filter((_, j) => j !== i) }))
            }
            onStart={() =>
              api.sendToAgent(shown.task.id, null, choiceOf(settings), settings.allowCommands).then(afterChange, fail)
            }
            onSend={send}
            onStop={() => api.stopAgent(shown.task.id).then(afterChange, fail)}
            onLand={land}
            onDiscard={discard}
            onAskRebase={askRebase}
            onOpenFolder={() => api.openFolder(shown.task.id).catch(fail)}
            onCopy={copy}
          />
        ) : (
          <div className="boot" />
        )}
      </main>
      <Toasts toasts={toasts} onUndo={undo} onClose={(id) => setToasts((t) => t.filter((x) => x.id !== id))} />
    </div>
  );
}

function Toasts({ toasts, onUndo, onClose }: { toasts: Toast[]; onUndo: () => void; onClose: (id: number) => void }) {
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.tone}`}>
          <span>{t.text}</span>
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
        <p className="lede">Spor runs agent tasks in their own branch and folder beside it.</p>
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
