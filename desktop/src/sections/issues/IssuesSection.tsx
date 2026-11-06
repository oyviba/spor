import { useRef, useState } from "react";
import { api, errorText } from "../../api";
import type { Issue, IssueDetail, TaskSummary } from "../../types";
import { useShell } from "../../shared/app";
import { useChanged, usePoll, useStored } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { DetailsCard, EmptyState, ListLoading, PageHeader, SidebarHeader } from "../../shared/Layout";
import { Markdown } from "../../shared/Markdown";
import { AttachButton, AttachmentStrip, useAttachments, withImages } from "../../shared/Attachments";
import { StatusPill, type Status } from "../../shared/StatusPill";
import { ago } from "../agents/transcript";
import { isOpen } from "../agents/status";
import { ModelPicker, ModePicker, choiceOf, useAgentSettings } from "../agents/Composer";
import { ArrowUp, Bot, Branch, External, MessageSquare, Tag, Users } from "../../shared/Icons";

type Filter = "assigned" | "yours" | "open" | "closed";
const FILTERS: { id: Filter; label: string }[] = [
  { id: "assigned", label: "Assigned" },
  { id: "yours", label: "Yours" },
  { id: "open", label: "Open" },
  { id: "closed", label: "Closed" },
];

/** An issue's status, with the agent task working on it if there is one. */
export function issueStatus(issue: Issue, task?: TaskSummary): Status {
  if (!issue.open) return { label: "Closed", tone: "violet", live: false };
  if (task?.run?.state === "running") return { label: "Agent working", tone: "sky", live: true };
  if (task) return { label: "Has a task", tone: "emerald", live: false };
  return { label: "Open", tone: "muted", live: false };
}

/** GitHub issues, through `gh`: read one, and hand it to an agent as a task
 * whose pull request closes it (#47). */
export function IssuesSection() {
  const shell = useShell();
  const { fail, tasks } = shell;
  const [filter, setFilter] = useStored<Filter>("issues.filter", "assigned");
  const [issues, setIssues] = useState<Issue[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useStored<number | null>("issues.selected", null);
  const [detail, setDetail] = useState<IssueDetail | null>(null);

  // The filter shown: an answer for one picked before it is dropped.
  const wanted = useRef(filter);
  wanted.current = filter;
  usePoll(
    async () => {
      const asked = filter;
      try {
        const list = await api.issues(asked);
        if (wanted.current !== asked) return;
        setIssues(list);
        setError(null);
      } catch (e) {
        if (wanted.current !== asked) return;
        setError(errorText(e));
        setIssues([]);
      }
    },
    60_000,
    [filter],
  );
  useChanged(() => {
    if (shell.target.section === "issues" && shell.target.number) setSelected(shell.target.number);
  }, [shell.target.seq]);

  const loadDetail = async () => {
    if (selected == null) return;
    try {
      setDetail(await api.issueDetail(selected));
    } catch (e) {
      fail(e);
    }
  };
  usePoll(loadDetail, 60_000, [selected], selected != null && !error);
  useChanged(() => setDetail(null), [selected]);

  // The newest task on an issue: an open one, else the last that landed.
  const taskFor = (n: number) =>
    tasks.find((t) => t.issue === n && isOpen(t)) ?? tasks.find((t) => t.issue === n);
  const shown = detail?.issue.number === selected ? detail : null;
  const task = shown ? taskFor(shown.issue.number) : undefined;

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
                setIssues(null);
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
          ) : issues == null ? (
            <ListLoading what="issues" />
          ) : issues.length === 0 ? (
            <div className="sidebar-empty">
              {filter === "assigned" ? "Nothing assigned to you." : "No issues here."}
            </div>
          ) : (
            issues.map((i) => {
              const t = taskFor(i.number);
              return (
                <button
                  key={i.number}
                  className={`task-row${selected === i.number ? " selected" : ""}`}
                  onClick={() => setSelected(i.number)}
                  title={i.title}
                >
                  <div className="task-row-line">
                    <span className="task-row-title">{i.title}</span>
                    <span className="task-row-time num">{ago(i.updated, Date.now())}</span>
                  </div>
                  <div className="task-row-line sub">
                    <span className="num">#{i.number}</span>
                    <span className="truncate">{i.labels.length > 0 ? i.labels.join(", ") : i.author}</span>
                    <span className="flex-1" />
                    {i.comments > 0 && (
                      <span className="muted num">
                        <MessageSquare size={10} /> {i.comments}
                      </span>
                    )}
                    <StatusPill status={issueStatus(i, t && isOpen(t) ? t : undefined)} />
                  </div>
                </button>
              );
            })
          )}
        </div>
      </aside>
      <main className="main">
        {selected == null || error ? (
          <EmptyState title={error ? "Issues aren't available" : "Pick an issue"}>
            {error ?? "Hand one to an agent: it gets its own branch, and its pull request closes the issue."}
          </EmptyState>
        ) : !shown ? (
          <div className="boot" />
        ) : (
          <div className="task-page with-inspector">
            <PageHeader
              title={
                <>
                  <h2 title={shown.issue.title}>
                    {shown.issue.title} <span className="muted num">#{shown.issue.number}</span>
                  </h2>
                  <StatusPill status={issueStatus(shown.issue, task && isOpen(task) ? task : undefined)} />
                </>
              }
            />
            <div className="task-body">
              <div className="conversation">
                <div className="column">
                  <IssueTimeline detail={shown} />
                </div>
              </div>
              <IssueCard detail={shown} task={task} onChanged={loadDetail} />
            </div>
            {shown.issue.open && !(task && isOpen(task)) && (
              <StartTask
                key={shown.issue.number}
                detail={shown}
                onStarted={(t) => shell.go({ section: "work", taskId: t.id })}
              />
            )}
          </div>
        )}
      </main>
    </>
  );
}

