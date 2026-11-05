// The pieces every section is built from: a sidebar with the repository at
// the top, a page header, and a details card.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { RepoHead } from "../types";
import { useShell } from "./app";
import { Branch } from "./Icons";

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
  const { repo, recent, openRepo, forgetRepo, menu, workspaceName, otherWorkspaces, moveRepo } = useShell();
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
  const sorted = [...recent].sort((a, b) => (a.split("/").pop() ?? a).localeCompare(b.split("/").pop() ?? b));
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
            onContextMenu={(e) =>
              menu(e, [
                ...otherWorkspaces.map((w) => ({ label: `Move to ${w.name}`, run: () => moveRepo(path, w.id) })),
                { label: "Copy path", separator: otherWorkspaces.length > 0, run: () => navigator.clipboard.writeText(path) },
                { label: `Remove from ${workspaceName}`, danger: true, separator: true, disabled: here, run: () => forgetRepo(path) },
              ])
            }
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

export function EmptyState({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <div className="empty-state">
      <h3>{title}</h3>
      {children && <div className="lede">{children}</div>}
    </div>
  );
}
