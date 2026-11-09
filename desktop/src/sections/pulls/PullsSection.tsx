import { useRef, useState } from "react";
import { api, errorText } from "../../api";
import type { Pull } from "../../types";
import { useShell } from "../../shared/app";
import { useChanged, usePoll, useStored, GITHUB_POLL_MS } from "../../shared/hooks";
import { EmptyState, ListLoading, SidebarHeader } from "../../shared/Layout";
import { StatusPill } from "../../shared/StatusPill";
import { ago } from "../agents/transcript";
import { PullView, pullStatus } from "../work/pull";

type Filter = "review" | "yours" | "open" | "closed";
const FILTERS: { id: Filter; label: string }[] = [
  { id: "review", label: "Review" },
  { id: "yours", label: "Yours" },
  { id: "open", label: "Open" },
  { id: "closed", label: "Closed" },
];

/** Pull requests on GitHub, through `gh`: a browser for them. The one
 * picked opens on the work page (#80), as it would in Work. */
export function PullsSection() {
  const shell = useShell();
  const [filter, setFilter] = useStored<Filter>("pulls.filter", "open");
  const [pulls, setPulls] = useState<Pull[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useStored<number | null>("pulls.selected", null);

  // The filter shown: an answer for one picked before it is dropped.
  const wanted = useRef(filter);
  wanted.current = filter;
  usePoll(
    async () => {
      const asked = filter;
      try {
        const list = await api.pulls(asked);
        if (wanted.current !== asked) return;
        setPulls(list);
        setError(null);
      } catch (e) {
        if (wanted.current !== asked) return;
        setError(errorText(e));
        setPulls([]);
      }
    },
    GITHUB_POLL_MS,
    [filter],
    true,
    true,
  );
  useChanged(() => {
    if (shell.target.section === "pulls" && shell.target.number) setSelected(shell.target.number);
  }, [shell.target.seq]);

  return (
    <>
      <aside className="sidebar">
        <SidebarHeader />
        <div className="segmented">
          {FILTERS.map((f) => (
            <button
              key={f.id}
              aria-pressed={filter === f.id}
              onClick={() => {
                if (f.id === filter) return;
                setPulls(null);
                setFilter(f.id);
              }}
            >
              {f.label}
            </button>
          ))}
        </div>
        <div className="task-list">
          {error ? (
            <div className="sidebar-empty">{error}</div>
          ) : pulls == null ? (
            <ListLoading what="pull requests" />
          ) : pulls.length === 0 ? (
            <div className="sidebar-empty">No pull requests here.</div>
          ) : (
            pulls.map((p) => (
              <button
                key={p.number}
                className={`task-row${selected === p.number ? " selected" : ""}`}
                onClick={() => setSelected(p.number)}
                title={p.title}
              >
                <div className="task-row-line">
                  <span className="task-row-title">{p.title}</span>
                  <span className="task-row-time num">{ago(p.updated, Date.now())}</span>
                </div>
                <div className="task-row-line sub">
                  <span className="num">#{p.number}</span>
                  <span className="truncate">{p.author}</span>
                  <span className="flex-1" />
                  <StatusPill status={pullStatus(p)} />
                </div>
              </button>
            ))
          )}
        </div>
      </aside>
      <main className="main">
        {selected == null || error ? (
          <EmptyState title={error ? "Pull requests aren't available" : "Pick a pull request"}>{error}</EmptyState>
        ) : (
          <PullView number={selected} from="pulls" />
        )}
      </main>
    </>
  );
}

