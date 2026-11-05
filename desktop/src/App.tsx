import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, errorText } from "./api";
import type { BranchInfo, Credential, RepoInfo, RepoState } from "./types";
import { usePoll, useStored } from "./shared/hooks";
import { ShellContext, useShell, type SectionId, type Shell, type Target, type ToastAction } from "./shared/app";
import { Alert, Bot, Branch, FileIcon, Gear, History as HistoryIcon, IssueIcon, Play, PullRequest, X } from "./shared/Icons";
import { DiffSettings, SettingsDialog, type SettingsPage } from "./shared/Settings";
import { GraphOptionsPanel } from "./sections/history/GraphOptions";
import { ShortcutsPage } from "./shared/Shortcuts";
import { display, effective, isMac, lookup, shortcutOf } from "./shared/keys";
import { getSettings, setSettings, useSettings } from "./shared/settings";
import { CredentialDialog, DialogView, MenuView, Palette, type Command, type DialogSpec, type MenuSpec } from "./shared/Overlay";
import { AgentsSection } from "./sections/agents/AgentsSection";
import { HistorySection } from "./sections/history/HistorySection";
import { ChangesSection } from "./sections/changes/ChangesSection";
import { IssuesSection } from "./sections/issues/IssuesSection";
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
  const sync = (what: "fetch" | "pull" | "push") => () => shell.act(() => api.sync(what), { undo: what === "pull" });
  return [
    { id: "fetch", label: "Fetch", hint: "all remotes", group: "Git", run: sync("fetch") },
    { id: "pull", label: "Pull", hint: "with rebase", group: "Git", run: sync("pull") },
    { id: "push", label: shell.state?.upstream ? "Push" : "Publish branch", group: "Git", run: sync("push") },
    { id: "stash", label: "Stash changes", group: "Git", run: () => shell.act(() => api.stash(null), { undo: false }) },
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
const RECENT_MAX = 12;

export function App() {
  const [repo, setRepoRaw] = useState<RepoInfo | null | undefined>(undefined);
  const [recent, setRecent] = useStored<string[]>("recent", []);
  const [imported, setImported] = useStored("recent.imported", false);
  const [menu, setMenu] = useState<MenuSpec | null>(null);
  const [dialog, setDialog] = useState<DialogSpec | null>(null);
  const [credential, setCredential] = useState<Credential | null>(null);
  const [palette, setPalette] = useState<{ branches: BranchInfo[] } | null>(null);
  /** The Settings page open, if Settings is. */
  const [settingsPage, setSettingsPage] = useState<string | null>(null);
  const { keymap } = useSettings();
  const mac = isMac();
  const setRepo = useCallback(
    (r: RepoInfo | null) => {
      setRepoRaw(r);
      if (r) setRecent((old) => [r.root, ...old.filter((x) => x !== r.root)].slice(0, RECENT_MAX));
    },
    [], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const [state, setState] = useState<RepoState | null>(null);
  const [section, setSection] = useStored<SectionId>("section", "agents");
  const [target, setTarget] = useState<Target & { seq: number }>({ section: "agents", seq: 0 });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [badges, setBadges] = useState<{ changes: number; working: number; asking: number }>({
    changes: 0,
    working: 0,
    asking: 0,
  });
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

  useEffect(() => {
    api.currentRepo().then(setRepo, () => setRepo(null));
    // The old native app's repositories, once, so the list carries over.
    if (!imported)
      api
        .legacyRepos()
        .then((paths) => {
          setRecent((old) => [...old, ...paths.filter((p) => !old.includes(p))].slice(0, RECENT_MAX));
          setImported(true);
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
    api
      .sync("pull")
      .then((m) => toast(m, "ok", true), (e) => toast(errorText(e), "error"))
      .finally(() => refresh());
  usePoll(refresh, 2000, [repo?.root], !!repo);

  // Rail counts: files changed, agents working.
  usePoll(
    async () => {
      const [files, tasks] = await Promise.all([api.status(), api.listTasks()]);
      const waiting = tasks.filter((t) => t.asks > 0);
      setBadges({
        changes: new Set(files.map((f) => f.path)).size,
        working: tasks.filter((t) => t.run?.state === "running").length,
        asking: waiting.length,
      });
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
    },
    2000,
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

  const openRepo = (path: string) =>
    api
      .openRepo(path)
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
            openRepo,
            recent,
            forgetRepo: (path) => setRecent((old) => old.filter((x) => x !== path)),
            menu: (e, items) => {
              e.preventDefault?.();
              setMenu({ x: e.clientX, y: e.clientY, items });
            },
            prompt: (spec) => new Promise((resolve) => setDialog({ kind: "prompt", ...spec, resolve })),
            confirm: (spec) => new Promise((resolve) => setDialog({ kind: "confirm", ...spec, resolve })),
          }
        : null,
    [repo, state, refresh, toast, fail, undo, target, recent], // eslint-disable-line react-hooks/exhaustive-deps
  );

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
    { id: "settings", label: "Settings", group: "App", keys: "Mod+,", whileTyping: true, run: () => setSettingsPage("diffs") },
    { id: "shortcuts", label: "Keyboard shortcuts", group: "App", keys: "Mod+/", whileTyping: true, run: () => setSettingsPage("shortcuts") },
    // Not while typing: there ⌘Z undoes the typing.
    { id: "undo", label: state?.undo ? `Undo: ${state.undo}` : "Undo", group: "App", keys: "Mod+Z", run: () => undo() },
    {
      id: "diff-layout",
      label: getSettings().diffLayout === "split" ? "Show diffs unified" : "Show diffs side by side",
      group: "App",
      whileTyping: true,
      run: () => setSettings({ diffLayout: getSettings().diffLayout === "split" ? "unified" : "split" }),
    },
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
      ...recent
        .filter((r) => r !== repo?.root)
        .map((r) => ({ id: `repo:${r}`, label: `Open ${r.split("/").pop()}`, hint: r, run: () => openRepo(r) })),
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
      {menu && <MenuView spec={menu} onClose={() => setMenu(null)} />}
      {dialog && <DialogView spec={dialog} onClose={() => setDialog(null)} />}
      {settingsPage && <SettingsDialog pages={settingsPages} initial={settingsPage} onClose={() => setSettingsPage(null)} />}
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

  if (repo === undefined) return <div className="boot" />;
  if (repo === null || !shell)
    return (
      <OpenRepo
        recent={recent}
        onOpen={(path) => api.openRepo(path).then(setRepo, fail)}
        onForget={(path) => setRecent((old) => old.filter((x) => x !== path))}
        toasts={toasts}
      />
    );

  return (
    <ShellContext.Provider value={shell}>
      <div className="app">
        <nav className="rail" aria-label="Sections">
          {SECTIONS.map((s) => {
            const count =
              s.id === "changes"
                ? badges.changes
                : s.id === "agents"
                  ? badges.asking || badges.working
                  : 0;
            const tone = s.id === "agents" ? (badges.asking ? " asking" : " live") : "";
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
          <button className="rail-item" onClick={() => setSettingsPage("diffs")} title={keyLabel("settings") ? `Settings (${keyLabel("settings")})` : "Settings"} aria-label="Settings">
            <Gear size={17} />
          </button>
        </nav>
        <div className="section">
          {state?.operation && <OperationBanner state={state} />}
          <div className="section-body">
            {section === "agents" && <AgentsSection />}
            {section === "history" && <HistorySection />}
            {section === "changes" && <ChangesSection />}
            {section === "issues" && <IssuesSection />}
            {section === "pulls" && <PullsSection />}
            {section === "actions" && <ActionsSection />}
          </div>
        </div>
        <Toasts toasts={toasts} onUndo={undo} onClose={(id) => setToasts((t) => t.filter((x) => x.id !== id))} />
        {overlays}
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
      {state.operation !== "merge" && (
        <button
          className="ghost small"
          title="Leave out the commit it stopped on"
          onClick={() => shell.act(api.skipOperation, { undo: false })}
        >
          Skip commit
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

function OpenRepo({
  recent,
  onOpen,
  onForget,
  toasts,
}: {
  recent: string[];
  onOpen: (path: string) => void;
  onForget: (path: string) => void;
  toasts: Toast[];
}) {
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
          <Branch size={12} /> Spor shows its history, changes, issues, pull requests, Actions and agent tasks.
        </p>
        <input name="path" autoFocus placeholder="~/code/my-project" />
        <button className="primary" type="submit">
          Open
        </button>
        {recent.length > 0 && (
          <div className="recent-repos">
            <div className="list-label">Recent</div>
            {recent.map((r) => (
              <div key={r} className="recent-row">
                <button type="button" className="recent-open" onClick={() => onOpen(r)} title={r}>
                  <span className="repo-name">{r.split("/").pop()}</span>
                  <span className="muted small-text truncate">{r}</span>
                </button>
                <button type="button" className="icon-button tiny" title="Remove from the list" onClick={() => onForget(r)}>
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
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
