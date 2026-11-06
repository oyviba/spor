import type { TaskPull, TaskSummary } from "../../types";
import { isOpen, statusOf } from "./status";
import { ago } from "./transcript";
import { useNow } from "../../shared/hooks";
import { StatusPill } from "../../shared/StatusPill";
import { AgentIcon, Branch, Chevron, Plus } from "../../shared/Icons";
import { SidebarHeader } from "../../shared/Layout";

interface Props {
  tasks: TaskSummary[];
  /** Open tasks' pull requests, by task id. */
  pulls: Record<string, TaskPull>;
  selected: string | null;
  seen: Record<string, number>;
  shelves: { working: boolean; settled: boolean };
  onShelves: (s: { working: boolean; settled: boolean }) => void;
  onSelect: (id: string) => void;
  onNew: () => void;
}

export function Sidebar(p: Props) {
  const anyRunning = p.tasks.some((t) => t.run?.state === "running");
  const now = useNow(1000, anyRunning);
  const nowMinute = useNow(30_000);
  // Active keeps a stable order (newest first): activity never moves a row.
  // Working and settled tasks sit in shelves at the bottom.
  // Waiting on you comes first, above everything (#21).
  const needsYou = p.tasks.filter((t) => isOpen(t) && t.asks > 0);
  const working = p.tasks.filter((t) => isOpen(t) && t.run?.state === "running" && t.asks === 0);
  const active = p.tasks.filter((t) => isOpen(t) && t.run?.state !== "running" && t.asks === 0);
  const settled = p.tasks.filter((t) => !isOpen(t));

  const row = (t: TaskSummary) => {
    const status = statusOf(t, p.seen[t.id], now, p.pulls[t.id]?.pull);
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
          <span className="truncate">{t.landed ? `landed on ${t.base}` : t.closed ? "pull request closed" : t.branch}</span>
          <span className="flex-1" />
          {isOpen(t) && t.stats.files > 0 && (
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
      <SidebarHeader>
        <button className="icon-button" onClick={p.onNew} title="New task (⌘N)" aria-label="New task">
          <Plus />
        </button>
      </SidebarHeader>
      <div className="task-list">
        {p.tasks.length === 0 && (
          <div className="sidebar-empty">No tasks yet. Give an agent its own branch and folder with ⌘N.</div>
        )}
        {needsYou.length > 0 && (
          <>
            <div className="list-label needs-you">Needs you</div>
            {needsYou.map(row)}
          </>
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
