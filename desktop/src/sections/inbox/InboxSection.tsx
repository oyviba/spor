import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import type { InboxGithub, InboxTask } from "../../types";
import { usePoll, GITHUB_POLL_MS } from "../../shared/hooks";
import { EmptyState, PageHeader, ScopeHeader } from "../../shared/Layout";
import { Bot, CircleX, PullRequest, Refresh } from "../../shared/Icons";
import { ago } from "../agents/transcript";
import { GROUPS, inboxItems, type InboxItem, type InboxKind } from "./inbox";

/** How often GitHub is asked; the Inbox asks again whenever it's opened. */

export interface Inbox {
  items: InboxItem[];
  errors: string[];
  /** Ask GitHub again now. */
  refresh: () => void;
  /** Every open agent task in those repositories, for the Agents view;
   * null until the first read. */
  tasks: InboxTask[] | null;
  /** Work's record of when each task was last looked at, by task id. */
  seen: Record<string, number>;
}

/** Work's record of when each task was last looked at, by task id (Work
 * keys it by work item: `task:<id>`). */
export function seenTasks(): Record<string, number> {
  try {
    const byItem: Record<string, number> = JSON.parse(localStorage.getItem("spor.work.seen") ?? "{}") ?? {};
    return Object.fromEntries(
      Object.entries(byItem)
        .filter(([key]) => key.startsWith("task:"))
        .map(([key, at]) => [key.slice("task:".length), at]),
    );
  } catch {
    return {};
  }
}

/** What's waiting for you in every repository in `roots`. Local tasks are
 * polled often (no network); GitHub every couple of minutes. */
export function useInbox(roots: string[], active: boolean): Inbox {
  const [tasks, setTasks] = useState<InboxTask[] | null>(null);
  const [github, setGithub] = useState<InboxGithub | null>(null);
  const [asked, setAsked] = useState(0);
  // Not reordered by switching repositories, so that doesn't ask GitHub again.
  const key = [...roots].sort().join("\n");
  usePoll(async () => setTasks(await api.inboxTasks(roots)), 3000, [key], active);
  usePoll(async () => setGithub(await api.inboxGithub(roots)), GITHUB_POLL_MS, [key, asked], active, true);
  const seen = seenTasks();
  return {
    items: inboxItems(tasks ?? [], github, seen),
    errors: github?.errors ?? [],
    refresh: useCallback(() => setAsked((n) => n + 1), []),
    tasks,
    seen,
  };
}

const ICONS: Record<InboxKind, React.ReactNode> = {
  ask: <Bot size={12} />,
  ready: <Bot size={12} />,
  failed: <CircleX size={12} />,
  review: <PullRequest size={12} />,
};

const repoName = (root: string) => root.split("/").pop() ?? root;

/** One list of what needs you, across every repository (#42). A row opens
 * the thing in its section and repository, and goes away once it's dealt
 * with. */
export function InboxSection({ inbox, onOpen }: { inbox: Inbox; onOpen: (item: InboxItem) => void }) {
  const { items, errors, refresh } = inbox;
  // Opening the Inbox is a good moment to ask GitHub again.
  useEffect(refresh, []); // eslint-disable-line react-hooks/exhaustive-deps
  const now = Date.now();
  const repos = new Set(items.map((i) => i.repo));
  return (
    <>
      <aside className="sidebar">
        <ScopeHeader title="Inbox" scope="Every repository, every workspace" />
        <div className="task-list">
          {items.length === 0 && <div className="sidebar-empty">Nothing needs you.</div>}
          {GROUPS.map((g) => {
            const rows = items.filter((i) => i.kind === g.kind);
            if (rows.length === 0) return null;
            return (
              <div key={g.kind}>
                <div className={`list-label${g.kind === "ask" ? " needs-you" : ""}`}>
                  <span>{g.label}</span>
                  <span className="num">{rows.length}</span>
                </div>
                {rows.map((i) => (
                  <button key={i.key} className="task-row" onClick={() => onOpen(i)} title={`${i.title} — ${i.repo}`}>
                    <div className="task-row-line">
                      <span className="task-row-title">{i.title}</span>
                      <span className="task-row-time num">{ago(i.since, now)}</span>
                    </div>
                    <div className="task-row-line sub">
                      {ICONS[i.kind]}
                      <span className="truncate">
                        {repoName(i.repo)} · {i.detail}
                      </span>
                    </div>
                  </button>
                ))}
              </div>
            );
          })}
        </div>
      </aside>
      <main className="main">
        <PageHeader title="Inbox">
          <button className="ghost small" onClick={refresh} title="Ask GitHub again">
            <Refresh size={12} /> Refresh
          </button>
        </PageHeader>
        <EmptyState
          title={
            items.length === 0
              ? "Nothing needs you"
              : `${items.length} ${items.length === 1 ? "thing needs" : "things need"} you in ${repos.size} ${repos.size === 1 ? "repository" : "repositories"}`
          }
        >
          <p>
            Agents waiting on an answer, finished agent work you haven't looked at, failed runs on your branches and
            pull requests waiting for your review, in every repository of every workspace. Click one to open it.
          </p>
          {errors.map((e) => (
            <p key={e} className="muted small-text">
              {e}
            </p>
          ))}
        </EmptyState>
      </main>
    </>
  );
}
