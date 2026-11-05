import { useEffect, useRef, useState } from "react";
import { api, errorText } from "../../api";
import type { LineComment, Pull, PullDetail, TaskSummary } from "../../types";
import { useShell } from "../../shared/app";
import { usePoll, useStored } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { DetailsCard, EmptyState, ListLoading, PageHeader, SidebarHeader, Tabs } from "../../shared/Layout";
import { DiffView } from "../../shared/DiffView";
import { Markdown } from "../../shared/Markdown";
import { StatusPill, type Status } from "../../shared/StatusPill";
import { CiIcon } from "../../shared/ci";
import { ago } from "../agents/transcript";
import { Bot, Branch, Check, ChevronDown, External, MessageSquare, Play, X } from "../../shared/Icons";
import { choiceOf, useAgentSettings } from "../agents/Composer";

type Filter = "review" | "yours" | "open" | "closed";
const FILTERS: { id: Filter; label: string }[] = [
  { id: "review", label: "Review" },
  { id: "yours", label: "Yours" },
  { id: "open", label: "Open" },
  { id: "closed", label: "Closed" },
];

/** The one status a PR shows in lists: what's in the way, or where it is. */
export function pullStatus(p: Pull): Status {
  if (p.state === "merged") return { label: "Merged", tone: "violet", live: false };
  if (p.state === "closed") return { label: "Closed", tone: "muted", live: false };
  if (p.checks === "failing") return { label: "Checks failing", tone: "red", live: false };
  if (p.review === "changesrequested") return { label: "Changes requested", tone: "amber", live: false };
  if (p.state === "draft") return { label: "Draft", tone: "muted", live: false };
  if (p.checks === "pending") return { label: "Checks running", tone: "sky", live: true };
  if (p.review === "approved") return { label: "Approved", tone: "emerald", live: false };
  return { label: "Open", tone: "sky", live: false };
}

/** Pull requests on GitHub, through `gh`: read, review, merge. */
export function PullsSection() {
  const shell = useShell();
  const { fail } = shell;
  const [filter, setFilter] = useStored<Filter>("pulls.filter", "open");
  const [pulls, setPulls] = useState<Pull[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useStored<number | null>("pulls.selected", null);
  const [detail, setDetail] = useState<PullDetail | null>(null);
  const [tab, setTab] = useState<"conversation" | "files" | "checks">("conversation");
  const [diff, setDiff] = useState<string | null>(null);
  const [comments, setComments] = useState<LineComment[]>([]);
  const [tasks, setTasks] = useState<TaskSummary[]>([]);

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
    60_000,
    [filter],
  );
  useEffect(() => {
    api.listTasks().then(setTasks, () => {});
  }, []);
  useEffect(() => {
    if (shell.target.section === "pulls" && shell.target.number) setSelected(shell.target.number);
  }, [shell.target.seq]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadDetail = async () => {
    if (selected == null) return;
    try {
      setDetail(await api.pullDetail(selected));
    } catch (e) {
      fail(e);
    }
  };
  usePoll(loadDetail, 30_000, [selected], selected != null && !error);
  useEffect(() => {
    setDetail(null);
    setDiff(null);
    setComments([]);
    setTab("conversation");
  }, [selected]);
  useEffect(() => {
    if (tab === "files" && selected != null && diff == null) api.pullDiff(selected).then(setDiff, fail);
  }, [tab, selected]); // eslint-disable-line react-hooks/exhaustive-deps

  const task = detail ? tasks.find((t) => t.branch === detail.pull.head && !t.landed) : undefined;
  const pr = detail?.pull.number === selected ? detail : null;

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
        ) : !pr ? (
          <div className="boot" />
        ) : (
          <div className="task-page with-inspector">
            <PageHeader
              title={
                <>
                  <h2 title={pr.pull.title}>
                    {pr.pull.title} <span className="muted num">#{pr.pull.number}</span>
                  </h2>
                  <StatusPill status={pullStatus(pr.pull)} />
                </>
              }
            >
              <Tabs
                value={tab}
                onChange={setTab}
                tabs={[
                  { id: "conversation", label: "Conversation" },
                  {
                    id: "files",
                    label: (
                      <>
                        Files <span className="tab-count num">{pr.changed_files}</span>
                        {comments.length > 0 && (
                          <span className="tab-count accent num">
                            <MessageSquare size={10} /> {comments.length}
                          </span>
                        )}
                      </>
                    ),
                  },
                  { id: "checks", label: <>Checks <span className="tab-count num">{pr.checks.length}</span></> },
                ]}
              />
            </PageHeader>
            <div className="task-body">
              {tab === "conversation" ? (
                <div className="conversation">
                  <div className="column">
                    <Timeline pr={pr} />
                  </div>
                </div>
              ) : tab === "files" ? (
                <div className="changes">
                  <div className="changes-column">
                    <DiffView
                      patch={diff}
                      comments={comments}
                      onComment={(file, line, side, text) =>
                        setComments((c) => [...c, { id: `${Date.now()}`, file, line, side, text }])
                      }
                    />
                  </div>
                </div>
              ) : (
                <div className="conversation">
                  <div className="column">
                    <Checks pr={pr} />
                  </div>
                </div>
              )}
              <PullCard pr={pr} task={task} onChanged={loadDetail} />
            </div>
            {pr.pull.state !== "merged" && (
              <ReviewBox
                pr={pr}
                comments={comments}
                onRemoveComment={(id) => setComments((c) => c.filter((x) => x.id !== id))}
                onSent={() => {
                  setComments([]);
                  loadDetail();
                }}
              />
            )}
          </div>
        )}
      </main>
    </>
  );
}

