import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api";
import type { LineComment, Pull, RunPatch, TaskDetail, TaskPull, TaskSummary, WorkBranch } from "../../types";
import { activityAt, matches, moveTo, shelve, workItems, type Filter, type WorkItem } from "./items";
import { useChanged, usePoll, useRepoChanges, useStored } from "../../shared/hooks";
import { useShell } from "../../shared/app";
import { EmptyState } from "../../shared/Layout";
import { Sidebar } from "./Sidebar";
import { BranchPage } from "./BranchPage";
import { FromIssue } from "./FromIssue";
import { PullPage } from "../pulls/PullsSection";
import { NewTask } from "../agents/NewTask";
import { TaskPage, type Tab } from "../agents/TaskPage";
import { choiceOf, useAgentSettings, type ComposerHandle } from "../agents/Composer";

/** What the page shows: something to start, or one work item. */
type View =
  | { kind: "new"; what?: "agent" | "issue" }
  | { kind: "task"; id: string }
  | { kind: "branch"; name: string }
  | { kind: "pull"; number: number };

const keyOf = (v: View): string | null =>
  v.kind === "task" ? `task:${v.id}` : v.kind === "branch" ? `branch:${v.name}` : v.kind === "pull" ? `pull:${v.number}` : null;

const viewOf = (i: WorkItem): View =>
  i.kind === "task" ? { kind: "task", id: i.task.id } : i.kind === "branch" ? { kind: "branch", name: i.branch.name } : { kind: "pull", number: i.pull.number };

/** Open pull requests, and which of them are yours or wait on your review. */
interface GitHub {
  pulls: Pull[];
  yours: Set<number>;
  review: Set<number>;
}
const NO_GITHUB: GitHub = { pulls: [], yours: new Set(), review: new Set() };

/** Work: every piece of work in the repository (agents' tasks, your
 * branches, everyone's pull requests) in one list, and the selected one's
 * page. */
