import { useState } from "react";
import type { RepoInfo, TaskSummary } from "../types";
import { statusOf } from "../status";
import { ago } from "../transcript";
import { useNow } from "../hooks";
import { StatusPill } from "./StatusPill";
import { AgentIcon, Branch, Chevron, Plus } from "./Icons";

interface Props {
  repo: RepoInfo;
  tasks: TaskSummary[];
  selected: string | null;
  seen: Record<string, number>;
  shelves: { working: boolean; settled: boolean };
  onShelves: (s: { working: boolean; settled: boolean }) => void;
  onSelect: (id: string) => void;
  onNew: () => void;
  onOpenRepo: (path: string) => void;
}

export function Sidebar(p: Props) {
  const anyRunning = p.tasks.some((t) => t.run?.state === "running");
  const now = useNow(1000, anyRunning);
  const nowMinute = useNow(30_000);
  // Active keeps a stable order (newest first): activity never moves a row.
  // Working and settled tasks sit in shelves at the bottom.
  const working = p.tasks.filter((t) => !t.landed && t.run?.state === "running");
  const active = p.tasks.filter((t) => !t.landed && t.run?.state !== "running");
  const settled = p.tasks.filter((t) => t.landed);
  const [editingRepo, setEditingRepo] = useState(false);

  const row = (t: TaskSummary) => {
    const status = statusOf(t, p.seen[t.id], now);
    const time = ago(t.run?.ended ?? t.run?.started ?? t.created, Math.max(now, nowMinute));
    return (
      <button
        key={t.id}
        className={`task-row${p.selected === t.id ? " selected" : ""}`}
        onClick={() => p.onSelect(t.id)}
        title={t.prompt}
      >
        <div className="task-row-line">
          <span className="task-row-title">{t.title}</span>
          {status ? <StatusPill status={status} /> : <span className="task-row-time num">{time}</span>}
        </div>
        <div className="task-row-line sub">
          <Branch size={11} className="shrink-0" />
          <span className="truncate">{t.landed ? `landed on ${t.base}` : t.branch}</span>
          <span className="flex-1" />
          {!t.landed && t.stats.files > 0 && (
            <span className="diffstat num">
              <span className="add">+{t.stats.additions}</span> <span className="del">−{t.stats.deletions}</span>
            </span>
          )}
          <AgentIcon agent={t.run?.agent ?? null} size={12} />
        </div>
      </button>
    );
  };

  const shelf = (key: "working" | "settled", label: string, items: TaskSummary[]) =>
    items.length > 0 && (
      <div className="shelf">
        <button
          className="shelf-header"
          onClick={() => p.onShelves({ ...p.shelves, [key]: !p.shelves[key] })}
          aria-expanded={p.shelves[key]}
        >
          <span>{label}</span>
          <span className="flex-1" />
          <span className="num">{items.length}</span>
          <Chevron size={12} className={`chev${p.shelves[key] ? " open" : ""}`} />
        </button>
        {p.shelves[key] && items.map(row)}
      </div>
    );

  return (
    <aside className="sidebar">
      <div className="sidebar-top">
        {editingRepo ? (
          <form
            className="repo-form"
            onSubmit={(e) => {
              e.preventDefault();
              const path = new FormData(e.currentTarget).get("path") as string;
              p.onOpenRepo(path);
              setEditingRepo(false);
            }}
          >
            <input
              name="path"
              autoFocus
              defaultValue={p.repo.root}
              onBlur={() => setEditingRepo(false)}
              onKeyDown={(e) => e.key === "Escape" && setEditingRepo(false)}
              placeholder="Path to a repository"
            />
          </form>
        ) : (
          <button className="repo-button" onClick={() => setEditingRepo(true)} title={`${p.repo.root} — click to open another`}>
            <span className="repo-name">{p.repo.name}</span>
            {p.repo.branch && (
              <span className="repo-branch">
                <Branch size={11} /> {p.repo.branch}
              </span>
            )}
          </button>
        )}
        <button className="icon-button" onClick={p.onNew} title="New task (⌘N)" aria-label="New task">
          <Plus />
        </button>
      </div>
      <div className="task-list">
        {p.tasks.length === 0 && (
          <div className="sidebar-empty">No tasks yet. Give an agent its own branch and folder with ⌘N.</div>
        )}
        {active.map(row)}
        <div className="shelves">
          {shelf("working", "Working", working)}
          {shelf("settled", "Settled", settled)}
        </div>
      </div>
    </aside>
  );
}