function Timeline({ pr }: { pr: PullDetail }) {
  const now = Date.now();
  return (
    <div className="timeline">
      <article className="timeline-item opening">
        <header>
          <span className="avatar">{pr.pull.author.slice(0, 1).toUpperCase()}</span>
          <b>{pr.pull.author}</b>
          <span className="muted">
            wants to merge <code>{pr.pull.head}</code> into <code>{pr.pull.base}</code>
          </span>
        </header>
        {pr.body.trim() ? <Markdown text={pr.body} /> : <p className="muted">No description.</p>}
      </article>
      {pr.timeline.map((c, i) => (
        <article key={i} className={`timeline-item${c.verdict ? ` verdict-${c.verdict}` : ""}`}>
          <header>
            <span className="avatar">{c.author.slice(0, 1).toUpperCase()}</span>
            <b>{c.author}</b>
            {c.verdict === "approved" && <span className="pill tone-emerald"><Check size={11} /> approved</span>}
            {c.verdict === "changesrequested" && <span className="pill tone-amber">requested changes</span>}
            {c.verdict === "commented" && <span className="muted">reviewed</span>}
            <span className="flex-1" />
            <span className="muted small-text num">{ago(c.at, now)}</span>
          </header>
          {c.body.trim() && <Markdown text={c.body} />}
        </article>
      ))}
    </div>
  );
}

function Checks({ pr }: { pr: PullDetail }) {
  const shell = useShell();
  if (pr.checks.length === 0) return <p className="hint">No checks ran on this pull request.</p>;
  // Failing first: that's what you came for.
  const order = ["failure", "running", "queued", "cancelled", "success", "skipped", "neutral"];
  const checks = [...pr.checks].sort((a, b) => order.indexOf(a.status) - order.indexOf(b.status));
  return (
    <ul className="check-list">
      {checks.map((c, i) => (
        <li key={i}>
          <CiIcon status={c.status} />
          <span className="truncate">
            {c.workflow && <span className="muted">{c.workflow} / </span>}
            {c.name}
          </span>
          <span className="flex-1" />
          {c.run_id ? (
            <button
              className="link small-text"
              onClick={() => shell.go({ section: "actions", runId: c.run_id!, jobId: c.job_id ?? undefined })}
            >
              <Play size={11} /> Open in Actions
            </button>
          ) : (
            c.url && (
              <button className="link small-text" onClick={() => api.openUrl(c.url).catch(shell.fail)}>
                <External size={11} /> Details
              </button>
            )
          )}
        </li>
      ))}
    </ul>
  );
}