export function WorkSection() {
  const shell = useShell();
  const { repo, fail, toast } = shell;
  // The shell's list, with the open task's row kept in step with its detail.
  const [tasks, setTasks] = useState<TaskSummary[]>(shell.tasks);
  useEffect(() => setTasks(shell.tasks), [shell.tasks]);
  const [view, setView] = useStored<View>("work.view", { kind: "new" });
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [patch, setPatch] = useState<{ key: string; text: string } | null>(null);
  // The Changes tab: everything against the base, or one run's changes.
  const [source, setSource] = useState<"all" | number>("all");
  const [runPatch, setRunPatch] = useState<{ key: string; value: RunPatch } | null>(null);
  const [tab, setTab] = useState<Tab>("conversation");
  const [settings, setSettings] = useAgentSettings();
  const [seen, setSeen] = useStored<Record<string, number>>("work.seen", {});
  const [settledOpen, setSettledOpen] = useStored("work.settled", false);
  const [filter, setFilter] = useStored<Filter>("work.filter", "all");
  const [order, setOrder] = useStored<Record<string, number>>("work.order", {});
  const [inspector, setInspector] = useStored("inspector", true);
  const [comments, setComments] = useState<Record<string, LineComment[]>>({});
  const [queue, setQueue] = useStored<Record<string, string[]>>("queue", {});
  const [focus, setFocus] = useState<Pick<LineComment, "file" | "line" | "side"> | null>(null);
  const composer = useRef<ComposerHandle>(null);

  const selectedId = view.kind === "task" ? view.id : null;

  // Another section sent us to a task, a pull request or a branch.
  useChanged(() => {
    const t = shell.target;
    if (t.section !== "work") return;
    if (t.taskId) setView({ kind: "task", id: t.taskId });
    else if (t.number) setView({ kind: "pull", number: t.number });
    else if (t.branch) setView({ kind: "branch", name: t.branch });
  }, [shell.target.seq]);

  // Your branches, reloaded when refs move.
  const [branches, setBranches] = useState<WorkBranch[]>([]);
  useRepoChanges(
    async () => {
      const list = await api.workBranches().catch(() => [] as WorkBranch[]);
      setBranches((old) => (JSON.stringify(old) === JSON.stringify(list) ? old : list));
    },
    ["refs"],
    [repo.root],
  );
  // Open pull requests from anyone (three `gh` calls a minute). Not GitHub,
  // or no `gh`: there are just none.
  const [hub, setHub] = useState<GitHub>(NO_GITHUB);
  const refreshGithub = useCallback(async () => {
    try {
      const [pulls, yours, review] = await Promise.all([api.pulls("open"), api.pulls("yours"), api.pulls("review")]);
      setHub({ pulls, yours: new Set(yours.map((p) => p.number)), review: new Set(review.map((p) => p.number)) });
    } catch {
      setHub(NO_GITHUB);
    }
  }, []);
  usePoll(refreshGithub, 60_000, [repo.root]);

  const refreshTasks = shell.reloadTasks;

  // Open tasks' pull requests (one `gh` call), and a note when one merges.
  const [pulls, setPulls] = useState<Record<string, TaskPull>>({});
  const merged = useRef<Set<string> | null>(null);
  const openTasks = tasks.filter((t) => !t.landed).length;
  const refreshPulls = useCallback(async () => {
    let list: TaskPull[];
    try {
      list = await api.taskPulls();
    } catch {
      return; // Not GitHub, or no `gh`: tasks just have no PRs.
    }
    setPulls(Object.fromEntries(list.map((p) => [p.task, p])));
    const now = new Set(list.filter((p) => p.pull.state === "merged").map((p) => p.task));
    if (merged.current)
      for (const p of list)
        if (now.has(p.task) && !merged.current.has(p.task))
          toast(`#${p.pull.number} merged — ${p.task} can be cleaned up`, "ok", false, {
            label: "Clean up",
            run: () => settle(p.task, p.merge_commit ?? `#${p.pull.number}`),
          });
    merged.current = now;
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  usePoll(refreshPulls, 30_000, [repo.root, openTasks], openTasks > 0);

  const items = useMemo(
    () => workItems({ tasks, taskPulls: pulls, branches, pulls: hub.pulls, yours: hub.yours, review: hub.review, seen }),
    [tasks, pulls, branches, hub, seen],
  );
  const shelves = useMemo(() => shelve(items.filter((i) => matches(i, filter)), order), [items, filter, order]);
  const selectedKey = keyOf(view);
  const selectedItem = items.find((i) => i.key === selectedKey);

  // An item gets its place in the list when first seen there, so it keeps it.
  useEffect(() => {
    const fresh = items.filter((i) => order[i.key] == null);
    if (fresh.length === 0) return;
    const known = new Set(items.map((i) => i.key));
    setOrder((o) => ({
      // Forget items that are gone.
      ...Object.fromEntries(Object.entries(o).filter(([k]) => known.has(k))),
      ...Object.fromEntries(fresh.map((i) => [i.key, i.since])),
    }));
  }, [items]); // eslint-disable-line react-hooks/exhaustive-deps

  // Opening a branch or pull request marks what it said as seen.
  useEffect(() => {
    if (!selectedItem || selectedItem.kind === "task") return;
    if (seen[selectedItem.key] !== selectedItem.updated) setSeen((s) => ({ ...s, [selectedItem.key]: selectedItem.updated }));
  }, [selectedItem?.key, selectedItem?.updated]); // eslint-disable-line react-hooks/exhaustive-deps

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
    const key = `task:${shown.task.id}`;
    if (seen[key] !== at) setSeen((s) => ({ ...s, [key]: at }));
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
        .sendToAgent(id, first, choiceOf(settings), settings.mode)
        .then(() => setQueue((q) => ({ ...q, [id]: rest })), fail)
        .then(refreshTasks)
        .finally(() => sending.current.delete(id));
    }
  }, [tasks]); // eslint-disable-line react-hooks/exhaustive-deps

  // New…: a branch for you, a task for an agent, or an issue to work on.
  // All three become a work item in the list.
  const newMenu = (at: { clientX: number; clientY: number }) =>
    shell.menu(at, [
      { label: "Branch…", run: newBranch },
      { label: "Agent task", run: () => setView({ kind: "new", what: "agent" }) },
      { label: "From issue…", run: () => setView({ kind: "new", what: "issue" }) },
    ]);
  const newBranch = async () => {
    const base = repo.default_base ?? repo.branches[0] ?? "main";
    const answer = await shell.prompt({
      title: "New branch",
      detail: `From ${base}, checked out here.`,
      placeholder: "feat/…",
      action: "Create branch",
    });
    const name = answer?.value.trim();
    if (!name) return;
    if (await shell.switchBranch((stash) => api.createBranch(name, base, stash))) {
      setView({ kind: "branch", name });
      setBranches(await api.workBranches().catch(() => branches));
    }
  };

  // ⌘N: New…; ⌘L: the composer; ⌘.: the details card.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      if (e.key === "n") {
        e.preventDefault();
        const r = document.querySelector(".new-work")?.getBoundingClientRect();
        newMenu({ clientX: r?.left ?? 80, clientY: (r?.bottom ?? 40) + 4 });
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
          await refreshGithub();
        }),
    onPush: () => shell.act(() => api.pushTask(id), { undo: false, pending: "Pushing…" }).then(afterChange),
    onSendReview: () =>
      api.reviewFeedback(id).then(async (text) => {
        await send(text);
        toast(shown?.task.run?.state === "running" ? "The review is queued for when the agent is done" : `Sent the review to ${id}`);
      }, fail),
    onSettle: (commit: string) => settle(id, commit),
  });

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
        shelves={shelves}
        any={items.length > 0}
        filter={filter}
        onFilter={setFilter}
        selected={selectedKey}
        seen={seen}
        settledOpen={settledOpen}
        onSettledOpen={setSettledOpen}
        onSelect={(i) => setView(viewOf(i))}
        onMove={(i, top) => setOrder((o) => moveTo(items, o, i.key, top))}
        onNew={newMenu}
      />
      <main className="main">
        {view.kind === "new" ? (
          view.what === "issue" ? (
            <FromIssue
              onStarted={async (t) => {
                await refreshTasks();
                setView({ kind: "task", id: t.id });
              }}
              onOpenTask={(id) => setView({ kind: "task", id })}
            />
          ) : (
            <NewTask repo={repo} settings={settings} onSettings={setSettings} onCreate={create} />
          )
        ) : view.kind === "pull" ? (
          <PullPage number={view.number} />
        ) : view.kind === "branch" ? (
          selectedItem?.kind === "branch" ? (
            selectedItem.pull ? (
              <PullPage number={selectedItem.pull.number} />
            ) : (
              <BranchPage branch={selectedItem.branch} />
            )
          ) : (
            <EmptyState title={`Nothing on ${view.name}`}>
              It has no commits of its own any more, or it's gone. Pick something from the list, or start something new with ⌘N.
            </EmptyState>
          )
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
