import type { WorkItem, Shelf, Filter } from "./items";
import { RulesPanel } from "./RulesPanel";
import { AutomationsPanel } from "./AutomationsPanel";
import { unseen } from "./items";
import { isOpen, statusOf } from "../agents/status";
import { doing } from "../agents/progress";
import { ago, tokens } from "../agents/transcript";
import { pullStatus } from "./pull";
import { useNow } from "../../shared/hooks";
import { useShell } from "../../shared/app";
import { StatusPill, type Status } from "../../shared/StatusPill";
import { AgentIcon, Archive, Branch, Chevron, Plus, PullRequest, Users } from "../../shared/Icons";
import { SidebarHeader } from "../../shared/Layout";

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "mine", label: "Mine" },
  { id: "agents", label: "Agents" },
  { id: "teammates", label: "Teammates" },
];

interface Props {
  shelves: Record<Shelf, WorkItem[]>;
  /** Anything at all, before the filter. */
  any: boolean;
  filter: Filter;
  onFilter: (f: Filter) => void;
  selected: string | null;
  seen: Record<string, number>;
  settledOpen: boolean;
  onSettledOpen: (open: boolean) => void;
  onSelect: (item: WorkItem) => void;
  onMove: (item: WorkItem, top: boolean) => void;
  /** The New… menu, at the button. */
  onNew: (at: { clientX: number; clientY: number }) => void;
  /** Delete the run checkpoints of tasks that are done, and discarded tasks' transcripts. */
  onCleanUp: () => void;
  /** What every agent task here has cost, and the tokens they used. */
  spent?: { usd: number; tokens: number };
}

/** Every work item in the repository, in shelves: what needs you, what's
 * in progress, what's in review, and (folded) what's settled. */
export function Sidebar(p: Props) {
  const shell = useShell();
  const all = Object.values(p.shelves).flat();
  const anyRunning = all.some((i) => i.kind === "task" && i.task.run?.state === "running");
  const now = useNow(1000, anyRunning);
  const nowMinute = useNow(30_000);
  const at = Math.max(now, nowMinute);

  // One pill per row: a task's own status; a PR's while it needs you or
  // has news; otherwise the row is quiet and shows its time.
  const statusFor = (i: WorkItem): Status | null => {
    if (i.kind === "task") return statusOf(i.task, p.seen[i.key], now, i.pull);
    if (i.pull && (i.shelf === "needs" || unseen(i, p.seen))) return pullStatus(i.pull);
    if (i.kind === "branch" && i.branch.current) return { label: "Checked out", tone: "muted", live: false };
    return null;
  };

  const row = (i: WorkItem) => {
    const status = statusFor(i);
    const time = i.kind === "task" ? ago(i.task.run?.ended ?? i.task.run?.started ?? i.task.created, at) : ago(i.updated, at);
    const stats =
      i.kind === "task"
        ? isOpen(i.task) && i.task.stats.files > 0 && i.task.stats
        : i.kind === "branch"
          ? i.branch.files > 0 && i.branch
          : i.pull;
    return (
      <button
        key={i.key}
        className={`task-row${p.selected === i.key ? " selected" : ""}`}
        data-kind={i.kind}
        onClick={() => p.onSelect(i)}
        onContextMenu={(e) =>
          shell.menu(e, [
            { label: "Move to top", run: () => p.onMove(i, true) },
            { label: "Move to bottom", run: () => p.onMove(i, false) },
          ])
        }
        title={i.kind === "task" ? i.task.prompt : i.title}
      >
        <div className="task-row-line">
          <span className="task-row-title">{i.title}</span>
          {status ? <StatusPill status={status} /> : <span className="task-row-time num">{time}</span>}
        </div>
        <div className="task-row-line sub">
          {i.kind === "pull" ? <PullRequest size={11} className="shrink-0" /> : <Branch size={11} className="shrink-0" />}
          <span className="truncate">
            {i.kind === "task" && i.task.ask
              ? capitalize(i.task.ask)
              : i.kind === "task" && i.task.run?.state === "running" && doing(i.task.progress)
              ? doing(i.task.progress)
              : i.kind === "task"
              ? i.task.landed
                ? `landed on ${i.task.base}`
                : i.task.closed
                  ? "pull request closed"
                  : i.task.branch
              : i.kind === "branch"
                ? i.branch.name
                : `#${i.pull.number} · ${i.pull.author}`}
          </span>
          <span className="flex-1" />
          {stats && (
            <span className="diffstat num">
              <span className="add">+{stats.additions}</span> <span className="del">−{stats.deletions}</span>
            </span>
          )}
          {i.owner === "agent" && i.kind === "task" ? (
            <AgentIcon agent={i.task.run?.agent ?? null} size={12} />
          ) : i.owner === "teammate" ? (
            <span title="A teammate's"><Users size={12} /></span>
          ) : null}
        </div>
      </button>
    );
  };

  const label = (text: string, items: WorkItem[], className = "") =>
    items.length > 0 && (
      <>
        <div className={`list-label${className}`}>{text}</div>
        {items.map(row)}
      </>
    );

  return (
    <aside className="sidebar">
      <SidebarHeader>
        <button
          className="icon-button new-work"
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            p.onNew({ clientX: r.left, clientY: r.bottom + 4 });
          }}
          title="New… (⌘N)"
          aria-label="New…"
        >
          <Plus />
        </button>
      </SidebarHeader>
      <div className="segmented">
        {FILTERS.map((f) => (
          <button key={f.id} aria-pressed={p.filter === f.id} onClick={() => p.onFilter(f.id)}>
            {f.label}
          </button>
        ))}
      </div>
      <div className="task-list">
        {!p.any ? (
          <div className="sidebar-empty">
            Nothing in flight. Start a branch, an agent task or work on an issue with ⌘N.
          </div>
        ) : (
          all.length === 0 && <div className="sidebar-empty">Nothing here with this filter.</div>
        )}
        {label("Needs you", p.shelves.needs, " needs-you")}
        {label("In progress", p.shelves.progress)}
        {label("In review", p.shelves.review)}
        {p.shelves.settled.length > 0 && (
          <div className="shelves">
            <div className="shelf">
              <button className="shelf-header" onClick={() => p.onSettledOpen(!p.settledOpen)} aria-expanded={p.settledOpen}>
                <span>Settled</span>
                <span className="flex-1" />
                <span className="num">{p.shelves.settled.length}</span>
                <Chevron size={12} className={`chev${p.settledOpen ? " open" : ""}`} />
              </button>
              {p.settledOpen && p.shelves.settled.map(row)}
              {p.settledOpen && (
                <button
                  className="link small-text shelf-action"
                  onClick={p.onCleanUp}
                  title="Delete the run checkpoints of tasks that are done, and discarded tasks' transcripts. Undo can't bring them back afterwards. Opening the repository does this too."
                >
                  <Archive size={12} />
                  Clean up old tasks
                </button>
              )}
            </div>
          </div>
        )}
      </div>
      <AutomationsPanel />
      <RulesPanel />
      {p.spent && p.spent.usd > 0 && (
        <div className="sidebar-foot num" title="What the agents' runs on this repository's tasks have cost so far">
          Agents have spent ${p.spent.usd.toFixed(2)} · {tokens(p.spent.tokens)} tokens
        </div>
      )}
    </aside>
  );
}

const capitalize = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