function PullCard({ pr, task, onChanged }: { pr: PullDetail; task?: TaskSummary; onChanged: () => void }) {
  const shell = useShell();
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState<string | null>(null);
  useEffect(() => {
    setMenu(false);
    setConfirm(null);
  }, [pr.pull.number]);
  const p = pr.pull;
  const open = p.state === "open" || p.state === "draft";
  const failing = pr.checks.filter((c) => c.status === "failure").length;
  const passing = pr.checks.filter((c) => c.status === "success").length;
  const PENDING: Record<string, string> = {
    squash: "Merging on GitHub…",
    merge: "Merging on GitHub…",
    rebase: "Merging on GitHub…",
    ready: "Marking it ready…",
    checkout: `Checking out ${p.head}…`,
    close: `Closing #${p.number}…`,
    reopen: `Reopening #${p.number}…`,
  };
  const act = (action: string) =>
    shell.act(() => api.pullAction(p.number, action), { undo: false, pending: PENDING[action] }).then((ok) => {
      if (ok) onChanged();
      setConfirm(null);
    });
  const merge = (method: string) => (confirm === method ? act(method) : setConfirm(method));
  const [settings] = useAgentSettings();
  // Reviews and line comments since the agent last ran, as its follow-up.
  const sendReview = async () => {
    if (!task) return;
    const ok = await shell.act(
      async () => {
        const text = await api.reviewFeedback(task.id);
        await api.sendToAgent(task.id, text, choiceOf(settings), settings.mode);
        return `Sent the review to ${task.id}`;
      },
      { undo: false, pending: "Sending the review…" },
    );
    if (ok) shell.go({ section: "agents", taskId: task.id });
  };
  return (
    <DetailsCard>
      <div className="inspector-section">
        <div className="inspector-row">
          <Branch size={13} />
          <span className="truncate">
            <code>{p.head}</code> <span className="muted">→ {p.base}</span>
          </span>
        </div>
        <div className="inspector-row">
          <span className="diffstat num">
            <span className="add">+{p.additions}</span> <span className="del">−{p.deletions}</span>
          </span>
          <span className="muted num">
            {pr.changed_files} files · {pr.commits} commits
          </span>
        </div>
        {pr.checks.length > 0 && (
          <div className={`inspector-row${failing ? " bad" : ""}`}>
            <CiIcon status={failing ? "failure" : passing === pr.checks.length ? "success" : "running"} size={13} />
            <span className="num">
              {failing ? `${failing} failing · ` : ""}
              {passing} of {pr.checks.length} checks passed
            </span>
          </div>
        )}
        {open && pr.mergeable === "conflicting" && (
          <div className="inspector-row bad">
            <X size={13} />
            <span>Conflicts with {p.base}</span>
          </div>
        )}
        {task && (
          <button className="inspector-row action" onClick={() => shell.go({ section: "agents", taskId: task.id })}>
            <Bot size={13} />
            <span className="truncate">
              Agent task: <b>{task.title}</b>
            </span>
          </button>
        )}
      </div>
      <div className="inspector-actions">
        {task && open && (
          <Button
            className="ghost"
            disabled={task.run?.state === "running"}
            title={task.run?.state === "running" ? "The agent is working — send it when it's done" : "Send reviews and line comments since the agent last ran"}
            onClick={sendReview}
          >
            <Bot size={12} /> Send review to agent
          </Button>
        )}
        {open && (
          <div className="split">
            <Button
              className={confirm === "squash" ? "danger" : "primary"}
              disabled={p.state === "draft"}
              title={p.state === "draft" ? "Mark it ready for review first" : "Squash and merge on GitHub"}
              onClick={() => merge("squash")}
            >
              {confirm === "squash" ? "Squash and merge?" : "Squash and merge"}
            </Button>
            <button className="primary split-more" aria-label="Other merge methods" disabled={p.state === "draft"} onClick={() => setMenu((m) => !m)}>
              <ChevronDown size={12} />
            </button>
            {menu && (
              <div className="menu" onMouseLeave={() => setMenu(false)}>
                <button onClick={() => { setMenu(false); merge("merge"); }}>Create a merge commit</button>
                <button onClick={() => { setMenu(false); merge("rebase"); }}>Rebase and merge</button>
              </div>
            )}
          </div>
        )}
        {confirm && confirm !== "squash" && (
          <Button className="danger" onClick={() => act(confirm)}>
            {confirm === "merge" ? "Merge?" : "Rebase and merge?"}
          </Button>
        )}
        {p.state === "draft" && (
          <Button className="ghost" onClick={() => act("ready")}>
            Ready for review
          </Button>
        )}
        <Button className="ghost" onClick={() => act("checkout")} title="Check out its branch here">
          Check out
        </Button>
        {p.state !== "merged" && (
          <Button className="ghost" onClick={() => act(open ? "close" : "reopen")}>
            {open ? "Close" : "Reopen"}
          </Button>
        )}
        <Button className="icon-button" title="Open on GitHub" onClick={() => api.openUrl(p.url).catch(shell.fail)}>
          <External size={13} />
        </Button>
      </div>
    </DetailsCard>
  );
}

