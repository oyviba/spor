import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import type { LineComment, RunPatch, TaskDetail, TaskPull, TaskSummary } from "../../types";
import { activityAt, isOpen } from "./status";
import { catchUpPrompt } from "./prompts";
import { useChanged, usePoll, useRepoChanges, useStored } from "../../shared/hooks";
import { useShell } from "../../shared/app";
import { Sidebar } from "./Sidebar";
import { NewTask } from "./NewTask";
import { TaskPage, type Tab } from "./TaskPage";
import { choiceOf, useAgentSettings, type ComposerHandle } from "./Composer";

type View = { kind: "new" } | { kind: "task"; id: string };

/** Agent tasks: a list of tasks, and the selected one's conversation,
 * changes and landing. */
export function AgentsSection() {
  const shell = useShell();
  const { repo, fail, toast } = shell;
  // The shell's list, with the open task's row kept in step with its detail.
  const [tasks, setTasks] = useState<TaskSummary[]>(shell.tasks);
  useEffect(() => setTasks(shell.tasks), [shell.tasks]);
  const [view, setView] = useStored<View>("agents.view", { kind: "new" });
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [patch, setPatch] = useState<{ key: string; text: string } | null>(null);
  // The Changes tab: everything against the base, or one run's changes.
  const [source, setSource] = useState<"all" | number>("all");
  const [runPatch, setRunPatch] = useState<{ key: string; value: RunPatch } | null>(null);
  const [tab, setTab] = useState<Tab>("conversation");
  const [settings, setSettings] = useAgentSettings();
  const [seen, setSeen] = useStored<Record<string, number>>("seen", {});
  const [shelves, setShelves] = useStored("shelves", { working: true, settled: false });
  const [inspector, setInspector] = useStored("inspector", true);
  const [comments, setComments] = useState<Record<string, LineComment[]>>({});
  const [queue, setQueue] = useStored<Record<string, string[]>>("queue", {});
  const [focus, setFocus] = useState<Pick<LineComment, "file" | "line" | "side"> | null>(null);
  const composer = useRef<ComposerHandle>(null);

  const selectedId = view.kind === "task" ? view.id : null;

  // Another section sent us to a task.
  useChanged(() => {
    if (shell.target.section === "agents" && shell.target.taskId) setView({ kind: "task", id: shell.target.taskId });
  }, [shell.target.seq]);

  const refreshTasks = shell.reloadTasks;

  // Open tasks' pull requests (one `gh` call), and a note when one merges.
  const [pulls, setPulls] = useState<Record<string, TaskPull>>({});
  const merged = useRef<Set<string> | null>(null);
  const openTasks = tasks.filter(isOpen).length;
  const refreshPulls = useCallback(async () => {
    let list: TaskPull[];
    try {
      list = await api.taskPulls();
    } catch {
      return; // Not GitHub, or no `gh`: tasks just have no PRs.
    }
    setPulls(Object.fromEntries(list.map((p) => [p.task, p])));
    const now = new Set(list.filter((p) => p.pull.state === "merged" || p.pull.state === "closed").map((p) => p.task));
    if (merged.current)
      for (const p of list)
        if (now.has(p.task) && !merged.current.has(p.task))
          toast(
            `#${p.pull.number} ${p.pull.state === "merged" ? "merged" : "was closed"} — ${p.task} can be cleaned up`,
            "ok",
            false,
            p.pull.state === "merged"
              ? { label: "Clean up", run: () => settle(p.task, p.merge_commit ?? `#${p.pull.number}`) }
              : { label: "Close task", run: () => close(p.task) },
          );
    merged.current = now;
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  usePoll(refreshPulls, 30_000, [repo.root, openTasks], openTasks > 0);

  const refreshDetail = useCallback(async () => {
    if (!selectedId) return;
    try {
      const d = await api.taskDetail(selectedId);
      setDetail((old) => (old?.task.id !== d.task.id || JSON.stringify(old) !== JSON.stringify(d) ? d : old));
      // Keep its row in step with the detail.
      setTasks((ts) => ts.map((t) => (t.id === d.task.id ? d.task : t)));
    } catch (e) {
      // The task was landed or discarded elsewhere.
      if (String(e).startsWith("No task")) setView({ kind: "new" });
    }
  }, [selectedId]); // eslint-disable-line react-hooks/exhaustive-deps
  useRepoChanges(refreshDetail, ["tasks", "refs"], [selectedId], !!selectedId);
  useChanged(() => {
    setDetail(null);
    setTab("conversation");
    setFocus(null);
    setSource("all");
  }, [selectedId]);

  // The diff, reloaded when the task's numbers change.
  const shown = detail && detail.task.id === selectedId ? detail : null;
  const patchKey = shown
    ? `${shown.task.id}:${shown.task.stats.additions}:${shown.task.stats.deletions}:${shown.task.stats.files}`
    : "";
  useEffect(() => {
    if (tab !== "changes" || !shown || source !== "all" || patch?.key === patchKey) return;
    api.taskPatch(shown.task.id).then((text) => setPatch({ key: patchKey, text }), fail);
  }, [tab, patchKey, source]); // eslint-disable-line react-hooks/exhaustive-deps
  const runKey = shown && source !== "all" ? `${patchKey}:${source}:${shown.runs[source]?.state}:${shown.runs[source]?.undone}` : "";
  useEffect(() => {
    if (tab !== "changes" || !shown || source === "all" || runPatch?.key === runKey) return;
    api.runPatch(shown.task.id, source).then(
      (value) => setRunPatch({ key: runKey, value }),
      (e) => {
        setRunPatch({ key: runKey, value: { patch: "", from_base: [] } });
        fail(e);
      },
    );
  }, [tab, runKey]); // eslint-disable-line react-hooks/exhaustive-deps

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
      if (!t || !isOpen(t)) {
        if (!t && tasks.length > 0) setQueue(({ [id]: _, ...rest }) => rest);
        continue;
      }
      if (messages.length === 0 || t.run?.state === "running" || sending.current.has(id)) continue;
      sending.current.add(id);
      const [first, ...rest] = messages;
      api
        .sendToAgent(id, first, choiceOf(settings), settings.mode)
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
      const t = await api.createTask(prompt, base, choiceOf(settings), settings.mode);
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
    return api
      .sendToAgent(id, text, choiceOf(settings), settings.mode)
      .then(() => setTab("conversation"))
      .then(afterChange, fail);
  };

  const settle = (id: string, commit: string) =>
    shell.act(() => api.settleTask(id, commit), { pending: "Cleaning up…" }).then(async () => {
      await refreshTasks();
      await refreshPulls();
    });

  const close = (id: string) =>
    shell.act(() => api.closeTask(id), { pending: "Cleaning up…" }).then(async () => {
      await refreshTasks();
      await refreshPulls();
    });

  const github = (id: string) => ({
    pull: pulls[id],
    onShowIssue: (n: number) => shell.go({ section: "issues", number: n }),
    onShowPull: (n: number) => shell.go({ section: "pulls", number: n }),
    onPublish: (draft: boolean, commitUncommitted: boolean) =>
      shell
        .act(() => api.publishTask(id, draft, commitUncommitted), {
          undo: false,
          pending: draft ? "Pushing and opening a draft pull request…" : "Pushing and opening a pull request…",
        })
        .then(async () => {
          await afterChange();
          await refreshPulls();
        }),
    onPush: () => shell.act(() => api.pushTask(id), { undo: false, pending: "Pushing…" }).then(afterChange),
    onSendReview: () =>
      api.reviewFeedback(id).then(async (text) => {
        await send(text);
        toast(shown?.task.run?.state === "running" ? "The review is queued for when the agent is done" : `Sent the review to ${id}`);
      }, fail),
    onSendFailure: () =>
      api.checkFeedback(id).then(async (text) => {
        await send(text);
        toast(shown?.task.run?.state === "running" ? "The failure is queued for when the agent is done" : `Sent the failure to ${id}`);
      }, fail),
    onSettle: (commit: string) => settle(id, commit),
    onClose: () => close(id),
  });

  const askRebase = () => {
    if (!shown) return;
    send(catchUpPrompt(shown.task.base, shown.conflicts, shown.task.pull != null));
  };

  const taskComments = selectedId ? (comments[selectedId] ?? []) : [];

  return (
    <>
      <Sidebar
        tasks={tasks}
        pulls={pulls}
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
            patch={
              source === "all"
                ? patch?.key === patchKey
                  ? patch.text
                  : null
                : runPatch?.key === runKey
                  ? runPatch.value.patch
                  : null
            }
            source={source}
            onSource={setSource}
            fromBase={source !== "all" && runPatch?.key === runKey ? runPatch.value.from_base : []}
            onRunChanges={(index) => {
              setSource(index);
              setTab("changes");
            }}
            onUndoRun={() => shell.act(() => api.undoRun(shown.task.id), { pending: "Undoing the run…" }).then(afterChange)}
            onAnswer={(ask, allow, message) =>
              api.answerAsk(shown.task.id, ask.id, allow, message ?? "").then(afterChange, fail)
            }
            onAllowForSession={(ask) =>
              api.answerAsk(shown.task.id, ask.id, true, "", true).then(afterChange, fail)
            }
            onRetrySetup={() =>
              api.sendToAgent(shown.task.id, null, choiceOf(settings), settings.mode).then(afterChange, fail)
            }
            onSkipSetup={() =>
              api
                .skipSetup(shown.task.id)
                .then(() => api.sendToAgent(shown.task.id, null, choiceOf(settings), settings.mode))
                .then(afterChange, fail)
            }
            onSteer={(text) => {
              setComments((c) => ({ ...c, [shown.task.id]: [] }));
              return api.steer(shown.task.id, text).then(afterChange, fail);
            }}
            onLiveMode={(mode) =>
              api.setMode(shown.task.id, mode).then(() => toast(`The agent is now in ${mode === "acceptEdits" ? "auto-accept edits" : mode} mode`), fail)
            }
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
              api.sendToAgent(shown.task.id, null, choiceOf(settings), settings.mode).then(afterChange, fail)
            }
            onSend={send}
            onStop={() => api.stopAgent(shown.task.id).then(afterChange, fail)}
            onLand={(merge, commitUncommitted) =>
              shell.act(() => api.landTask(shown.task.id, merge, commitUncommitted), { pending: `Landing on ${shown.task.base}…` }).then(afterChange)
            }
            onDiscard={() =>
              shell.act(() => api.discardTask(shown.task.id), { pending: "Discarding…" }).then((ok) => {
                if (ok) setView({ kind: "new" });
                return refreshTasks();
              })
            }
            onAskRebase={askRebase}
            onOpenFolder={() => api.openFolder(shown.task.id).catch(fail)}
            onCopy={(text, what) => navigator.clipboard.writeText(text).then(() => toast(`${what} copied`), fail)}
            onShowBranch={() => shell.go({ section: "history", branch: shown.task.branch })}
            github={github(shown.task.id)}
          />
        ) : (
          <div className="boot" />
        )}
      </main>
    </>
  );
}
