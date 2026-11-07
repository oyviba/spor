// A pull request on the work page (#80): its conversation, changes and
// checks, the card to land it, and the box to comment or approve. The Pull
// requests browser and Work show it the same way, and a task with a pull
// request gets its Discussion and Checks tabs from here.

import { useEffect, useState } from "react";
import { api } from "../../api";
import type { LineComment, Pull, PullDetail, Run, TaskSummary } from "../../types";
import { useShell, type SectionId } from "../../shared/app";
import { WorkCard } from "../../shared/WorkCard";
import { findWork, itemFromPull } from "../../shared/work";
import { useChanged, usePoll, useStored, GITHUB_POLL_MS } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { DetailsCard } from "../../shared/Layout";
import { DiffView } from "../../shared/DiffView";
import { Markdown } from "../../shared/Markdown";
import { type Status } from "../../shared/StatusPill";
import { CiIcon } from "../../shared/ci";
import { ago } from "../agents/transcript";
import { isOpen } from "../agents/status";
import { Bot, Check, ChevronDown, External, MessageSquare, Play, X } from "../../shared/Icons";
import { choiceOf, useAgentSettings } from "../agents/Composer";
import { catchUpPrompt } from "../agents/prompts";
import { WorkPage, shownTab, type WorkTab } from "./WorkPage";
import { handToAgent } from "./handover";

/** A pull request's details, reloaded every 30 s, and its diff once asked. */
export function usePullDetail(number: number | null, wantDiff: boolean) {
  const { fail } = useShell();
  const [detail, setDetail] = useState<PullDetail | null>(null);
  const [diff, setDiff] = useState<string | null>(null);
  const reload = async () => {
    if (number == null) return;
    try {
      setDetail(await api.pullDetail(number));
    } catch (e) {
      fail(e);
    }
  };
  usePoll(reload, GITHUB_POLL_MS, [number], number != null, true);
  useChanged(() => {
    setDetail(null);
    setDiff(null);
  }, [number]);
  useEffect(() => {
    if (wantDiff && number != null && diff == null) api.pullDiff(number).then(setDiff, fail);
  }, [wantDiff, number]); // eslint-disable-line react-hooks/exhaustive-deps
  return { pr: detail?.pull.number === number ? detail : null, diff, reload };
}

/** The Actions runs on a branch with no pull request: its Checks tab. */
export function RunChecks({ branch }: { branch: string }) {
  const shell = useShell();
  const [runs, setRuns] = useState<Run[] | null>(null);
  usePoll(
    async () => setRuns(await api.runs(branch, null).catch(() => [])),
    GITHUB_POLL_MS,
    [branch],
    true,
    true,
  );
  if (runs == null) return <p className="hint">Loading runs…</p>;
  // The latest run of each workflow is where the branch stands.
  const latest = runs.filter((r, i) => runs.findIndex((o) => o.workflow === r.workflow) === i);
  if (latest.length === 0) return <p className="hint">No Actions runs on {branch}.</p>;
  return (
    <ul className="check-list">
      {latest.map((r) => (
        <li key={r.id}>
          <CiIcon status={r.status} />
          <span className="truncate">
            {r.workflow} <span className="muted num">#{r.number}</span>
          </span>
          <span className="flex-1" />
          <button className="link small-text" onClick={() => shell.go({ section: "actions", runId: r.id }, { browse: true })}>
            <Play size={11} /> Open in Actions
          </button>
        </li>
      ))}
    </ul>
  );
}

/** A pull request on the work page. A task's pull request opens as the
 * task (TaskView); this is for one with no task: a teammate's, or yours. */
export function PullView({ number, from }: { number: number; from: SectionId }) {
  const shell = useShell();
  const [settings] = useAgentSettings();
  const [tab, setTab] = useState<WorkTab>("discussion");
  const [inspector, setInspector] = useStored("inspector", true);
  const [comments, setComments] = useState<LineComment[]>([]);
  const { pr, diff, reload } = usePullDetail(number, tab === "changes");
  useChanged(() => {
    setComments([]);
    setTab("discussion");
  }, [number]);
  if (!pr) return <div className="boot" />;

  const p = pr.pull;
  const task = shell.tasks.find((t) => t.branch === p.head && isOpen(t));
  const item = findWork(shell.work, { pull: p.number, branch: p.head }) ?? itemFromPull(p);
  const tabs: { id: WorkTab; extra?: React.ReactNode }[] = [
    { id: "discussion" },
    {
      id: "changes",
      extra: (
        <>
          <span className="tab-count num">{pr.changed_files}</span>
          {comments.length > 0 && (
            <span className="tab-count accent num">
              <MessageSquare size={10} /> {comments.length}
            </span>
          )}
        </>
      ),
    },
    { id: "checks", extra: <span className="tab-count num">{pr.checks.length}</span> },
  ];
  const shown = shownTab(tabs, tab);
  // Your own branch with no agent on it: an agent can take it from here.
  const local = shell.repo.branches.includes(p.head);
  const open = p.state === "open" || p.state === "draft";
  const action =
    !task && open && local && item.owner.kind === "you" ? (
      <Button className="ghost" onClick={() => handToAgent(shell, settings, p.head, p.base)} title={`Give ${p.head} its own folder and an agent`}>
        <Bot size={12} /> Start an agent on this
      </Button>
    ) : null;

  return (
    <WorkPage
      item={item}
      title={
        <>
          {p.title} <span className="muted num">#{p.number}</span>
        </>
      }
      hint={p.title}
      status={pullStatus(p)}
      action={action}
      tabs={tabs}
      tab={shown}
      onTab={setTab}
      inspector={inspector}
      onInspector={setInspector}
      details={<PullCard pr={pr} task={task} from={from} onChanged={reload} />}
      dock={
        p.state !== "merged" &&
        shown !== "checks" && (
          <ReviewBox
            pr={pr}
            comments={comments}
            onRemoveComment={(id) => setComments((c) => c.filter((x) => x.id !== id))}
            onSent={() => {
              setComments([]);
              reload();
            }}
          />
        )
      }
    >
      {shown === "discussion" ? (
        <div className="conversation">
          <div className="column">
            <Timeline pr={pr} />
          </div>
        </div>
      ) : shown === "changes" ? (
        <div className="changes">
          <div className="changes-column">
            <DiffView
              patch={diff}
              comments={comments}
              onComment={(file, line, side, text) => setComments((c) => [...c, { id: `${Date.now()}`, file, line, side, text }])}
            />
          </div>
        </div>
      ) : (
        <div className="conversation">
          <div className="column">
            <PullChecks pr={pr} />
          </div>
        </div>
      )}
    </WorkPage>
  );
}

