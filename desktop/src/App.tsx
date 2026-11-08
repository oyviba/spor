import { Activity, Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, errorText } from "./api";
import type { BranchInfo, Credential, RepoInfo, RepoState, TaskSummary, WorkItemInfo } from "./types";
import { KeepScroll, usePoll, useRepoChanges, useStored } from "./shared/hooks";
import { ShellContext, useShell, type SectionId, type Shell, type Target, type ToastAction } from "./shared/app";
import { Alert, Bot, Branch, FileIcon, Gear, History as HistoryIcon, InboxIcon, IssueIcon, Play, PullRequest, Sparkle, X } from "./shared/Icons";
import { Button } from "./shared/Button";
import { RepoPanel } from "./shared/Layout";
import { BranchBar } from "./shared/BranchBar";
import { DiffSettings, SettingsDialog, type SettingsPage } from "./shared/SettingsDialog";
import { AgentLimit } from "./sections/agents/AgentLimit";
import { GraphOptionsPanel } from "./sections/history/GraphOptions";
import { ShortcutsPage } from "./shared/Shortcuts";
import { AppearancePage } from "./shared/ThemeEditor";
import { useApplyTheme } from "./shared/theme";
import { display, effective, isMac, keepChanges, lookup, shortcutOf } from "./shared/keys";
import { getSettings, setSettings, useSettings } from "./shared/settings";
import { resolveTarget } from "./shared/targets";
import {
  WORKSPACE_COLORS,
  activeWorkspace,
  create as createWorkspace,
  firstRun,
  fromList,
  getWorkspaces,
  initials,
  landing,
  moveRepo,
  noteOpened,
  recolor,
  remove as removeWorkspace,
  removeRepo,
  rename as renameWorkspace,
  replaceAll,
  switchTo,
  useWorkspaces,
  type Workspace,
} from "./shared/workspaces";
import { CredentialDialog, DialogView, MenuView, Palette, type Command, type DialogSpec, type MenuItem, type MenuSpec } from "./shared/Overlay";
import { WorkSection } from "./sections/work/WorkSection";
import { HistorySection } from "./sections/history/HistorySection";
import { ChangesSection } from "./sections/changes/ChangesSection";
import { IssuesSection } from "./sections/issues/IssuesSection";
import { PullsSection } from "./sections/pulls/PullsSection";
import { ActionsSection } from "./sections/actions/ActionsSection";
import { InboxSection, useInbox } from "./sections/inbox/InboxSection";
import { AgentsSection } from "./sections/agents/AgentsSection";
import { askLine, type InboxItem } from "./sections/inbox/inbox";

/** When you last looked at each task, by task id, as Work keeps it
 * (`work.seen`, keyed `task:<id>`): whether a finished run is unseen. */
function seenTasks(): Record<string, number> {
  try {
    const seen = JSON.parse(localStorage.getItem("spor.work.seen") ?? "{}") as Record<string, number>;
    return Object.fromEntries(
      Object.entries(seen)
        .filter(([k]) => k.startsWith("task:"))
        .map(([k, at]) => [k.slice(5), at]),
    );
  } catch {
    return {};
  }
}

interface Toast {
  id: number;
  text: string;
  /** progress: something still going; it stays until that's done. */
  tone: "ok" | "error" | "progress";
  undo?: boolean;
  action?: ToastAction;
}

/**
 * The sections (#83), in the order of the questions you come with: what needs
 * me, what's in flight, my checkout, the graph. Then, after a divider, the
 * browsers: GitHub's lists, for what belongs to no work item yet.
 *
 * Left to right is workspace → repository → section: the Inbox and Agents
 * span every repository (`global`), so they sit on the rail with the
 * workspaces; the rest belong to one repository and are listed under the
 * open one in the repository column.
 */
const SECTIONS: { id: SectionId; label: string; icon: React.ReactNode; key: string; browser?: boolean; global?: boolean }[] = [
  { id: "inbox", label: "Inbox", icon: <InboxIcon size={17} />, key: "Mod+1", global: true },
  { id: "agents", label: "Agents", icon: <Sparkle size={17} />, key: "Mod+Shift+A", global: true },
  { id: "work", label: "Work", icon: <Bot size={17} />, key: "Mod+2" },
  { id: "changes", label: "Changes", icon: <FileIcon size={17} />, key: "Mod+3" },
  { id: "history", label: "History", icon: <HistoryIcon size={17} />, key: "Mod+4" },
  { id: "issues", label: "Issues", icon: <IssueIcon size={15} />, key: "Mod+5", browser: true },
  { id: "pulls", label: "Pull requests", icon: <PullRequest size={15} />, key: "Mod+6", browser: true },
  { id: "actions", label: "Actions", icon: <Play size={15} />, key: "Mod+7", browser: true },
];

