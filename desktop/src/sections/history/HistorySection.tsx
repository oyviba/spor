import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api";
import type { BranchInfo, CommitDetail, Graph as GraphData, PrBadge, TagInfo, WorktreeInfo } from "../../types";
import { useShell } from "../../shared/app";
import { useChanged, useNow, usePoll, useStored } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { DetailsCard, EmptyState, PageHeader, SidebarHeader, Tabs } from "../../shared/Layout";
import { DiffView } from "../../shared/DiffView";
import { parsePatch } from "../../shared/diff";
import { Bot, Branch, Chevron, Commit, Copy, Folder, MessageSquare, Merge, Search, Tag, Trash, Undo } from "../../shared/Icons";
import type { MenuItem } from "../../shared/Overlay";
import type { Shell } from "../../shared/app";
import { BRANCH_DRAG, Graph, type GraphHandle } from "./Graph";
import { Rewrite } from "./Rewrite";
import { Blame } from "./Blame";
import { GraphOptionsButton } from "./GraphOptions";
import { GraphSearch } from "./GraphSearch";
import { searchTerms, stepHit } from "./search";

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
  const [tab, setTab] = useState<"graph" | "commit" | "rewrite" | "blame">("graph");
  const [blameOf, setBlameOf] = useState<{ path: string; rev: string | null } | null>(null);
  const [rewriteBase, setRewriteBase] = useState<string | null>(null);
  const [tags, setTags] = useState<TagInfo[]>([]);
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [prs, setPrs] = useState<Map<string, PrBadge>>(new Map());
  const [commit, setCommit] = useState<CommitDetail | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Set<string> | null>(null);
  const [searching, setSearching] = useState(false);
  const [matchesOnly, setMatchesOnly] = useStored("history.matchesOnly", true);
  // The sidebar's own filter, on the names of branches, tags and worktrees.
  const [filter, setFilter] = useState("");
  const searchInput = useRef<HTMLInputElement>(null);
  const [shelves, setShelves] = useStored("history.shelves", { agent: true, remote: false, tags: false, worktrees: false });
  const [loadMs, setLoadMs] = useState<number | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const graphRef = useRef<GraphHandle>(null);
  const now = useNow(60_000);

  // Reload when any ref moves (commit, checkout, fetch, agent commits…).
  useChanged(() => {
    const t0 = performance.now();
    Promise.all([api.graph(limit), api.branches()])
      .then(([g, b]) => {
        setGraph(g);
        setBranches(b);
        setLoadMs(Math.round(performance.now() - t0));
      })
      .catch(fail)
      .finally(() => setLoadingMore(false));
    api.tags().then(setTags, () => setTags([]));
    api.worktrees().then(setWorktrees, () => setWorktrees([]));
  }, [state?.refs, limit]);

  // PR badges on branch labels: one `gh` call, in the background.
  usePoll(
    () =>
      api.prBadges().then(
        (list) => setPrs(new Map(list.map((b) => [b.branch, b]))),
        () => undefined,
      ),
    60_000,
    [state?.refs],
  );

  // Sent here from elsewhere: a branch (an agent task's), a commit, or a
  // file to blame.
  useChanged(() => {
    const t = shell.target;
    if (t.section !== "history" || !graph) return;
    if (t.blame) {
      setBlameOf(t.blame);
      setTab("blame");
      return;
    }
    const hash =
      t.hash ?? (t.branch ? graph.rows.find((r) => r.refs.some((x) => x === t.branch))?.hash : undefined);
    if (hash) reveal(hash);
  }, [shell.target.seq, !!graph]);

  useChanged(() => {
    setCommit(null);
    if (selected) api.commitDetail(selected).then(setCommit, fail);
  }, [selected]);

  // Search: git's own (messages, authors, hashes) and the names of branches
  // and tags, debounced. A newer search drops an older one's answer.
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setHits(null);
      setSearching(false);
      return;
    }
    setSearching(true);
    let stale = false;
    const id = setTimeout(
      () =>
        api.searchCommits(q).then(
          (h) => {
            if (stale) return;
            setHits(new Set(h));
            setSearching(false);
          },
          (e) => {
            if (stale) return;
            setSearching(false);
            fail(e);
          },
        ),
      200,
    );
    return () => {
      stale = true;
      clearTimeout(id);
    };
  }, [query, state?.refs]); // eslint-disable-line react-hooks/exhaustive-deps

  // ⌘F, or / outside a text field, goes to the search.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      const typing = !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
      const find = (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "f";
      const slash = e.key === "/" && !e.metaKey && !e.ctrlKey && !e.altKey && !typing;
      if (!find && !slash) return;
      e.preventDefault();
      setTab("graph");
      requestAnimationFrame(() => {
        searchInput.current?.focus();
        searchInput.current?.select();
      });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Switching between the matches and the graph keeps the commit in view.
  useEffect(() => {
    if (selected) requestAnimationFrame(() => graphRef.current?.scrollTo(selected));
  }, [matchesOnly, hits === null]); // eslint-disable-line react-hooks/exhaustive-deps

  const current = state?.branch ?? shell.repo.branch;
  const tipOf = (name: string) => graph?.rows.find((r) => r.refs.includes(name))?.hash;
  /** Select a commit and bring it into view. A search that doesn't
   * include it gives way: it wouldn't be in the list. */
  const reveal = (hash: string) => {
    if (hits && matchesOnly && !hits.has(hash)) {
      setQuery("");
      setHits(null);
    }
    setSelected(hash);
    setTab("graph");
    requestAnimationFrame(() => graphRef.current?.scrollTo(hash));
  };
  const showBranch = (name: string) => {
    const hash = tipOf(name);
    if (!hash) return shell.toast(`${name} is further back than the graph shows`, "error");
    reveal(hash);
  };
  const showCommit = (hash: string) => {
    if (!graph?.rows.some((r) => r.hash === hash)) return shell.toast("That commit is further back than the graph shows", "error");
    reveal(hash);
  };
  const startRewrite = (hash: string) => {
    setRewriteBase(hash);
    setTab("rewrite");
  };
  const commitMenu = (hash: string): MenuItem[] => commitActions(shell, hash, current, startRewrite);

  // The search's hits among the commits the graph has loaded, as rows.
  const hitRows = useMemo(
    () => (hits && graph ? graph.rows.flatMap((r, i) => (hits.has(r.hash) ? [i] : [])) : []),
    [hits, graph],
  );
  const selectedRow = selected && graph ? graph.rows.findIndex((r) => r.hash === selected) : -1;
  const stepHits = (step: 1 | -1) => {
    const next = stepHit(hitRows, selectedRow, step);
    const row = next == null ? undefined : graph?.rows[next];
    if (row) {
      setSelected(row.hash);
      graphRef.current?.scrollTo(row.hash);
    }
  };
  const terms = useMemo(() => searchTerms(query), [query]);
  const hitStats = hits ? { total: hits.size, loaded: hitRows.length, at: hitRows.includes(selectedRow) ? hitRows.indexOf(selectedRow) : null } : null;

  // The default branch is pinned above the rest, and out of the filter's reach.
  const defaultName = graph?.main ?? null;
  const defaultBranch = branches.find((b) => !b.remote && b.name === defaultName);
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  const named = (name: string) => words.every((w) => name.toLowerCase().includes(w));
  const local = branches.filter((b) => !b.remote && !b.agent && b.name !== defaultName && named(b.name));
  const agent = branches.filter((b) => b.agent && named(b.name));
  const remote = branches.filter((b) => b.remote && named(b.name));
  const shownTags = tags.filter((t) => named(t.name));
  const shownWorktrees = worktrees.filter((w) => named(w.branch ?? w.path));
  const remotes = useMemo(() => new Set(remote.map((b) => b.name)), [branches]); // eslint-disable-line react-hooks/exhaustive-deps

  const branchMenu = (b: BranchInfo): MenuItem[] => {
    const into = current ?? "HEAD";
    const items: MenuItem[] = [{ label: `Show ${b.name}`, run: () => showBranch(b.name) }];
    if (!b.current) {
      items.push(
        { label: b.remote ? `Check out ${b.name} as a local branch` : `Check out ${b.name}`, run: () => shell.switchBranch((stash) => api.checkout(b.name, stash)) },
        { label: `Merge ${b.name} into ${into}`, run: () => shell.act(() => api.mergeBranch(b.name), { pending: `Merging ${b.name}…` }), separator: true },
        { label: `Rebase ${into} onto ${b.name}`, run: () => shell.act(() => api.rebaseOnto(b.name), { pending: `Rebasing onto ${b.name}…` }), disabled: !current },
      );
    }
    if (!b.remote) {
      items.push({
        label: "Rename…",
        separator: true,
        run: async () => {
          const r = await shell.prompt({ title: `Rename ${b.name}`, initial: b.name, action: "Rename" });
          if (r && r.value !== b.name) shell.act(() => api.renameBranch(b.name, r.value));
        },
      });
      if (!b.current) items.push({ label: `Delete ${b.name}`, danger: true, run: () => shell.act(() => api.deleteBranch(b.name)) });
    } else {
      items.push({
        label: `Delete ${b.name} from the remote…`,
        danger: true,
        separator: true,
        run: async () => {
          const ok = await shell.confirm({
            title: `Delete ${b.name} from the remote?`,
            detail: "Everyone using the remote loses the branch. This can't be undone from here.",
            action: "Delete from remote",
            danger: true,
          });
          if (ok) shell.act(() => api.deleteRemoteBranch(b.name), { undo: false, pending: `Deleting ${b.name}…` });
        },
      });
    }
    items.push({ label: "Copy name", separator: true, run: () => navigator.clipboard.writeText(b.name).then(() => shell.toast("Copied")) });
    return items;
  };

  /** A branch dropped on another: merge it there, rebase, or a PR between them. */
  const dropMenu = (source: string, target: string, e: { clientX: number; clientY: number }) => {
    const items: MenuItem[] = [
      {
        label: `Merge ${source} into ${target}`,
        run: () => shell.act(() => api.mergeInto(source, target), { pending: `Merging ${source} into ${target}…` }),
      },
    ];
    if (source === current)
      items.push({ label: `Rebase ${source} onto ${target}`, run: () => shell.act(() => api.rebaseOnto(target), { pending: `Rebasing onto ${target}…` }) });
    items.push({
      label: `Open pull request: ${source} → ${target}`,
      separator: true,
      run: () => api.pullRequestBetween(source, target).then(api.openUrl).catch(fail),
    });
    shell.menu(e, items);
  };
  const [dropOn, setDropOn] = useState<string | null>(null);

  const branchRow = (b: BranchInfo) => (
    <div
      key={b.name}
      className={`branch-row${b.current ? " current" : ""}${b.name === defaultName && !b.remote ? " default-branch" : ""}${dropOn === b.name ? " drop-target" : ""}`}
      onContextMenu={(e) => shell.menu(e, branchMenu(b))}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData(BRANCH_DRAG, b.name);
        e.dataTransfer.setData("text/plain", b.name);
        e.dataTransfer.effectAllowed = "move";
      }}
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes(BRANCH_DRAG)) return;
        e.preventDefault();
        setDropOn(b.name);
      }}
      onDragLeave={() => setDropOn((d) => (d === b.name ? null : d))}
      onDrop={(e) => {
        setDropOn(null);
        const source = e.dataTransfer.getData(BRANCH_DRAG);
        if (source && source !== b.name) {
          e.preventDefault();
          dropMenu(source, b.name, e);
        }
      }}
    >
      <button
        className="branch-name"
        onClick={() => showBranch(b.name)}
        onDoubleClick={() => !b.current && shell.switchBranch((stash) => api.checkout(b.name, stash))}
        title={`Show ${b.name} — double-click to check out, right-click for more`}
      >
        {b.agent ? <Bot size={12} /> : <Branch size={12} />}
        <span className="truncate">{b.name}</span>
        {prs.get(b.name) && <span className="pr-badge muted-badge">#{prs.get(b.name)!.number}</span>}
      </button>
      {!b.current && (
        <span className="row-actions">
          <Button
            className="icon-button tiny"
            title={`Check out ${b.name}`}
            onClick={() => shell.switchBranch((stash) => api.checkout(b.name, stash))}
          >
            <Undo size={11} style={{ transform: "scaleX(-1)" }} />
          </Button>
          <Button
            className="icon-button tiny"
            title={`Merge ${b.name} into ${current ?? "HEAD"}`}
            onClick={() => shell.act(() => api.mergeBranch(b.name), { pending: `Merging ${b.name}…` })}
          >
            <Merge size={11} />
          </Button>
          {!b.remote && (
            <Button
              className="icon-button tiny"
              title={`Delete ${b.name} (⌘Z undoes)`}
              onClick={() => shell.act(() => api.deleteBranch(b.name))}
            >
              <Trash size={11} />
            </Button>
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
            value={filter}
            placeholder="Filter branches and tags"
            title="Show the branches, tags and worktrees whose name has all of these words. Search the commits themselves above the graph (⌘F)."
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setFilter("")}
          />
        </div>
        <div className="task-list">
          {defaultBranch && (
            <>
              <div className="list-label">Default branch</div>
              {branchRow(defaultBranch)}
            </>
          )}
          {(local.length > 0 || !filter) && <div className="list-label">Branches</div>}
          {local.map(branchRow)}
          {filter && local.length + agent.length + remote.length + shownTags.length + shownWorktrees.length === 0 && (
            <p className="hint">Nothing is named like that.</p>
          )}
          <Shelf
            label="Agent branches"
            count={agent.length}
            open={shelves.agent || !!filter}
            onToggle={() => setShelves({ ...shelves, agent: !shelves.agent })}
          >
            {agent.map(branchRow)}
          </Shelf>
          <Shelf
            label="Remote"
            count={remote.length}
            open={shelves.remote || !!filter}
            onToggle={() => setShelves({ ...shelves, remote: !shelves.remote })}
          >
            {remote.map(branchRow)}
          </Shelf>
          <Shelf
            label="Tags"
            count={shownTags.length}
            open={shelves.tags || !!filter}
            onToggle={() => setShelves({ ...shelves, tags: !shelves.tags })}
          >
            {shownTags.map((t) => (
              <div key={t.name} className="branch-row" onContextMenu={(e) => shell.menu(e, tagMenu(shell, t, showCommit))}>
                <button className="branch-name" onClick={() => showCommit(t.hash)} title={`Show ${t.name} — right-click for more`}>
                  <Tag size={12} />
                  <span className="truncate">{t.name}</span>
                </button>
                <span className="row-actions">
                  <Button className="icon-button tiny" title={`Delete ${t.name} (⌘Z undoes)`} onClick={() => shell.act(() => api.deleteTag(t.name))}>
                    <Trash size={11} />
                  </Button>
                </span>
              </div>
            ))}
          </Shelf>
          <Shelf
            label="Worktrees"
            count={worktrees.length > 1 ? shownWorktrees.length : 0}
            open={shelves.worktrees || !!filter}
            onToggle={() => setShelves({ ...shelves, worktrees: !shelves.worktrees })}
          >
            {shownWorktrees.map((w) => (
              <div
                key={w.path}
                className={`branch-row${w.current ? " current" : ""}`}
                onContextMenu={(e) =>
                  shell.menu(e, [
                    { label: "Open in Spor", run: () => shell.openRepo(w.path), disabled: w.current || w.prunable },
                    ...(w.branch ? [{ label: `Show ${w.branch}`, run: () => showBranch(w.branch!) }] : []),
                    { label: "Copy path", run: () => navigator.clipboard.writeText(w.path).then(() => shell.toast("Copied")) },
                    {
                      label: "Remove worktree",
                      danger: true,
                      separator: true,
                      disabled: w.main || w.current || w.agent,
                      run: () => shell.act(() => api.removeWorktree(w.path), { undo: false, pending: "Removing the worktree…" }),
                    },
                  ])
                }
              >
                <button
                  className="branch-name"
                  onClick={() => (w.current ? undefined : shell.openRepo(w.path))}
                  title={`${w.path}${w.agent ? " — an agent task's folder" : ""}${w.current ? " (open here)" : " — click to open"}`}
                >
                  {w.agent ? <Bot size={12} /> : <Folder size={12} />}
                  <span className="truncate">{w.branch ?? `detached ${w.head.slice(0, 7)}`}</span>
                  <span className="muted small-text truncate">{w.main ? "main" : w.path.split("/").pop()}</span>
                </button>
              </div>
            ))}
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
            {tab === "graph" && <GraphOptionsButton />}
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
                ...(rewriteBase ? [{ id: "rewrite" as const, label: "Rewrite" }] : []),
                ...(blameOf
                  ? [{ id: "blame" as const, label: <span className="truncate tab-file">Blame {blameOf.path.split("/").pop()}</span> }]
                  : []),
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
                <GraphSearch
                  query={query}
                  onQuery={(q) => {
                    setQuery(q);
                    if (!q) setHits(null);
                  }}
                  inputRef={searchInput}
                  hits={hitStats}
                  searching={searching}
                  matchesOnly={matchesOnly}
                  onMatchesOnly={setMatchesOnly}
                  onStep={stepHits}
                  onLoadMore={graph.more ? () => setLimit((l) => l + PAGE) : null}
                  defaultBranch={defaultName}
                  onDefault={() => defaultName && showBranch(defaultName)}
                />
                {hits && matchesOnly && hitRows.length === 0 ? (
                  <div className="graph-no-matches muted">
                    {searching ? "Searching…" : `No commit matches “${query.trim()}”.`}
                    {!searching && hits.size > 0 && " The matches are further back than the graph has loaded."}
                  </div>
                ) : (
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
                  flat={matchesOnly}
                  terms={terms}
                  currentBranch={current}
                  remotes={remotes}
                  nowMs={now}
                  prs={prs}
                  onContext={(hash, e) => shell.menu(e, commitMenu(hash))}
                  onDropBranch={(source, target, e) => dropMenu(source, target, e)}
                />
                )}
                {graph.more && !hits && (
                  <Button
                    className="link load-more"
                    busy={loadingMore}
                    onClick={() => {
                      setLoadingMore(true);
                      setLimit((l) => l + PAGE);
                    }}
                  >
                    Load {PAGE.toLocaleString()} more commits
                  </Button>
                )}
              </div>
            ) : tab === "blame" && blameOf ? (
              <div className="changes">
                <Blame
                  path={blameOf.path}
                  rev={blameOf.rev}
                  onShowCommit={showCommit}
                  onWhy={(taskId) => shell.go({ section: "work", taskId })}
                  onFileHistory={(path) => {
                    setQuery(`path:${path}`);
                    setTab("graph");
                  }}
                />
              </div>
            ) : tab === "rewrite" && rewriteBase ? (
              <div className="changes">
                <Rewrite
                  base={rewriteBase}
                  onCancel={() => {
                    setRewriteBase(null);
                    setTab("graph");
                  }}
                  onDone={() => {
                    setRewriteBase(null);
                    setTab("graph");
                  }}
                />
              </div>
            ) : (
              <div className="changes">
                <div className="changes-column">
                  {commit ? (
                    <DiffView
                      patch={commit.patch}
                      empty="This commit changes no files."
                      onBlame={(path) => {
                        setBlameOf({ path, rev: commit.hash });
                        setTab("blame");
                      }}
                    />
                  ) : (
                    <p className="hint">Select a commit.</p>
                  )}
                </div>
              </div>
            )}
            {commit && tab !== "rewrite" && tab !== "blame" && (
              <CommitCard
                commit={commit}
                current={current}
                onSelect={setSelected}
                onOpen={() => setTab("commit")}
                onMore={(e) => shell.menu(e, commitMenu(commit.hash))}
              />
            )}
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
  onMore,
}: {
  commit: CommitDetail;
  current: string | null;
  onSelect: (hash: string) => void;
  onOpen: () => void;
  onMore: (at: { clientX: number; clientY: number }) => void;
}) {
  const shell = useShell();
  const [naming, setNaming] = useState(false);
  const [confirmRevert, setConfirmRevert] = useState(false);
  useChanged(() => {
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
      {commit.task && (
        <div className="inspector-section" title={commit.task.prompt}>
          <button className="inspector-row action" onClick={() => shell.go({ section: "work", taskId: commit.task!.id })}>
            <MessageSquare size={13} />
            <span className="truncate">Made by the agent task “{commit.task.title}”</span>
            <span className="muted"> · Open conversation</span>
          </button>
        </div>
      )}
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
            <Button
              className="ghost"
              onClick={() => shell.act(() => api.cherryPick(commit.hash), { pending: `Cherry-picking ${short}…` })}
              title={`Apply ${short} on ${current ?? "HEAD"}`}
            >
              Cherry-pick
            </Button>
            <Button
              className={confirmRevert ? "danger" : "ghost"}
              onClick={() =>
                confirmRevert
                  ? shell.act(() => api.revert(commit.hash), { pending: `Reverting ${short}…` }).then(() => setConfirmRevert(false))
                  : setConfirmRevert(true)
              }
              onBlur={() => setConfirmRevert(false)}
              title={`A new commit on ${current ?? "HEAD"} that undoes ${short}`}
            >
              {confirmRevert ? "Revert?" : "Revert"}
            </Button>
            <button
              className="ghost"
              title="Tag, check out, rebase, reset, rewrite…"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                onMore({ clientX: r.left, clientY: r.bottom + 4 });
              }}
            >
              More…
            </button>
          </>
        )}
      </div>
    </DetailsCard>
  );
}

