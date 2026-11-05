import { Activity, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, errorText } from "./api";
import type { BranchInfo, Credential, RepoInfo, RepoState, TaskSummary } from "./types";
import { KeepScroll, usePoll, useRepoChanges, useStored } from "./shared/hooks";
import { ShellContext, useShell, type SectionId, type Shell, type Target, type ToastAction } from "./shared/app";
import { Alert, Bot, Branch, FileIcon, Gear, History as HistoryIcon, IssueIcon, Play, PullRequest, X } from "./shared/Icons";
import { Button } from "./shared/Button";
import { RepoPanel } from "./shared/Layout";
import { DiffSettings, SettingsDialog, type SettingsPage } from "./shared/SettingsDialog";
import { GraphOptionsPanel } from "./sections/history/GraphOptions";
import { ShortcutsPage } from "./shared/Shortcuts";
import { AppearancePage } from "./shared/ThemeEditor";
import { useApplyTheme } from "./shared/theme";
import { display, effective, isMac, lookup, shortcutOf } from "./shared/keys";
import { getSettings, setSettings, useSettings } from "./shared/settings";
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
import { AgentsSection } from "./sections/agents/AgentsSection";
import { HistorySection } from "./sections/history/HistorySection";
import { ChangesSection } from "./sections/changes/ChangesSection";
import { IssuesSection } from "./sections/issues/IssuesSection";
import { PullsSection } from "./sections/pulls/PullsSection";
import { ActionsSection } from "./sections/actions/ActionsSection";

interface Toast {
  id: number;
  text: string;
  /** progress: something still going; it stays until that's done. */
  tone: "ok" | "error" | "progress";
  undo?: boolean;
  action?: ToastAction;
}

const SECTIONS: { id: SectionId; label: string; icon: React.ReactNode; key: string }[] = [
  { id: "changes", label: "Changes", icon: <FileIcon size={17} />, key: "Mod+1" },
  { id: "history", label: "History", icon: <HistoryIcon size={17} />, key: "Mod+2" },
  { id: "issues", label: "Issues", icon: <IssueIcon size={17} />, key: "Mod+3" },
  { id: "pulls", label: "Pull requests", icon: <PullRequest size={17} />, key: "Mod+4" },
  { id: "actions", label: "Actions", icon: <Play size={17} />, key: "Mod+5" },
  { id: "agents", label: "Agents", icon: <Bot size={17} />, key: "Mod+6" },
];