function IssueTimeline({ detail }: { detail: IssueDetail }) {
  const now = Date.now();
  const i = detail.issue;
  return (
    <div className="timeline">
      <article className="timeline-item opening">
        <header>
          <span className="avatar">{i.author.slice(0, 1).toUpperCase()}</span>
          <b>{i.author}</b>
          <span className="muted">opened this issue</span>
        </header>
        {detail.body.trim() ? <Markdown text={detail.body} /> : <p className="muted">No description.</p>}
      </article>
      {detail.comments.map((c, n) => (
        <article key={n} className="timeline-item">
          <header>
            <span className="avatar">{c.author.slice(0, 1).toUpperCase()}</span>
            <b>{c.author}</b>
            <span className="flex-1" />
            <span className="muted small-text num">{ago(c.at, now)}</span>
          </header>
          {c.body.trim() && <Markdown text={c.body} />}
        </article>
      ))}
    </div>
  );
}

function IssueCard({ detail, task, onChanged }: { detail: IssueDetail; task?: TaskSummary; onChanged: () => void }) {
  const shell = useShell();
  const i = detail.issue;
  const act = (action: "close" | "reopen") =>
    shell
      .act(() => api.issueAction(i.number, action), {
        undo: false,
        pending: action === "close" ? `Closing #${i.number}…` : `Reopening #${i.number}…`,
      })
      .then((ok) => ok && onChanged());
  return (
    <DetailsCard>
      <div className="inspector-section">
        {i.labels.length > 0 && (
          <div className="inspector-row">
            <Tag size={13} />
            <span className="truncate">{i.labels.join(", ")}</span>
          </div>
        )}
        <div className="inspector-row">
          <Users size={13} />
          <span className="truncate">
            {i.assignees.length > 0 ? i.assignees.join(", ") : <span className="muted">No one assigned</span>}
          </span>
        </div>
        {task && (
          <button className="inspector-row action" onClick={() => shell.go({ section: "work", taskId: task.id })}>
            <Bot size={13} />
            <span className="truncate">
              {task.landed ? "Landed by" : task.closed ? "Closed task:" : "Agent task:"} <b>{task.id}</b>
            </span>
          </button>
        )}
        {task?.pull != null && (
          <button className="inspector-row action" onClick={() => shell.go({ section: "pulls", number: task.pull! }, { browse: true })}>
            <Branch size={13} />
            <span>
              Pull request <span className="num">#{task.pull}</span>
            </span>
          </button>
        )}
      </div>
      <div className="inspector-actions">
        <Button className="ghost" onClick={() => act(i.open ? "close" : "reopen")}>
          {i.open ? "Close issue" : "Reopen"}
        </Button>
        <Button className="icon-button" title="Open on GitHub" onClick={() => api.openUrl(i.url).catch(shell.fail)}>
          <External size={13} />
        </Button>
      </div>
    </DetailsCard>
  );
}

