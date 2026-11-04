import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api";
import type { BranchInfo, CommitDetail, Graph as GraphData } from "../../types";
import { useShell } from "../../shared/app";
import { useNow, useStored } from "../../shared/hooks";
import { DetailsCard, EmptyState, PageHeader, SidebarHeader, Tabs } from "../../shared/Layout";
import { DiffView } from "../../shared/DiffView";
import { parsePatch } from "../../shared/diff";
import { Bot, Branch, Chevron, Commit, Copy, Merge, Search, Trash, Undo } from "../../shared/Icons";
import { Graph, type GraphHandle } from "./Graph";

const PAGE = 2000;

/** History: every branch's commits as a graph, a commit's details and
 * changes, and what can be done to a commit or a branch. */
export function HistorySection() {
  const shell = useShell();
  const { state, fail } = shell;
  const [graph, setGraph] = useState<GraphData | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<"graph" | "commit">("graph");
  const [commit, setCommit] = useState<CommitDetail | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Set<string> | null>(null);
  const [shelves, setShelves] = useStored("history.shelves", { agent: true, remote: false });
  const [loadMs, setLoadMs] = useState<number | null>(null);
  const graphRef = useRef<GraphHandle>(null);
  const now = useNow(60_000);

  // Reload when any ref moves (commit, checkout, fetch, agent commits…).
  useEffect(() => {
    const t0 = performance.now();
    Promise.all([api.graph(limit), api.branches()])
      .then(([g, b]) => {
        setGraph(g);
        setBranches(b);
        setLoadMs(Math.round(performance.now() - t0));
      })
      .catch(fail);
  }, [state?.refs, limit]); // eslint-disable-line react-hooks/exhaustive-deps

  // Sent here from elsewhere: a branch (an agent task's) or a commit.
  useEffect(() => {
    const t = shell.target;
    if (t.section !== "history" || !graph) return;
    const hash =
      t.hash ?? (t.branch ? graph.rows.find((r) => r.refs.some((x) => x === t.branch))?.hash : undefined);
    if (hash) {
      setSelected(hash);
      setTab("graph");
      requestAnimationFrame(() => graphRef.current?.scrollTo(hash));
    }
  }, [shell.target.seq, !!graph]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    setCommit(null);
    if (selected) api.commitDetail(selected).then(setCommit, fail);
  }, [selected]); // eslint-disable-line react-hooks/exhaustive-deps

  // Search: git's own (messages, authors, hashes), debounced.
  useEffect(() => {
    const q = query.trim();
    if (!q) return setHits(null);
    const id = setTimeout(() => api.searchCommits(q).then((h) => setHits(new Set(h)), fail), 250);
    return () => clearTimeout(id);
  }, [query]); // eslint-disable-line react-hooks/exhaustive-deps

  const current = state?.branch ?? shell.repo.branch;
  const tipOf = (name: string) => graph?.rows.find((r) => r.refs.includes(name))?.hash;
  const showBranch = (name: string) => {
    const hash = tipOf(name);
    if (!hash) return shell.toast(`${name} is further back than the graph shows`, "error");
    setSelected(hash);
    setTab("graph");
    graphRef.current?.scrollTo(hash);
  };
  const nextHit = () => {
    if (!hits || !graph) return;
    const rows = graph.rows;
    const from = selected ? rows.findIndex((r) => r.hash === selected) : -1;
    const next = rows.slice(from + 1).find((r) => hits.has(r.hash)) ?? rows.find((r) => hits.has(r.hash));
    if (next) {
      setSelected(next.hash);
      graphRef.current?.scrollTo(next.hash);
    }
  };

  const local = branches.filter((b) => !b.remote && !b.agent);
  const agent = branches.filter((b) => b.agent);
  const remote = branches.filter((b) => b.remote);
  const remotes = useMemo(() => new Set(remote.map((b) => b.name)), [branches]); // eslint-disable-line react-hooks/exhaustive-deps

  const branchRow = (b: BranchInfo) => (
    <div key={b.name} className={`branch-row${b.current ? " current" : ""}`}>
      <button className="branch-name" onClick={() => showBranch(b.name)} title={`Show ${b.name}`}>
        {b.agent ? <Bot size={12} /> : <Branch size={12} />}
        <span className="truncate">{b.name}</span>
      </button>
      {!b.current && (
        <span className="row-actions">
          <button
            className="icon-button tiny"
            title={`Check out ${b.name}`}
            onClick={() => shell.switchBranch((stash) => api.checkout(b.name, stash))}
          >
            <Undo size={11} style={{ transform: "scaleX(-1)" }} />
          </button>
          <button
            className="icon-button tiny"
            title={`Merge ${b.name} into ${current ?? "HEAD"}`}
            onClick={() => shell.act(() => api.mergeBranch(b.name))}
          >
            <Merge size={11} />
          </button>
          {!b.remote && (
            <button
              className="icon-button tiny"
              title={`Delete ${b.name} (⌘Z undoes)`}
              onClick={() => shell.act(() => api.deleteBranch(b.name))}
            >
              <Trash size={11} />
            </button>
          )}
        </span>
      )}
    </div>
  );

  return (
    <>
      <aside className="sidebar">
        <SidebarHeader />
        <div className="sidebar-search">
          <Search size={13} />
          <input
            value={query}
            placeholder="Search commits"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") nextHit();
              if (e.key === "Escape") setQuery("");
            }}
          />
          {hits && <span className="num muted small-text">{hits.size}</span>}
        </div>
        <div className="task-list">
          <div className="list-label">Branches</div>
          {local.map(branchRow)}
          <Shelf
            label="Agent branches"
            count={agent.length}
            open={shelves.agent}
            onToggle={() => setShelves({ ...shelves, agent: !shelves.agent })}
          >
            {agent.map(branchRow)}
          </Shelf>
          <Shelf
            label="Remote"
            count={remote.length}
            open={shelves.remote}
            onToggle={() => setShelves({ ...shelves, remote: !shelves.remote })}
          >
            {remote.map(branchRow)}
          </Shelf>
        </div>
      </aside>
      <main className="main">
        <div className="task-page">
          <PageHeader
            title={
              <>
                <h2>History</h2>
                {graph && (
                  <span className="muted small-text num" title="Time to load and lay out the graph">
                    {graph.rows.length.toLocaleString()} commits{loadMs != null && ` · ${loadMs} ms`}
                  </span>
                )}
              </>
            }
          >
            <Tabs
              value={tab}
              onChange={setTab}
              tabs={[
                { id: "graph", label: "Graph" },
                {
                  id: "commit",
                  label: commit ? (
                    <>
                      <Commit size={12} /> <span className="mono">{commit.hash.slice(0, 7)}</span>
                    </>
                  ) : (
                    "Commit"
                  ),
                },
              ]}
            />
          </PageHeader>
          <div className="task-body">
            {!graph ? (
              <div className="boot" />
            ) : graph.rows.length === 0 ? (
              <EmptyState title="No commits yet">Make the first one in Changes.</EmptyState>
            ) : tab === "graph" ? (
              <div className="graph-wrap">
                <Graph
                  ref={graphRef}
                  graph={graph}
                  selected={selected}
                  onSelect={setSelected}
                  onOpen={(h) => {
                    setSelected(h);
                    setTab("commit");
                  }}
                  hits={hits}
                  currentBranch={current}
                  remotes={remotes}
                  nowMs={now}
                />
                {graph.more && (
                  <button className="link load-more" onClick={() => setLimit((l) => l + PAGE)}>
                    Load {PAGE.toLocaleString()} more commits
                  </button>
                )}
              </div>
            ) : (
              <div className="changes">
                <div className="changes-column">
                  {commit ? (
                    <DiffView patch={commit.patch} empty="This commit changes no files." />
                  ) : (
                    <p className="hint">Select a commit.</p>
                  )}
                </div>
              </div>
            )}
            {commit && <CommitCard commit={commit} current={current} onSelect={setSelected} onOpen={() => setTab("commit")} />}
          </div>
        </div>
      </main>
    </>
  );
}

