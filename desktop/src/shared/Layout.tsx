// The pieces every section is built from: a sidebar with the repository at
// the top, a page header, and a details card.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { RepoHead } from "../types";
import { useRepoChanges } from "./hooks";
import { useShell, type Shell } from "./app";
import { Branch, Folder, Plus } from "./Icons";

/** The top of every sidebar: the repository (click to switch to a recent
 * one or open another) and the section's own buttons. */
export function SidebarHeader({ children }: { children?: React.ReactNode }) {
  const { repo, state, openRepo } = useShell();
  const [editing, setEditing] = useState(false);
  const [switching, setSwitching] = useState(false);
  const branch = state?.branch ?? repo.branch;
  return (
    <div className="sidebar-top">
      {editing ? (
        <form
          className="repo-form"
          onSubmit={(e) => {
            e.preventDefault();
            openRepo(new FormData(e.currentTarget).get("path") as string);
            setEditing(false);
          }}
        >
          <input
            name="path"
            autoFocus
            defaultValue={repo.root}
            onBlur={() => setEditing(false)}
            onKeyDown={(e) => e.key === "Escape" && setEditing(false)}
            placeholder="Path to a repository"
          />
        </form>
      ) : (
        <button
          className={`repo-button${switching ? " open" : ""}`}
          title={`${repo.root} — click to switch repository`}
          onClick={() => setSwitching((v) => !v)}
          aria-expanded={switching}
        >          <span className="repo-name">{repo.name}</span>
          <span className="repo-branch">
            <Branch size={11} /> {branch ?? "detached HEAD"}
            {state && (state.ahead > 0 || state.behind > 0) && (
              <span className="num">
                {state.behind > 0 && ` ↓${state.behind}`}
                {state.ahead > 0 && ` ↑${state.ahead}`}
              </span>
            )}
          </span>
        </button>
      )}
      {switching && <RepoSwitcher onClose={() => setSwitching(false)} onOpenOther={() => setEditing(true)} />}
      {children}
    </div>
  );
}

/** The workspace's repositories with their branch and state: click to
 * switch, right-click to move one to another workspace or remove it. */
function RepoSwitcher({ onClose, onOpenOther }: { onClose: () => void; onOpenOther: () => void }) {
  const shell = useShell();
  const { repo, recent, openRepo, menu, workspaceName } = shell;
  const [heads, setHeads] = useState<Map<string, RepoHead>>(new Map());
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    api.repoHeads(recent).then((list) => setHeads(new Map(list.map((h) => [h.path, h]))), () => undefined);
  }, [recent.join("\n")]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    const down = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      // Leave it open for the right-click menu it opens.
      if (!ref.current?.contains(t) && !t.closest(".menu.floating") && !t.closest(".repo-button")) onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", down);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", down);
      window.removeEventListener("keydown", key);
    };
  }, [onClose]);
  const sorted = byName(recent);
  return (
    <div className="repo-switcher" ref={ref} role="dialog" aria-label={`Repositories in ${workspaceName}`}>
      <div className="list-label">{workspaceName}</div>
      {sorted.map((path) => {
        const h = heads.get(path);
        const here = path === repo.root;
        return (
          <button
            key={path}
            className={`repo-switch-row${here ? " current" : ""}${h && !h.exists ? " missing" : ""}`}
            title={h && !h.exists ? `${path} — not found` : path}
            onClick={() => {
              onClose();
              if (!here) openRepo(path);
            }}
            onContextMenu={(e) => menu(e, repoMenu(shell, path))}
          >
            <span className="repo-switch-name truncate">{path.split("/").pop()}</span>
            {h?.exists === false ? (
              <span className="muted small-text">not found</span>
            ) : (
              <>
                <span className="muted small-text truncate repo-switch-branch">
                  <Branch size={10} /> {h?.branch ?? "…"}
                </span>
                <span className="flex-1" />
                {h && h.changes > 0 && <span className="num small-text muted" title={`${h.changes} changed files`}>{h.changes}</span>}
                {h && h.behind > 0 && <span className="num small-text behind" title={`${h.behind} to pull`}>↓{h.behind}</span>}
                {h && h.ahead > 0 && <span className="num small-text muted" title={`${h.ahead} to push`}>↑{h.ahead}</span>}
              </>
            )}
          </button>
        );
      })}
      <button
        className="repo-switch-row other"
        onClick={() => {
          onClose();
          onOpenOther();
        }}
      >
        Open another repository…
      </button>
    </div>
  );
}

const byName = (paths: string[]) =>
  [...paths].sort((a, b) => (a.split("/").pop() ?? a).localeCompare(b.split("/").pop() ?? b));

/** Right-click on a repository in the switcher or the panel. */
function repoMenu(shell: Shell, path: string) {
  const { repo, otherWorkspaces, moveRepo, workspaceName, forgetRepo } = shell;
  return [
    ...otherWorkspaces.map((w) => ({ label: `Move to ${w.name}`, run: () => moveRepo(path, w.id) })),
    { label: "Copy path", separator: otherWorkspaces.length > 0, run: () => navigator.clipboard.writeText(path) },
    { label: `Remove from ${workspaceName}`, danger: true, separator: true, disabled: path === repo.root, run: () => forgetRepo(path) },
  ];
}