/** The issue as an agent's prompt, ready to edit, with where it starts
 * from and which agent runs it. */
export function StartTask({ detail, onStarted }: { detail: IssueDetail; onStarted: (t: TaskSummary) => void }) {
  const shell = useShell();
  const { repo } = shell;
  const [settings, setSettings] = useAgentSettings();
  const [prompt, setPrompt] = useState(detail.prompt);
  const [base, setBase] = useState(repo.default_base ?? repo.branch ?? repo.branches[0] ?? "main");
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const attachments = useAttachments(shell.fail);
  const verb = settings.agent === "none" ? "Create task" : "Start agent task";

  const start = async () => {
    if (busy || !prompt.trim() || attachments.saving) return;
    setBusy(true);
    try {
      const t = await api.createIssueTask(
        detail.issue.number,
        withImages(prompt, attachments.images),
        base,
        choiceOf(settings),
        settings.mode,
      );
      shell.toast(`Started ${t.id} on #${detail.issue.number}`);
      onStarted(t);
    } catch (e) {
      shell.fail(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="composer-dock">
      <div className="column">
        <div className={`composer${attachments.dragging ? " dropping" : ""}`} {...attachments.handlers}>
          <AttachmentStrip images={attachments.images} onRemove={attachments.remove} />
          {open ? (
            <textarea
              rows={8}
              value={prompt}
              autoFocus
              onChange={(e) => setPrompt(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  start();
                }
              }}
            />
          ) : (
            <button className="issue-prompt-preview" onClick={() => setOpen(true)} title="Edit what the agent is told">
              <Bot size={12} />
              <span className="truncate">
                The agent gets this issue, its comments, and: keep it focused, test it, commit. Its pull request
                closes #{detail.issue.number}.
              </span>
              <span className="link">Edit prompt</span>
            </button>
          )}
          <div className="composer-footer">
            <label className="chip-select-wrap" title="The branch it starts from and its pull request goes into">
              <Branch size={12} />
              <select className="chip-select bare" value={base} onChange={(e) => setBase(e.target.value)}>
                {repo.branches.map((b) => (
                  <option key={b}>{b}</option>
                ))}
              </select>
            </label>
            <select
              className="chip-select"
              value={settings.agent}
              onChange={(e) => setSettings({ ...settings, agent: e.target.value as typeof settings.agent })}
            >
              <option value="claude" disabled={!repo.claude}>
                Claude Code{repo.claude ? "" : " (not installed)"}
              </option>
              <option value="command">Custom command</option>
              <option value="none">No agent — I'll run one</option>
            </select>
            {settings.agent === "claude" && (
              <>
                <ModelPicker settings={settings} onSettings={setSettings} />
                <ModePicker mode={settings.mode} onChange={(mode) => setSettings({ ...settings, mode })} />
              </>
            )}
            <span className="flex-1" />
            <AttachButton onFiles={attachments.add} />
            <Button className="primary" disabled={!prompt.trim() || attachments.saving} busy={busy} onClick={start}>
              <ArrowUp size={12} /> {verb}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