/** Is the task open on screen (Work, its page)? Then it needs no toast. */
function viewing(taskId: string): boolean {
  try {
    const section = JSON.parse(localStorage.getItem("spor.section") ?? "null");
    const view = JSON.parse(localStorage.getItem("spor.work.view") ?? "null");
    const agents = JSON.parse(localStorage.getItem("spor.agents.open") ?? "null");
    const open = (section === "work" && view?.kind === "task" && view.id === taskId) || (section === "agents" && agents?.taskId === taskId);
    return open && document.hasFocus();
  } catch {
    return false;
  }
}

/** A command the palette lists and a shortcut can run. */
interface AppCommand {
  id: string;
  label: string;
  group: string;
  hint?: string;
  /** Its default shortcut ("Mod+K"). */
  keys?: string;
  /** Also runs when the focus is in a text field. */
  whileTyping?: boolean;
  run: () => void;
}

/** What can be done to the open repository, for the palette and shortcuts. */
function repoCommands(shell: Shell): AppCommand[] {
  const sync = (what: "fetch" | "pull" | "push") => () =>
    shell.act(() => api.sync(what), {
      undo: what === "pull",
      pending: { fetch: "Fetching…", pull: "Pulling…", push: shell.state?.upstream ? "Pushing…" : "Publishing…" }[what],
    });
  return [
    { id: "fetch", label: "Fetch", hint: "all remotes", group: "Git", run: sync("fetch") },
    { id: "pull", label: "Pull", hint: "with rebase", group: "Git", run: sync("pull") },
    { id: "push", label: shell.state?.upstream ? "Push" : "Publish branch", group: "Git", run: sync("push") },
    { id: "stash", label: "Stash changes", group: "Git", run: () => shell.act(() => api.stash(null), { undo: false, pending: "Stashing…" }) },
    {
      id: "pr",
      label: "Open pull request for this branch",
      group: "Git",
      run: () => api.pullRequestUrl().then(api.openUrl).catch(shell.fail),
    },
  ];
}

/** The rail's layout, so each move of it (#83, then the sections under the
 * open repository) is announced once. */
const RAIL_KEY = "spor.rail";
const RAIL = "3";

/** How often the open repository is fetched in the background. */
const AUTO_FETCH_MS = 5 * 60 * 1000;