/** The workspace's repositories in a column beside the rail, the open one
 * marked with its sections listed under it: workspace, then repository,
 * then section, left to right and top to bottom (⌘⇧E hides the column).
 * The open one is live; the others are read on opening, when a ref moves
 * here, on coming back to the window and every 30 s. */
export function RepoPanel({ color, changed, children }: { color: string; changed: number; children?: React.ReactNode }) {
  const shell = useShell();
  const { repo, state, recent, openRepo, menu, workspaceName } = shell;
  const [heads, setHeads] = useState<Map<string, RepoHead>>(new Map());
  const load = () =>
    api.repoHeads(recent).then((list) => setHeads(new Map(list.map((h) => [h.path, h]))), () => undefined);
  const paths = recent.join("\n");
  useRepoChanges(load, ["refs"], [paths, repo.root]);
  useEffect(() => {
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [paths]); // eslint-disable-line react-hooks/exhaustive-deps
  const openOther = async () => {
    const r = await shell.prompt({ title: "Open a repository", placeholder: "Path to a repository", action: "Open" });
    if (r?.value.trim()) openRepo(r.value.trim());
  };
  return (
    <aside className="repo-panel" aria-label={`Repositories in ${workspaceName}`} style={{ "--ws": color } as React.CSSProperties}>
      <div className="repo-panel-top">
        <span className="repo-panel-workspace truncate">{workspaceName}</span>
        <button className="icon-button tiny" title="Open another repository" aria-label="Open another repository" onClick={openOther}>
          <Plus size={14} />
        </button>
      </div>
      <div className="repo-panel-list">
        {byName(recent).map((path) => {
          const here = path === repo.root;
          const h: RepoHead | undefined = here
            ? {
                path,
                exists: true,
                branch: state?.branch ?? repo.branch ?? null,
                ahead: state?.ahead ?? 0,
                behind: state?.behind ?? 0,
                changes: changed,
              }
            : heads.get(path);
          const missing = h?.exists === false;
          const row = (
            <button
              key={path}
              className={`repo-panel-row${here ? " current" : ""}${missing ? " missing" : ""}`}
              title={missing ? `${path} — not found` : path}
              aria-current={here ? "true" : undefined}
              onClick={() => !here && openRepo(path)}
              onContextMenu={(e) => menu(e, repoMenu(shell, path))}
            >
              <Folder size={14} />
              <span className="repo-panel-text">
                <span className="repo-panel-name truncate">{path.split("/").pop()}</span>
                <span className="repo-panel-branch truncate">
                  {missing ? (
                    "not found"
                  ) : (
                    <>
                      <Branch size={10} /> {h ? (h.branch ?? "detached HEAD") : "…"}
                    </>
                  )}
                </span>
              </span>
              {h && !missing && (
                <span className="repo-panel-counts">
                  {h.changes > 0 && <span className="num small-text dirty" title={`${h.changes} changed files`}>{h.changes}</span>}
                  {h.behind > 0 && <span className="num small-text behind" title={`${h.behind} to pull`}>↓{h.behind}</span>}
                  {h.ahead > 0 && <span className="num small-text muted" title={`${h.ahead} to push`}>↑{h.ahead}</span>}
                </span>
              )}
            </button>
          );
          if (!here) return row;
          return (
            <div key={path} className="repo-panel-open">
              {row}
              <nav className="repo-sections" aria-label={`${path.split("/").pop()}: sections`}>
                {children}
              </nav>
            </div>
          );
        })}
      </div>
    </aside>
  );
}

export function PageHeader({ title, children }: { title: React.ReactNode; children?: React.ReactNode }) {
  return (
    <header className="task-header">
      <div className="task-title">{title}</div>
      {children}
    </header>
  );
}

export function Tabs<T extends string>({
  value,
  onChange,
  tabs,
}: {
  value: T;
  onChange: (t: T) => void;
  tabs: { id: T; label: React.ReactNode }[];
}) {
  return (
    <nav className="tabs" role="tablist">
      {tabs.map((t) => (
        <button key={t.id} role="tab" aria-selected={value === t.id} onClick={() => onChange(t.id)}>
          {t.label}
        </button>
      ))}
    </nav>
  );
}

/** The floating card on the right of a page. */
export function DetailsCard({ children }: { children: React.ReactNode }) {
  return <aside className="inspector">{children}</aside>;
}

/** A list on its way (from GitHub, say): shown after a beat, so a quick
 * load doesn't flash it. */
export function ListLoading({ what }: { what: string }) {
  return (
    <div className="sidebar-empty loading" role="status">
      <span className="spinner" aria-hidden /> Loading {what}…
    </div>
  );
}

export function EmptyState({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <div className="empty-state">
      <h3>{title}</h3>
      {children && <div className="lede">{children}</div>}
    </div>
  );
}