/** What can be done to a commit: the graph's right-click menu and the
 * commit card's More…. */
function commitActions(shell: Shell, hash: string, current: string | null, rewrite: (hash: string) => void): MenuItem[] {
  const short = hash.slice(0, 7);
  const on = current ?? "HEAD";
  const reset = (mode: "soft" | "mixed" | "hard") => async () => {
    if (mode === "hard") {
      const ok = await shell.confirm({
        title: `Reset ${on} to ${short} and discard your changes?`,
        detail: "Uncommitted changes are thrown away. ⌘Z brings back the commits, not the changes.",
        action: "Reset hard",
        danger: true,
      });
      if (!ok) return;
    }
    shell.act(() => api.resetTo(hash, mode), { pending: `Resetting to ${short}…` });
  };
  return [
    { label: "Copy hash", run: () => navigator.clipboard.writeText(hash).then(() => shell.toast("Hash copied")) },
    {
      label: "Create branch here…",
      separator: true,
      run: async () => {
        const r = await shell.prompt({ title: `New branch at ${short}`, placeholder: "Branch name", action: "Create and switch" });
        if (r) shell.switchBranch((stash) => api.createBranch(r.value, hash, stash));
      },
    },
    {
      label: "Tag here…",
      run: async () => {
        const r = await shell.prompt({
          title: `Tag ${short}`,
          placeholder: "Tag name, e.g. v1.2.0",
          action: "Tag",
          extra: { placeholder: "Message (optional — makes an annotated tag)" },
        });
        if (r) shell.act(() => api.createTag(r.value, hash, r.extra || null));
      },
    },
    { label: "Check out this commit (detached)", run: () => shell.switchBranch((stash) => api.checkoutCommit(hash, stash)) },
    { label: `Cherry-pick onto ${on}`, separator: true, run: () => shell.act(() => api.cherryPick(hash), { pending: `Cherry-picking ${short}…` }) },
    { label: `Revert on ${on}`, run: () => shell.act(() => api.revert(hash), { pending: `Reverting ${short}…` }) },
    { label: `Rebase ${on} onto ${short}`, run: () => shell.act(() => api.rebaseOnto(hash), { pending: `Rebasing onto ${short}…` }), disabled: !current },
    { label: `Rewrite ${on} after here…`, run: () => rewrite(hash), disabled: !current },
    { label: `Reset ${on} here — keep changes staged (soft)`, separator: true, run: reset("soft"), disabled: !current },
    { label: `Reset ${on} here — keep changes in files (mixed)`, run: reset("mixed"), disabled: !current },
    { label: `Reset ${on} here — discard changes (hard)…`, danger: true, run: reset("hard"), disabled: !current },
  ];
}

function tagMenu(shell: Shell, t: TagInfo, show: (hash: string) => void): MenuItem[] {
  return [
    { label: `Show ${t.name}`, run: () => show(t.hash) },
    { label: "Copy name", run: () => navigator.clipboard.writeText(t.name).then(() => shell.toast("Copied")) },
    { label: "Push to remote", separator: true, run: () => shell.act(() => api.pushTag(t.name), { undo: false, pending: `Pushing ${t.name}…` }) },
    { label: `Delete ${t.name}`, danger: true, separator: true, run: () => shell.act(() => api.deleteTag(t.name)) },
    {
      label: "Delete from remote too…",
      danger: true,
      run: async () => {
        const ok = await shell.confirm({
          title: `Delete ${t.name} from the remote?`,
          detail: "The local tag goes too (⌘Z brings that one back).",
          action: "Delete everywhere",
          danger: true,
        });
        if (ok) {
          await shell.act(() => api.pushTag(t.name, true), { undo: false, pending: `Deleting ${t.name} from the remote…` });
          shell.act(() => api.deleteTag(t.name));
        }
      },
    },
  ];
}