export function App() {
  const [repo, setRepoRaw] = useState<RepoInfo | null | undefined>(undefined);
  const workspaces = useWorkspaces();
  const ws = activeWorkspace(workspaces);
  const recent = ws.repos;
  const [menu, setMenu] = useState<MenuSpec | null>(null);
  const [dialog, setDialog] = useState<DialogSpec | null>(null);
  const [credential, setCredential] = useState<Credential | null>(null);
  const [palette, setPalette] = useState<{ branches: BranchInfo[] } | null>(null);
  /** The Settings page open, if Settings is. */
  const [settingsPage, setSettingsPage] = useState<string | null>(null);
  const { keymap } = useSettings();
  const mac = isMac();
  useApplyTheme();
  const setRepo = useCallback(
    (r: RepoInfo | null) => {
      setRepoRaw(r);
      if (r) noteOpened(r.root);
    },
    [], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const [state, setState] = useState<RepoState | null>(null);
  /** The open repository, for callbacks that outlive a render. */
  const here = useRef<string | undefined>(undefined);
  here.current = repo?.root;
  const [section, setSection] = useStored<SectionId>("section", "inbox");
  // The Inbox and Agents span every repository: open, they show no
  // repository (no column, branch bar or repository header), and a
  // workspace's badge goes back to the section last open in its repository.
  const global = !!SECTIONS.find((s) => s.id === section)?.global;
  const [repoSection, setRepoSection] = useStored<SectionId>("section.repo", "work");
  useEffect(() => {
    if (!global) setRepoSection(section);
  }, [section]); // eslint-disable-line react-hooks/exhaustive-deps
  const [reposShown, setReposShown] = useStored("repos.shown", true);
  const [target, setTarget] = useState<Target & { seq: number }>({ section: "work", seq: 0 });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [changed, setChanged] = useState(0);
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  // Tasks known to be waiting, so a new question toasts once.
  const asking = useRef<Set<string> | null>(null);

  const toast = useCallback((text: string, tone: Toast["tone"] = "ok", undo = false, action?: ToastAction) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-2), { id, text, tone, undo, action }]);
    setTimeout(
      () => setToasts((t) => t.filter((x) => x.id !== id)),
      action ? 10000 : undo ? 6000 : tone === "error" ? 7000 : 4000,
    );
  }, []);
  const fail = useCallback((e: unknown) => toast(errorText(e), "error"), [toast]);
  // Something slow under way: say so if it takes more than a moment, until
  // it's done (the returned function). Quick ones never show.
  const progress = useCallback((text: string) => {
    const id = Date.now() + Math.random();
    let shown = false;
    const timer = setTimeout(() => {
      shown = true;
      setToasts((t) => [...t.slice(-2), { id, text, tone: "progress" }]);
    }, 400);
    return () => {
      clearTimeout(timer);
      if (shown) setToasts((t) => t.filter((x) => x.id !== id));
    };
  }, []);
  /** Run `work` with a progress toast, then toast its message (or the error). */
  const track = useCallback(
    async <T,>(text: string, work: () => Promise<T>): Promise<T> => {
      const done = progress(text);
      try {
        return await work();
      } finally {
        done();
      }
    },
    [progress],
  );

  useEffect(() => {
    api.currentRepo().then(setRepo, () => setRepo(null));
    // The first run takes the old native app's workspaces, so they carry over.
    if (firstRun)
      api
        .legacyWorkspaces()
        .then((legacy) => {
          if (!legacy) return;
          const open = getWorkspaces().list.flatMap((w) => w.repos);
          const next = fromList(legacy.list, legacy.active);
          // Keep a repository opened before the import finished.
          for (const path of open) if (!next.list.some((w) => w.repos.includes(path))) next.list[0].repos.push(path);
          replaceAll(next);
          const current = open[0];
          if (current) noteOpened(current);
        })
        .catch(() => undefined);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // git or ssh waiting on a passphrase or password (fetch, pull, push).
  usePoll(
    async () => {
      const ask = await api.askpassPending();
      setCredential((old) => (old?.id === ask?.id ? old : ask));
    },
    700,
    [],
    !!repo,
  );

  // Fetch quietly every few minutes so "behind" is true without asking.
  useEffect(() => {
    if (!repo) return;
    const run = () => api.autoFetch().catch(() => undefined);
    const first = setTimeout(run, 15_000);
    const every = setInterval(run, AUTO_FETCH_MS);
    return () => {
      clearTimeout(first);
      clearInterval(every);
    };
  }, [repo?.root]); // eslint-disable-line react-hooks/exhaustive-deps

  const lastState = useRef<RepoState | null>(null);
  const refresh = useCallback(async () => {
    if (!repo) return;
    try {
      const next = await api.repoState();
      noticeNewCommits(lastState.current, next);
      lastState.current = next;
      setState(next);
    } catch {
      // Shown by whatever section fails next; the chrome just goes quiet.
    }
  }, [repo]); // eslint-disable-line react-hooks/exhaustive-deps

  /** The upstream gained commits (a fetch, background or not): say so, with Pull. */
  const noticeNewCommits = (old: RepoState | null, now: RepoState) => {
    if (!old || !now.branch || old.branch !== now.branch || now.behind <= old.behind) return;
    const n = now.behind;
    const commits = `${n} new ${n === 1 ? "commit" : "commits"}`;
    const upstream = now.upstream ?? "The remote";
    if (now.ahead > 0)
      toast(`${upstream} has ${commits} — ${now.branch} has diverged`, "ok", false, {
        label: "Pull with rebase",
        run: () => doPull(),
      });
    else toast(`${upstream} has ${commits} to pull`, "ok", false, { label: "Pull", run: () => doPull() });
  };
  const doPull = () =>
    track("Pulling…", () => api.sync("pull"))
      .then((m) => toast(m, "ok", true), (e) => toast(errorText(e), "error"))
      .finally(() => refresh());
  useRepoChanges(refresh, ["refs", "index"], [repo?.root], !!repo);

  // How many files changed, for the repository column.
  useRepoChanges(
    async () => {
      const files = await api.status();
      setChanged(new Set(files.map((f) => f.path)).size);
    },
    ["index", "worktree"],
    [repo?.root],
    !!repo,
  );

  // Agent tasks, loaded here once for the rail, Work and Issues: a task's
  // commits are refs, the rest is in `.git/spor/tasks`.
  const reloadTasks = useCallback(async () => {
    const list = await api.listTasks();
    setTasks(list);
    // Starts held back by the limit on agents at once go when a slot is free.
    if (list.some((t) => t.waiting != null)) api.startWaiting().catch(() => {});
    const waiting = list.filter((t) => t.asks > 0);
    if (asking.current) {
      for (const t of waiting) {
        if (asking.current.has(t.id) || viewing(t.id)) continue;
        // With the window in the background nobody sees a toast: ask the
        // operating system too (#41).
        if (!document.hasFocus()) api.notify(`${t.title} needs you`, askLine(t.ask, t.asks)).catch(() => {});
        toast(`${t.title} is waiting for you`, "ok", false, {
          label: "Answer",
          run: () => {
            // From Agents, answer there, not in the repository's Work.
            if (localStorage.getItem("spor.section") === '"agents"' && here.current) return setAgentOpen({ repo: here.current, taskId: t.id });
            setSection("work");
            setTarget((old) => ({ section: "work", taskId: t.id, seq: old.seq + 1 }));
          },
        });
      }
    }
    asking.current = new Set(waiting.map((t) => t.id));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useRepoChanges(reloadTasks, ["tasks", "refs"], [repo?.root], !!repo);

  // Every work item, for the work card in each inspector. GitHub's part is
  // cached in the backend, so this is cheap on every change.
  const [work, setWork] = useState<WorkItemInfo[]>([]);
  const reloadWork = useCallback(async () => {
    const list = await api.workItems(seenTasks()).catch(() => null);
    if (list) setWork((old) => (JSON.stringify(old) === JSON.stringify(list) ? old : list));
  }, []);
  useRepoChanges(reloadWork, ["work"], [repo?.root], !!repo);
  useEffect(() => setWork([]), [repo?.root]);

  // What needs you in every repository of every workspace: the Inbox, and its badge.
  const everyRepo = [...new Set(workspaces.list.flatMap((w) => w.repos))];
  const inbox = useInbox(everyRepo, !!repo);

  const undo = useCallback(() => {
    track("Undoing…", api.undo)
      .then(async (msg) => {
        toast(msg);
        await refresh();
      })
      .catch(fail);
  }, [toast, fail, refresh, track]);

  /** Resolves to whether it opened. */
  const openRepo = (path: string) =>
    track(`Opening ${path.split("/").pop()}…`, () => api.openRepo(path)).then(
      (r) => {
        lastState.current = null;
        setRepo(r);
        setState(null);
        return true;
      },
      (e) => {
        fail(e);
        return false;
      },
    );

  /** An Inbox row: its workspace and repository (switching if need be),
   * then its section. */
  const openItem = async (item: Pick<InboxItem, "repo" | "target">) => {
    if (item.repo !== repo?.root && !(await openRepo(item.repo))) return;
    const owner = workspaces.list.find((w) => w.repos.includes(item.repo));
    if (owner && owner.id !== workspaces.active) switchTo(owner.id);
    // Its work item if it has one, from the tasks of the repository it's in.
    const to =
      item.repo === repo?.root
        ? resolveTarget(item.target, tasks)
        : await api.listTasks().then(
            (t) => resolveTarget(item.target, t),
            () => item.target,
          );
    setSection(to.section);
    setTarget((old) => ({ ...to, seq: old.seq + 1 }));
  };

  // The task open in Agents. Its repository is opened for it, behind the
  // view, so answering needs no trip to the repository.
  const [agentOpen, setAgentOpen] = useStored<{ repo: string; taskId: string } | null>("agents.open", null);
  useEffect(() => {
    if (section !== "agents" || !agentOpen || !repo || repo.root === agentOpen.repo) return;
    openRepo(agentOpen.repo).then((ok) => {
      if (!ok) return setAgentOpen(null);
      const owner = workspaces.list.find((w) => w.repos.includes(agentOpen.repo));
      if (owner && owner.id !== workspaces.active) switchTo(owner.id);
    });
  }, [section, agentOpen?.repo, repo?.root]); // eslint-disable-line react-hooks/exhaustive-deps

  const shell: Shell | null = useMemo(
    () =>
      repo
        ? {
            repo,
            state,
            refresh,
            tasks,
            reloadTasks,
            work,
            reloadWork,
            toast,
            fail,
            undo,
            act: async (run, opts) => {
              try {
                const msg = await track(opts?.pending ?? "Working…", run);
                if (msg) toast(msg, "ok", opts?.undo ?? true);
                return true;
              } catch (e) {
                fail(e);
                return false;
              } finally {
                await refresh();
                void reloadWork();
              }
            },
            switchBranch: async (run) => {
              try {
                toast(await track("Switching…", () => run(false)), "ok", true);
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
                    track("Stashing and switching…", () => run(true))
                      .then((msg) => toast(msg, "ok", true), fail)
                      .finally(refresh),
                });
                return false;
              } finally {
                await refresh();
              }
            },
            go: (t, opts) => {
              const to = opts?.browse ? t : resolveTarget(t, tasks);
              setSection(to.section);
              setTarget((old) => ({ ...to, seq: old.seq + 1 }));
            },
            target,
            openRepo,
            recent,
            forgetRepo: removeRepo,
            workspaceName: ws.name,
            otherWorkspaces: workspaces.list.filter((w) => w.id !== ws.id).map((w) => ({ id: w.id, name: w.name })),
            moveRepo,
            menu: (e, items) => {
              e.preventDefault?.();
              setMenu({ x: e.clientX, y: e.clientY, items });
            },
            prompt: (spec) => new Promise((resolve) => setDialog({ kind: "prompt", ...spec, resolve })),
            confirm: (spec) => new Promise((resolve) => setDialog({ kind: "confirm", ...spec, resolve })),
          }
        : null,
    [repo, state, refresh, tasks, work, toast, fail, undo, track, target, workspaces], // eslint-disable-line react-hooks/exhaustive-deps
  );

  /** Switch workspace, opening the repository last open there. */
  const goToWorkspace = (id: string) => {
    const target = workspaces.list.find((w) => w.id === id);
    if (!target) return;
    switchTo(id);
    if (global) setSection(repoSection);
    const path = landing(target);
    if (path && path !== repo?.root) openRepo(path);
    else if (!path) {
      // An empty workspace: the page to open one.
      setRepoRaw(null);
      setState(null);
    }
  };
  const workspaceMenu = (w: Workspace): MenuItem[] => [
    {
      label: "Rename…",
      run: async () => {
        const r = await new Promise<{ value: string } | null>((resolve) =>
          setDialog({ kind: "prompt", title: `Rename ${w.name}`, initial: w.name, action: "Rename", resolve }),
        );
        if (r) renameWorkspace(w.id, r.value);
      },
    },
    ...WORKSPACE_COLORS.map((c, i) => ({
      label: i === w.color ? `Color ${i + 1} ✓` : `Color ${i + 1}`,
      swatch: c,
      run: () => recolor(w.id, i),
      separator: i === 0,
    })),
    {
      label: `Delete ${w.name}`,
      danger: true,
      separator: true,
      disabled: workspaces.list.length === 1,
      run: async () => {
        const ok = await new Promise<boolean>((resolve) =>
          setDialog({
            kind: "confirm",
            title: `Delete the workspace ${w.name}?`,
            detail: w.repos.length
              ? `Its ${w.repos.length} ${w.repos.length === 1 ? "repository moves" : "repositories move"} to ${workspaces.list.find((x) => x.id !== w.id)!.name}. Nothing on disk changes.`
              : "Nothing on disk changes.",
            action: "Delete workspace",
            danger: true,
            resolve,
          }),
        );
        if (ok) removeWorkspace(w.id);
      },
    },
  ];
  const newWorkspace = async () => {
    const r = await new Promise<{ value: string } | null>((resolve) =>
      setDialog({ kind: "prompt", title: "New workspace", placeholder: "Work, Side projects…", action: "Create", resolve }),
    );
    if (r) {
      createWorkspace(r.value);
      setRepoRaw(null);
      setState(null);
    }
  };

  const openPalette = () => {
    if (!repo) return;
    api.branches().then(
      (branches) => setPalette({ branches }),
      () => setPalette({ branches: [] }),
    );
  };

  // Every command with a shortcut, and the shortcuts the user changed
  // (Settings → Shortcuts, ⌘/). The palette and the key handler read this.
  const appCommands: AppCommand[] = [
    ...SECTIONS.map((x) => ({
      id: `go:${x.id}`,
      label: `Go to ${x.label}`,
      group: "Sections",
      keys: x.key,
      whileTyping: true,
      run: () => setSection(x.id),
    })),
    { id: "palette", label: "Command palette", group: "App", keys: "Mod+K", whileTyping: true, run: () => openPalette() },
    { id: "settings", label: "Settings", group: "App", keys: "Mod+,", whileTyping: true, run: () => setSettingsPage("appearance") },
    { id: "shortcuts", label: "Keyboard shortcuts", group: "App", keys: "Mod+/", whileTyping: true, run: () => setSettingsPage("shortcuts") },
    {
      id: "repos",
      label: reposShown ? "Hide repositories" : "Show repositories",
      group: "App",
      keys: "Mod+Shift+E",
      whileTyping: true,
      run: () => setReposShown((v) => !v),
    },
    // Not while typing: there ⌘Z undoes the typing.
    { id: "undo", label: state?.undo ? `Undo: ${state.undo}` : "Undo", group: "App", keys: "Mod+Z", run: () => undo() },
    {
      id: "diff-layout",
      label: getSettings().diffLayout === "split" ? "Show diffs unified" : "Show diffs side by side",
      group: "App",
      whileTyping: true,
      run: () => setSettings({ diffLayout: getSettings().diffLayout === "split" ? "unified" : "split" }),
    },
    ...workspaces.list.slice(0, 9).map((w, i) => ({
      id: `workspace:${i + 1}`,
      label: `Workspace ${i + 1}: ${w.name}`,
      group: "Workspaces",
      keys: `Mod+Alt+${i + 1}`,
      whileTyping: true,
      run: () => goToWorkspace(w.id),
    })),
    { id: "workspace:new", label: "New workspace…", group: "Workspaces", whileTyping: true, run: () => newWorkspace() },
    ...(["system", "light", "dark"] as const).map((a) => ({
      id: `appearance:${a}`,
      label: a === "system" ? "Appearance: follow the system" : `Appearance: ${a}`,
      group: "App",
      whileTyping: true,
      run: () => setSettings((s) => ({ ...s, theme: { ...s.theme, appearance: a } })),
    })),
    ...(shell ? repoCommands(shell) : []),
  ];
  const shortcuts = effective(appCommands, keymap);
  const keyLabel = (id: string) => {
    const k = shortcuts.get(id);
    return k ? display(k, mac) : undefined;
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const sc = shortcutOf(e, mac);
      if (!sc) return;
      const id = lookup(appCommands, keymap, sc);
      const command = id ? appCommands.find((c) => c.id === id) : undefined;
      if (!command) return;
      const typing = (e.target as HTMLElement)?.closest?.("input, textarea, select, [contenteditable]");
      if (typing && !command.whileTyping) return;
      e.preventDefault();
      command.run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // The rail's order and shortcuts moved (#83). Once: keep the shortcuts the
  // user chose working, and say where the defaults went.
  useEffect(() => {
    let was: string | null;
    try {
      was = localStorage.getItem(RAIL_KEY);
      if (was === RAIL) return;
      localStorage.setItem(RAIL_KEY, RAIL);
    } catch {
      return;
    }
    // Nothing to move for someone opening spor for the first time.
    if (firstRun) return;
    const under = "Work, Changes, History and the browsers now sit under the open repository; the rail has the Inbox and Agents, across every repository.";
    // Only the layout moved since #83; the shortcuts are as they were.
    if (was === "2") {
      toast(under);
      return;
    }
    const kept = keepChanges(appCommands, getSettings().keymap);
    if (JSON.stringify(kept) !== JSON.stringify(getSettings().keymap)) setSettings({ keymap: kept });
    const keys = keyLabel("shortcuts");
    const moved = `${under} The sections' shortcuts moved${keys ? ` (${keys} lists them)` : ""}.`;
    toast(moved, "ok", false, { label: "Show shortcuts", run: () => setSettingsPage("shortcuts") });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // ⌘K also looks in what was said in agents' conversations (#108). A stable
  // function, so the palette doesn't start its pause over on every render.
  const goRef = useRef<Shell["go"] | null>(null);
  goRef.current = shell?.go ?? null;
  const searchConversations = useCallback(
    (query: string): Promise<Command[]> =>
      api.searchTranscripts(query).then((hits) =>
        hits.map((h, i) => ({
          id: `said:${h.task}:${h.run}:${i}`,
          label: h.snippet,
          hint: `${h.title} · ${h.kind === "prompt" ? (h.run === 0 ? "prompt" : "follow-up") : h.kind === "agent" ? "the agent said" : h.kind === "steer" ? "you steered" : "debrief"}`,
          run: () => goRef.current?.({ section: "work", taskId: h.task }),
        })),
      ),
    [],
  );

  const commands = (branches: BranchInfo[]): Command[] => {
    if (!shell) return [];
    return [
      ...appCommands.filter((c) => c.id !== "palette").map((c) => ({ id: c.id, label: c.label, hint: c.hint, keys: keyLabel(c.id), run: c.run })),
      // Work items open their page in Work.
      ...tasks
        .filter((t) => !t.landed)
        .map((t) => ({
          id: `work:${t.id}`,
          label: `Open ${t.title}`,
          hint: t.pull != null ? `work item · #${t.pull}` : t.issue != null ? `work item · issue #${t.issue}` : "work item",
          run: () => shell.go({ section: "work", taskId: t.id }),
        })),
      ...branches
        .filter((b) => !b.current && !b.remote)
        .map((b) => ({
          id: `co:${b.name}`,
          label: `Switch to ${b.name}`,
          hint: "branch",
          run: () => shell.switchBranch((stash) => api.checkout(b.name, stash)),
        })),
      ...workspaces.list.flatMap((w) =>
        w.repos
          .filter((r) => r !== repo?.root)
          .map((r) => ({ id: `repo:${r}`, label: `Open ${r.split("/").pop()}`, hint: `${w.name} · ${r}`, run: () => openRepo(r) })),
      ),
      ...branches
        .filter((b) => b.remote)
        .map((b) => ({
          id: `co:${b.name}`,
          label: `Check out ${b.name}`,
          hint: "remote branch",
          run: () => shell.switchBranch((stash) => api.checkout(b.name, stash)),
        })),
    ];
  };

  const settingsPages: SettingsPage[] = [
    {
      id: "appearance",
      label: "Appearance",
      preview: true,
      render: () => (
        <AppearancePage
          toast={toast}
          prompt={(spec) => new Promise((resolve) => setDialog({ kind: "prompt", ...spec, resolve }))}
        />
      ),
    },
    { id: "agents", label: "Agents", render: () => <AgentLimit /> },
    { id: "diffs", label: "Diffs", render: () => <DiffSettings /> },
    { id: "history", label: "History graph", render: () => <GraphOptionsPanel /> },
    {
      id: "shortcuts",
      label: "Shortcuts",
      render: () => (
        <ShortcutsPage
          commands={appCommands.map((c) => ({ id: c.id, label: c.id === "undo" ? "Undo" : c.label, group: c.group, keys: c.keys }))}
          menu={(e, items) => {
            e.preventDefault?.();
            setMenu({ x: e.clientX, y: e.clientY, items });
          }}
        />
      ),
    },
  ];

  const overlays = (
    <>
      {settingsPage && <SettingsDialog pages={settingsPages} initial={settingsPage} onClose={() => setSettingsPage(null)} />}
      {menu && <MenuView spec={menu} onClose={() => setMenu(null)} />}
      {dialog && <DialogView spec={dialog} onClose={() => setDialog(null)} />}
      {palette && <Palette
          commands={commands(palette.branches)}
          onClose={() => setPalette(null)}
          search={searchConversations}
        />}
      {credential && (
        <CredentialDialog
          ask={credential}
          onAnswer={(answer) => {
            setCredential(null);
            api.askpassAnswer(credential.id, answer).catch(fail);
          }}
        />
      )}
    </>
  );

  const workspaceRail = (
    <div className="rail-workspaces" aria-label="Workspaces">
      {workspaces.list.map((w, i) => (
        <button
          key={w.id}
          className={`workspace-badge${w.id === ws.id && !global ? " active" : ""}`}
          style={{ "--ws": WORKSPACE_COLORS[w.color] } as React.CSSProperties}
          onClick={() => goToWorkspace(w.id)}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, items: workspaceMenu(w) });
          }}
          title={`${w.name}${keyLabel(`workspace:${i + 1}`) ? ` (${keyLabel(`workspace:${i + 1}`)})` : ""} — right-click to rename, recolor or delete`}
          aria-label={`Workspace ${w.name}`}
          aria-current={w.id === ws.id && !global ? "true" : undefined}
          data-current={w.id === ws.id || undefined}
        >
          {initials(w.name)}
        </button>
      ))}
      <button className="workspace-badge add" onClick={newWorkspace} title="New workspace" aria-label="New workspace">
        +
      </button>
      <span className="rail-divider" />
    </div>
  );
  const settingsButton = (
    <button
      className="rail-item"
      onClick={() => setSettingsPage("appearance")}
      title={keyLabel("settings") ? `Settings (${keyLabel("settings")})` : "Settings"}
      aria-label="Settings"
    >
      <Gear size={17} />
    </button>
  );

  /** A section's link: on the rail (the Inbox), in the repository column
   * under the open repository (named), or in the strip that stands in for
   * the column while it's hidden (icons). */
  const sectionLink = (s: (typeof SECTIONS)[number], where: "rail" | "panel" | "strip") => {
    // One number on the rail: how many things need you.
    const count = s.id === "inbox" ? inbox.items.length : 0;
    const keys = keyLabel(`go:${s.id}`);
    return (
      <button
        key={s.id}
        className={`nav-section ${where === "panel" ? "repo-section" : "rail-item"}${s.browser ? " browser" : ""}${section === s.id ? " active" : ""}`}
        onClick={() => setSection(s.id)}
        title={where === "panel" ? keys || undefined : keys ? `${s.label} (${keys})` : s.label}
        aria-label={s.label}
        aria-current={section === s.id ? "page" : undefined}
      >
        {s.icon}
        {where === "panel" && <span className="repo-section-label">{s.label}</span>}
        {count > 0 && <span className="rail-badge num asking">{count}</span>}
      </button>
    );
  };
  const repoSections = (where: "panel" | "strip") =>
    SECTIONS.filter((s) => !s.global).map((s, i, all) => (
      <Fragment key={s.id}>
        {s.browser && !all[i - 1]?.browser && <span className={where === "panel" ? "repo-sections-divider" : "rail-divider"} />}
        {sectionLink(s, where)}
      </Fragment>
    ));

  if (repo === undefined) return <div className="boot" />;
  if (repo === null || !shell)
    return (
      <div className="app">
        <nav className="rail" aria-label="Workspaces">
          {workspaceRail}
          <span className="flex-1" />
          {settingsButton}
        </nav>
        <OpenRepo
          workspace={ws.name}
          recent={recent}
          onOpen={(path) => api.openRepo(path).then(setRepo, fail)}
          onForget={removeRepo}
          toasts={toasts}
        />
        {overlays}
      </div>
    );

  return (
    <ShellContext.Provider value={shell}>
      <div className={`app ${global ? "global-view" : reposShown ? "with-repos" : "with-strip"}`}>
        <nav className="rail" aria-label="Workspaces">
          {workspaceRail}
          {SECTIONS.filter((s) => s.global).map((s) => sectionLink(s, "rail"))}
          <span className="flex-1" />
          {settingsButton}
        </nav>
        {global ? null : reposShown ? (
          <RepoPanel color={WORKSPACE_COLORS[ws.color]} changed={changed}>
            {repoSections("panel")}
          </RepoPanel>
        ) : (
          <nav className="section-strip" aria-label={`${repo.name}: sections`} style={{ "--ws": WORKSPACE_COLORS[ws.color] } as React.CSSProperties}>
            <button
              className="section-strip-repo"
              onClick={() => setReposShown(true)}
              title={`${repo.name} — show the repositories${keyLabel("repos") ? ` (${keyLabel("repos")})` : ""}`}
              aria-label="Show repositories"
            >
              {repo.name.slice(0, 2)}
            </button>
            {repoSections("strip")}
          </nav>
        )}
        <div className="section">
          {state && !global && <BranchBar state={state} />}
          {state?.operation && !global && <OperationBanner state={state} />}
          <div className="section-body">
            <Sections
              key={repo.root}
              section={section}
              globals={{
                inbox: <InboxSection inbox={inbox} onOpen={openItem} />,
                agents: (
                  <AgentsSection
                    inbox={inbox}
                    selected={agentOpen}
                    page={
                      agentOpen && agentOpen.repo === repo.root ? (
                        <WorkSection key={agentOpen.taskId} embedded={{ taskId: agentOpen.taskId, onGone: () => setAgentOpen(null) }} />
                      ) : (
                        <main className="main">
                          <div className="boot" />
                        </main>
                      )
                    }
                    onOpen={(r, taskId) => setAgentOpen({ repo: r, taskId })}
                    onClose={() => setAgentOpen(null)}
                    onOpenRepo={(r) => openItem({ repo: r, target: { section: "work" } })}
                  />
                ),
              }}
            />
          </div>
        </div>
        <Toasts toasts={toasts} onUndo={undo} onClose={(id) => setToasts((t) => t.filter((x) => x.id !== id))} />
        {overlays}
      </div>
    </ShellContext.Provider>
  );
}