/** Is the task open on screen (Agents, its page)? Then it needs no toast. */
function viewing(taskId: string): boolean {
  try {
    const section = JSON.parse(localStorage.getItem("spor.section") ?? "null");
    const view = JSON.parse(localStorage.getItem("spor.agents.view") ?? "null");
    return section === "agents" && view?.kind === "task" && view.id === taskId && document.hasFocus();
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
  const [section, setSection] = useStored<SectionId>("section", "agents");
  const [reposShown, setReposShown] = useStored("repos.shown", true);
  const [target, setTarget] = useState<Target & { seq: number }>({ section: "agents", seq: 0 });
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

  // The rail's count of files changed.
  useRepoChanges(
    async () => {
      const files = await api.status();
      setChanged(new Set(files.map((f) => f.path)).size);
    },
    ["index", "worktree"],
    [repo?.root],
    !!repo,
  );

  // Agent tasks, loaded here once for the rail, Agents and Issues: a task's
  // commits are refs, the rest is in `.git/spor/tasks`.
  const reloadTasks = useCallback(async () => {
    const list = await api.listTasks();
    setTasks(list);
    const waiting = list.filter((t) => t.asks > 0);
    if (asking.current) {
      for (const t of waiting)
        if (!asking.current.has(t.id) && !viewing(t.id))
          toast(`${t.title} is waiting for you`, "ok", false, {
            label: "Answer",
            run: () => {
              setSection("agents");
              setTarget((old) => ({ section: "agents", taskId: t.id, seq: old.seq + 1 }));
            },
          });
    }
    asking.current = new Set(waiting.map((t) => t.id));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useRepoChanges(reloadTasks, ["tasks", "refs"], [repo?.root], !!repo);
  const working = tasks.filter((t) => t.run?.state === "running").length;
  const askingNow = tasks.filter((t) => t.asks > 0).length;

  const undo = useCallback(() => {
    track("Undoing…", api.undo)
      .then(async (msg) => {
        toast(msg);
        await refresh();
      })
      .catch(fail);
  }, [toast, fail, refresh, track]);

  const openRepo = (path: string) =>
    track(`Opening ${path.split("/").pop()}…`, () => api.openRepo(path))
      .then((r) => {
        lastState.current = null;
        setRepo(r);
        setState(null);
      })
      .catch(fail);

  const shell: Shell | null = useMemo(
    () =>
      repo
        ? {
            repo,
            state,
            refresh,
            tasks,
            reloadTasks,
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
            go: (t) => {
              setSection(t.section);
              setTarget((old) => ({ ...t, seq: old.seq + 1 }));
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
    [repo, state, refresh, tasks, toast, fail, undo, track, target, workspaces], // eslint-disable-line react-hooks/exhaustive-deps
  );

  /** Switch workspace, opening the repository last open there. */
  const goToWorkspace = (id: string) => {
    const target = workspaces.list.find((w) => w.id === id);
    if (!target) return;
    switchTo(id);
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

  const commands = (branches: BranchInfo[]): Command[] => {
    if (!shell) return [];
    return [
      ...appCommands.filter((c) => c.id !== "palette").map((c) => ({ id: c.id, label: c.label, hint: c.hint, keys: keyLabel(c.id), run: c.run })),
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
      {palette && <Palette commands={commands(palette.branches)} onClose={() => setPalette(null)} />}
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
          className={`workspace-badge${w.id === ws.id ? " active" : ""}`}
          style={{ "--ws": WORKSPACE_COLORS[w.color] } as React.CSSProperties}
          onClick={() => goToWorkspace(w.id)}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, items: workspaceMenu(w) });
          }}
          title={`${w.name}${keyLabel(`workspace:${i + 1}`) ? ` (${keyLabel(`workspace:${i + 1}`)})` : ""} — right-click to rename, recolor or delete`}
          aria-label={`Workspace ${w.name}`}
          aria-current={w.id === ws.id ? "true" : undefined}
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
      <div className={`app${reposShown ? " with-repos" : ""}`}>
        <nav className="rail" aria-label="Sections">
          {workspaceRail}
          {SECTIONS.map((s) => {
            const count = s.id === "changes" ? changed : s.id === "agents" ? askingNow || working : 0;
            const tone = s.id === "agents" ? (askingNow ? " asking" : " live") : "";
            return (
              <button
                key={s.id}
                className={`rail-item${section === s.id ? " active" : ""}`}
                onClick={() => setSection(s.id)}
                title={keyLabel(`go:${s.id}`) ? `${s.label} (${keyLabel(`go:${s.id}`)})` : s.label}
                aria-label={s.label}
                aria-current={section === s.id ? "page" : undefined}
              >
                {s.icon}
                {count > 0 && <span className={`rail-badge num${tone}`}>{count}</span>}
              </button>
            );
          })}
          <span className="flex-1" />
          {settingsButton}
        </nav>
        {reposShown && <RepoPanel color={WORKSPACE_COLORS[ws.color]} changed={changed} />}
        <div className="section">
          {state?.operation && <OperationBanner state={state} />}
          <div className="section-body">
            <Sections key={repo.root} section={section} />
          </div>
        </div>
        <Toasts toasts={toasts} onUndo={undo} onClose={(id) => setToasts((t) => t.filter((x) => x.id !== id))} />
        {overlays}
      </div>
    </ShellContext.Provider>
  );
}

const SECTION_VIEWS: Record<SectionId, () => React.ReactNode> = {
  changes: ChangesSection,
  history: HistorySection,
  issues: IssuesSection,
  pulls: PullsSection,
  actions: ActionsSection,
  agents: AgentsSection,
};

/** The sections opened so far, kept mounted so going back to one shows it
 * as it was at once, and then brings it up to date. A hidden one keeps its
 * state but not its effects: it doesn't poll, watch or take keys. */
function Sections({ section }: { section: SectionId }) {
  const [opened, setOpened] = useState<SectionId[]>([section]);
  if (!opened.includes(section)) setOpened([...opened, section]);
  return (
    <>
      <KeepScroll shown={section} />
      {SECTIONS.filter((s) => opened.includes(s.id)).map((s) => {
        const View = SECTION_VIEWS[s.id];
        return (
          <Activity key={s.id} mode={s.id === section ? "visible" : "hidden"}>
            <View />
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