function Shelf({
  label,
  count,
  open,
  onToggle,
  children,
}: {
  label: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  if (count === 0) return null;
  return (
    <div className="shelf">
      <button className="shelf-header" onClick={onToggle} aria-expanded={open}>
        <span>{label}</span>
        <span className="flex-1" />
        <span className="num">{count}</span>
        <Chevron size={12} className={`chev${open ? " open" : ""}`} />
      </button>
      {open && children}
    </div>
  );
}

function CommitCard({
  commit,
  current,
  onSelect,
  onOpen,
}: {
  commit: CommitDetail;
  current: string | null;
  onSelect: (hash: string) => void;
  onOpen: () => void;
}) {
  const shell = useShell();
  const [naming, setNaming] = useState(false);
  const [confirmRevert, setConfirmRevert] = useState(false);
  useEffect(() => {
    setNaming(false);
    setConfirmRevert(false);
  }, [commit.hash]);
  const [subject, ...body] = commit.message.split("\n");
  const files = parsePatch(commit.patch);
  const short = commit.hash.slice(0, 7);
  return (
    <DetailsCard>
      <div className="inspector-section card-text">
        <div className="card-title">{subject}</div>
        {body.join("\n").trim() && <div className="card-body">{body.join("\n").trim()}</div>}
        <div className="muted small-text">
          {commit.author} · {commit.date}
        </div>
      </div>
      <div className="inspector-section">
        <button className="inspector-row action" onClick={() => navigator.clipboard.writeText(commit.hash).then(() => shell.toast("Hash copied"))}>
          <Copy size={13} />
          <span className="mono">{short}</span>
          {commit.parents.length > 0 && (
            <span className="muted">
              {" "}
              ← {commit.parents.map((p) => (
                <a
                  key={p}
                  className="mono parent-link"
                  onClick={(e) => {
                    e.stopPropagation();
                    onSelect(p);
                  }}
                >
                  {p.slice(0, 7)}{" "}
                </a>
              ))}
            </span>
          )}
        </button>
        <button className="inspector-row action" onClick={onOpen}>
          <span className="diffstat num">
            <span className="add">+{files.reduce((n, f) => n + f.additions, 0)}</span>{" "}
            <span className="del">−{files.reduce((n, f) => n + f.deletions, 0)}</span>
          </span>
          <span>
            {files.length} {files.length === 1 ? "file" : "files"} <span className="muted">· view changes</span>
          </span>
        </button>
      </div>
      <div className="inspector-actions">
        {naming ? (
          <form
            className="inline-form"
            onSubmit={(e) => {
              e.preventDefault();
              const name = (new FormData(e.currentTarget).get("name") as string).trim();
              if (name) {
                setNaming(false);
                shell.switchBranch((stash) => api.createBranch(name, commit.hash, stash));
              }
            }}
          >
            <input name="name" autoFocus placeholder="New branch name" onKeyDown={(e) => e.key === "Escape" && setNaming(false)} />
            <button className="primary small" type="submit">
              Create
            </button>
          </form>
        ) : (
          <>
            <button className="ghost" onClick={() => setNaming(true)}>
              <Branch size={12} /> Branch here
            </button>
            <button className="ghost" onClick={() => shell.act(() => api.cherryPick(commit.hash))} title={`Apply ${short} on ${current ?? "HEAD"}`}>
              Cherry-pick
            </button>
            <button
              className={confirmRevert ? "danger" : "ghost"}
              onClick={() => (confirmRevert ? shell.act(() => api.revert(commit.hash)) : setConfirmRevert(true))}
              onBlur={() => setConfirmRevert(false)}
              title={`A new commit on ${current ?? "HEAD"} that undoes ${short}`}
            >
              {confirmRevert ? "Revert?" : "Revert"}
            </button>
          </>
        )}
      </div>
    </DetailsCard>
  );
}