type GlobalId = "inbox" | "agents";

const SECTION_VIEWS: Record<Exclude<SectionId, GlobalId>, () => React.ReactNode> = {
  changes: ChangesSection,
  history: HistorySection,
  issues: IssuesSection,
  pulls: PullsSection,
  actions: ActionsSection,
  work: WorkSection,
};

/** The sections opened so far, kept mounted so going back to one shows it
 * as it was at once, and then brings it up to date. A hidden one keeps its
 * state but not its effects: it doesn't poll, watch or take keys. */
function Sections({ section, globals }: { section: SectionId; globals: Record<GlobalId, React.ReactNode> }) {
  const [opened, setOpened] = useState<SectionId[]>([section]);
  if (!opened.includes(section)) setOpened([...opened, section]);
  return (
    <>
      <KeepScroll shown={section} />
      {SECTIONS.filter((s) => opened.includes(s.id)).map((s) => {
        const View = s.id === "inbox" || s.id === "agents" ? null : SECTION_VIEWS[s.id];
        return (
          <Activity key={s.id} mode={s.id === section ? "visible" : "hidden"}>
            {View ? <View /> : globals[s.id as GlobalId]}
          </Activity>
        );
      })}
    </>
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
        <Button
          className="primary small"
          onClick={() => shell.act(api.continueOperation, { undo: false, pending: `Continuing the ${what.toLowerCase()}…` })}
        >
          Continue {what.toLowerCase()}
        </Button>
      )}
      {state.operation !== "merge" && (
        <Button
          className="ghost small"
          title="Leave out the commit it stopped on"
          onClick={() => shell.act(api.skipOperation, { undo: false, pending: "Skipping the commit…" })}
        >
          Skip commit
        </Button>
      )}
      <Button
        className="ghost small"
        onClick={() => shell.act(api.abortOperation, { undo: false, pending: `Aborting the ${what.toLowerCase()}…` })}
      >
        Abort
      </Button>
    </div>
  );
}

