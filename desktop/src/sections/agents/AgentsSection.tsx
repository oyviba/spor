import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import type { LineComment, TaskDetail, TaskSummary } from "../../types";
import { activityAt } from "./status";
import { usePoll, useStored } from "../../shared/hooks";
import { useShell } from "../../shared/app";
import { Sidebar } from "./Sidebar";
import { NewTask } from "./NewTask";
import { TaskPage, type Tab } from "./TaskPage";
import { choiceOf, type AgentSettings, type ComposerHandle } from "./Composer";

type View = { kind: "new" } | { kind: "task"; id: string };

/** Agent tasks: a list of tasks, and the selected one's conversation,
 * changes and landing. */
export function AgentsSection() {
  const shell = useShell();
  const { repo, fail, toast } = shell;
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [view, setView] = useStored<View>("agents.view", { kind: "new" });
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
  const composer = useRef<ComposerHandle>(null);

  const selectedId = view.kind === "task" ? view.id : null;

  // Another section sent us to a task.
  useEffect(() => {
    if (shell.target.section === "agents" && shell.target.taskId) setView({ kind: "task", id: shell.target.taskId });
  }, [shell.target.seq]); // eslint-disable-line react-hooks/exhaustive-deps

  const refreshTasks = useCallback(async () => {
    setTasks(await api.listTasks());
  }, []);
  usePoll(refreshTasks, 1500, [repo.root]);

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
  }, [selectedId]); // eslint-disable-line react-hooks/exhaustive-deps
  usePoll(refreshDetail, running ? 1000 : 3000, [selectedId], !!selectedId);
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

  // ⌘N: a new task; ⌘L: the composer; ⌘.: the details card.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      if (e.key === "n") {
        e.preventDefault();
        setView({ kind: "new" });
      } else if (e.key === "l") {
        e.preventDefault();
        composer.current?.focus();
      } else if (e.key === ".") {
        e.preventDefault();
        setInspector((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

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

  const askRebase = () => {
    if (!shown) return;
    const base = shown.task.base;
    send(
      `${base} has moved on and this branch now conflicts with it in ${shown.conflicts.join(", ")}. ` +
        `Rebase onto ${base} (git rebase ${base}), resolve the conflicts keeping both sides' intent, ` +
        `make sure it still builds and the tests pass, and commit.`,
    );
  };

  const taskComments = selectedId ? (comments[selectedId] ?? []) : [];

  return (
    <>
      <Sidebar
        tasks={tasks}
        selected={selectedId}
        seen={seen}
        shelves={shelves}
        onShelves={setShelves}
        onSelect={(id) => setView({ kind: "task", id })}
        onNew={() => setView({ kind: "new" })}
      />
      <main className="main">
        {view.kind === "new" || !selectedId ? (
          <NewTask repo={repo} settings={settings} onSettings={setSettings} onCreate={create} />
        ) : shown ? (
          <TaskPage
            ref={composer}
            repo={{ ...repo, branch: shell.state?.branch ?? repo.branch }}
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
            onLand={(merge, commitUncommitted) =>
              shell.act(() => api.landTask(shown.task.id, merge, commitUncommitted)).then(afterChange)
            }
            onDiscard={() =>
              shell.act(() => api.discardTask(shown.task.id)).then((ok) => {
                if (ok) setView({ kind: "new" });
                return refreshTasks();
              })
            }
            onAskRebase={askRebase}
            onOpenFolder={() => api.openFolder(shown.task.id).catch(fail)}
            onCopy={(text, what) => navigator.clipboard.writeText(text).then(() => toast(`${what} copied`), fail)}
            onShowBranch={() => shell.go({ section: "history", branch: shown.task.branch })}
          />
        ) : (
          <div className="boot" />
        )}
      </main>
    </>
  );
}
