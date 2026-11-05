// The pieces every section is built from: a sidebar with the repository at
// the top, a page header, and a details card.

import { useState } from "react";
import { useShell } from "./app";
import { Branch } from "./Icons";

/** The top of every sidebar: the repository (click to switch to a recent
 * one or open another) and the section's own buttons. */
export function SidebarHeader({ children }: { children?: React.ReactNode }) {
  const { repo, state, openRepo, recent, forgetRepo, menu } = useShell();
  const [editing, setEditing] = useState(false);
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
          className="repo-button"
          title={`${repo.root} — click to switch repository`}
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            const others = recent.filter((x) => x !== repo.root);
            menu({ clientX: r.left, clientY: r.bottom + 4 }, [
              ...others.map((path) => ({ label: `${path.split("/").pop()} — ${path}`, run: () => openRepo(path) })),
              { label: "Open another repository…", run: () => setEditing(true), separator: true },
              ...(others.length > 0
                ? [{ label: "Clear recent repositories", run: () => others.forEach(forgetRepo) }]
                : []),
            ]);
          }}
        >
          <span className="repo-name">{repo.name}</span>
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
      {children}
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