function Toasts({ toasts, onUndo, onClose }: { toasts: Toast[]; onUndo: () => void; onClose: (id: number) => void }) {
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.tone}`}>
          {t.tone === "progress" && <span className="spinner" aria-hidden />}
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
          {t.tone !== "progress" && (
            <button className="icon-button tiny" onClick={() => onClose(t.id)} aria-label="Dismiss">
              <X size={11} />
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

function OpenRepo({
  workspace,
  recent,
  onOpen,
  onForget,
  toasts,
}: {
  workspace: string;
  recent: string[];
  onOpen: (path: string) => Promise<unknown>;
  onForget: (path: string) => void;
  toasts: Toast[];
}) {
  // The one being opened: it shows busy, and nothing else opens meanwhile.
  const [opening, setOpening] = useState<string | null>(null);
  const open = (path: string) => {
    if (opening || !path.trim()) return;
    setOpening(path);
    onOpen(path).finally(() => setOpening(null));
  };
  return (
    <div className="open-repo">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          open(new FormData(e.currentTarget).get("path") as string);
        }}
      >
        <h1>Open a repository</h1>
        <p className="muted small-text">It joins the workspace {workspace}.</p>
        <p className="lede">
          <Branch size={12} /> Spor shows its history, changes, issues, pull requests, Actions and agent tasks.
        </p>
        <input name="path" autoFocus placeholder="~/code/my-project" />
        <Button className="primary" type="submit" busy={opening != null && !recent.includes(opening)}>
          Open
        </Button>
        {recent.length > 0 && (
          <div className="recent-repos">
            <div className="list-label">In {workspace}</div>
            {recent.map((r) => (
              <div key={r} className="recent-row">
                <button
                  type="button"
                  className={`recent-open${opening === r ? " opening" : ""}`}
                  onClick={() => open(r)}
                  disabled={opening != null}
                  aria-busy={opening === r || undefined}
                  title={r}
                >
                  <span className="repo-name">{r.split("/").pop()}</span>
                  <span className="muted small-text truncate">{r}</span>
                  {opening === r && <span className="spinner" aria-hidden />}
                </button>
                <button type="button" className="icon-button tiny" title={`Remove from ${workspace}`} onClick={() => onForget(r)}>
                  <X size={11} />
                </button>
              </div>
            ))}
          </div>
        )}
      </form>
      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone}`}>
            <span>{t.text}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