function ReviewBox({
  pr,
  comments,
  onRemoveComment,
  onSent,
}: {
  pr: PullDetail;
  comments: LineComment[];
  onRemoveComment: (id: string) => void;
  onSent: () => void;
}) {
  const shell = useShell();
  const [text, setText] = useState("");
  const [sending, setSending] = useState<"comment" | "approve" | null>(null);
  // Line comments go in the review's body as `file:line` notes (spor's gh
  // wrapper posts one comment, not a review thread per line).
  const body = () => {
    const notes = comments.map((c) => `- \`${c.file}:${c.line}\`${c.side === "old" ? " (removed line)" : ""}: ${c.text}`);
    return [text.trim(), notes.length ? notes.join("\n") : ""].filter(Boolean).join("\n\n");
  };
  const send = (action: "comment" | "approve") => {
    if (sending) return;
    setSending(action);
    return shell
      .act(() => api.pullAction(pr.pull.number, action, body()), {
        undo: false,
        pending: action === "approve" ? "Approving…" : "Commenting…",
      })
      .then((ok) => {
        if (ok) {
          setText("");
          onSent();
        }
      })
      .finally(() => setSending(null));
  };
  const has = text.trim().length > 0 || comments.length > 0;
  return (
    <div className="composer-dock">
      <div className="column">
        <div className="composer">
          {comments.length > 0 && (
            <div className="chips">
              {comments.map((c) => (
                <span key={c.id} className="chip" title={c.text}>
                  <span className="chip-main">
                    <MessageSquare size={11} />
                    <span className="mono">
                      {c.file.split("/").pop()}:{c.line}
                    </span>
                    <span className="truncate">{c.text}</span>
                  </span>
                  <button className="chip-x" onClick={() => onRemoveComment(c.id)} aria-label="Remove comment">
                    <X size={10} />
                  </button>
                </span>
              ))}
            </div>
          )}
          <textarea
            value={text}
            rows={1}
            placeholder={`Comment on #${pr.pull.number}…`}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && has) {
                e.preventDefault();
                send("comment");
              }
            }}
          />
          <div className="composer-footer">
            <span className="muted small-text">Markdown · line comments from Files are included</span>
            <span className="flex-1" />
            <Button className="ghost" disabled={!has || sending === "approve"} busy={sending === "comment"} onClick={() => send("comment")}>
              Comment
            </Button>
            <Button className="primary" disabled={sending === "comment"} busy={sending === "approve"} onClick={() => send("approve")}>
              <Check size={12} /> Approve
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