/** The one status a PR shows in lists: what's in the way, or where it is. */
export function pullStatus(p: Pull): Status {
  if (p.state === "merged") return { label: "Landed", tone: "violet", live: false };
  if (p.state === "closed") return { label: "Closed", tone: "muted", live: false };
  if (p.checks === "failing") return { label: "Checks failing", tone: "red", live: false };
  if (p.review === "changesrequested") return { label: "Changes requested", tone: "amber", live: false };
  if (p.state === "draft") return { label: "Draft", tone: "muted", live: false };
  if (p.checks === "pending") return { label: "Checks running", tone: "sky", live: true };
  if (p.review === "approved") return { label: "Approved", tone: "emerald", live: false };
  return { label: "Open", tone: "sky", live: false };
}


export function Timeline({ pr }: { pr: PullDetail }) {
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

export function PullChecks({ pr }: { pr: PullDetail }) {
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
              onClick={() => shell.go({ section: "actions", runId: c.run_id!, jobId: c.job_id ?? undefined }, { browse: true })}
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

export function PullCard({ pr, task, from, onChanged }: { pr: PullDetail; task?: TaskSummary; from: SectionId; onChanged: () => void }) {
  const shell = useShell();
  const item = findWork(shell.work, { pull: pr.pull.number, branch: pr.pull.head }) ?? itemFromPull(pr.pull);
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState<string | null>(null);
  useChanged(() => {
    setMenu(false);
    setConfirm(null);
  }, [pr.pull.number]);
  const p = pr.pull;
  const open = p.state === "open" || p.state === "draft";
  const conflicting = open && pr.mergeable === "conflicting";
  const PENDING: Record<string, string> = {
    squash: `Landing #${p.number} on GitHub…`,
    merge: `Landing #${p.number} on GitHub…`,
    rebase: `Landing #${p.number} on GitHub…`,
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
    if (ok) shell.go({ section: "work", taskId: task.id });
  };
  // GitHub won't merge it until the base is merged into the branch.
  const askCatchUp = async () => {
    if (!task) return;
    const ok = await shell.act(
      async () => {
        await api.sendToAgent(task.id, catchUpPrompt(p.base, [], true), choiceOf(settings), settings.mode);
        return `Asked ${task.id} to merge ${p.base} in`;
      },
      { undo: false, pending: "Sending to the agent…" },
    );
    if (ok) shell.go({ section: "work", taskId: task.id });
  };
  return (
    <DetailsCard>
      <WorkCard item={item} from={from} />
      <div className="inspector-section">
        <div className="inspector-row">
          <span className="diffstat num">
            <span className="add">+{p.additions}</span> <span className="del">−{p.deletions}</span>
          </span>
          <span className="muted num">
            {pr.changed_files} files · {pr.commits} commits
          </span>
        </div>
        {conflicting && (
          <div className="inspector-row bad">
            <X size={13} />
            <span>
              Conflicts with {p.base}, so GitHub can't merge it.{" "}
              {task
                ? "Have the agent merge it in, then push its commit."
                : `Check it out, merge origin/${p.base} into it, resolve the conflicts and push.`}
              {task && (
                <button
                  className="link"
                  disabled={task.run?.state === "running"}
                  title={task.run?.state === "running" ? "The agent is working — ask when it's done" : undefined}
                  onClick={askCatchUp}
                >
                  Ask agent to merge {p.base} in
                </button>
              )}
            </span>
          </div>
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
              disabled={p.state === "draft" || conflicting}
              title={
                p.state === "draft"
                  ? "Mark it ready for review first"
                  : conflicting
                    ? `Resolve the conflicts with ${p.base} first`
                    : `Squash and merge into ${p.base} on GitHub`
              }
              onClick={() => merge("squash")}
            >
              {confirm === "squash" ? `Land #${p.number}?` : `Land #${p.number}`}
            </Button>
            <button className="primary split-more" aria-label="Other ways to land" disabled={p.state === "draft" || conflicting} onClick={() => setMenu((m) => !m)}>
              <ChevronDown size={12} />
            </button>
            {menu && (
              <div className="menu" onMouseLeave={() => setMenu(false)}>
                <button onClick={() => { setMenu(false); merge("merge"); }}>Land with a merge commit</button>
                <button onClick={() => { setMenu(false); merge("rebase"); }}>Land by rebasing</button>
              </div>
            )}
          </div>
        )}
        {confirm && confirm !== "squash" && (
          <Button className="danger" onClick={() => act(confirm)}>
            {confirm === "merge" ? "Land with a merge commit?" : "Land by rebasing?"}
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

export function ReviewBox({
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
