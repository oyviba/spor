// Every agent task in every repository of every workspace, in one list:
// what each is doing. A click opens its conversation right here, to answer
// or follow up on, whichever repository it's in. Built on the Inbox's poll of
// the tasks, so the list reads nothing of its own.

import type { ReactNode } from "react";
import { EmptyState, PageHeader, ScopeHeader } from "../../shared/Layout";
import { useStored } from "../../shared/hooks";
import { StatusPill } from "../../shared/StatusPill";
import { Folder } from "../../shared/Icons";
import { ago } from "./transcript";
import type { Inbox } from "../inbox/InboxSection";
import { STATES, agentGroups, countsByRepo, type AgentState } from "./overview";

const BY = [
  { id: "state", label: "By status" },
  { id: "repo", label: "By repository" },
] as const;

const repoName = (root: string) => root.split("/").pop() ?? root;
const tone = (state: AgentState) => STATES.find((s) => s.state === state)!.tone;

export function AgentsSection({
  inbox,
  selected,
  page,
  onOpen,
  onClose,
  onOpenRepo,
}: {
  inbox: Inbox;
  /** The task open on the right. */
  selected: { repo: string; taskId: string } | null;
  /** Its page (null while its repository opens); replaces the overview. */
  page: ReactNode;
  /** Open a task's conversation on the right. */
  onOpen: (repo: string, taskId: string) => void;
  /** Back to the overview. */
  onClose: () => void;
  /** Open a repository's Work. */
  onOpenRepo: (repo: string) => void;
}) {
  const [by, setBy] = useStored<"state" | "repo">("agents.by", "state");
  const { tasks, seen } = inbox;
  const groups = agentGroups(tasks ?? [], seen, by);
  const counts = countsByRepo(tasks ?? [], seen);
  const now = Date.now();
  // As the list counts them: one asking is waiting on you, not working.
  const working = counts.reduce((n, c) => n + c.counts.working, 0);
  return (
    <>
      <aside className="sidebar">
        <ScopeHeader title="Agents" scope="Every repository, every workspace" />
        {selected && (
          <button className="ghost small" onClick={onClose}>
            ← Overview
          </button>
        )}
        <div className="segmented">
          {BY.map((b) => (
            <button key={b.id} aria-pressed={by === b.id} onClick={() => setBy(b.id)}>
              {b.label}
            </button>
          ))}
        </div>
        <div className="task-list">
          {tasks != null && tasks.length === 0 && (
            <div className="sidebar-empty">No agent tasks in any repository. Start one from a repository's Work with ⌘N.</div>
          )}
          {groups.map((g) => (
            <div key={g.key}>
              <div className={`list-label${g.key === "asking" ? " needs-you" : ""}`} title={by === "repo" ? g.key : undefined}>
                <span>{g.label}</span>
                <span className="num">{g.rows.length}</span>
              </div>
              {g.rows.map((r) => {
                const label = by === "state" ? r.detail : STATES.find((s) => s.state === r.state)!.label;
                return (
                  <button
                    key={`${r.task.repo}:${r.task.id}`}
                    className={`task-row${selected?.repo === r.task.repo && selected.taskId === r.task.id ? " selected" : ""}`}
                    onClick={() => onOpen(r.task.repo, r.task.id)}
                    title={`${r.task.title} — ${r.task.repo}`}
                  >
                    <div className="task-row-line">
                      <span className="task-row-title">{r.task.title}</span>
                      {by === "repo" ? (
                        <StatusPill status={{ label, tone: tone(r.state), live: r.state === "working" }} />
                      ) : (
                        <span className="task-row-time num">{ago(r.at, now)}</span>
                      )}
                    </div>
                    <div className="task-row-line sub">
                      <span className="truncate">
                        {by === "state" ? `${repoName(r.task.repo)} · ${r.detail}` : `${r.detail} · ${ago(r.at, now)}`}
                      </span>
                    </div>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      </aside>
      {selected ? (
        page
      ) : (
      <main className="main">
        <PageHeader title="Agents" />
        <EmptyState
          title={
            tasks == null
              ? "Looking for agents…"
              : tasks.length === 0
                ? "No agents"
                : `${tasks.length} ${tasks.length === 1 ? "agent task" : "agent tasks"} in ${counts.length} ${counts.length === 1 ? "repository" : "repositories"}${working > 0 ? `, ${working} working` : ""}`
          }
        >
          <p>Every agent task in every repository of every workspace. Click one to open its conversation here.</p>
          {counts.length > 0 && (
            <table className="agents-overview">
              <thead>
                <tr>
                  <th>Repository</th>
                  {STATES.map((s) => (
                    <th key={s.state} className="num">
                      {s.label}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {counts.map((c) => (
                  <tr key={c.repo} onClick={() => onOpenRepo(c.repo)} title={`${c.repo} — open its Work`}>
                    <td>
                      <Folder size={12} /> {repoName(c.repo)}
                    </td>
                    {STATES.map((s) => (
                      <td key={s.state} className={`num${c.counts[s.state] > 0 ? ` tone-${s.tone}` : " muted"}`}>
                        {c.counts[s.state] || "–"}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </EmptyState>
      </main>
      )}
    </>
  );
}
