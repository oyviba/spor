import { useEffect, useMemo, useRef, useState } from "react";
import { api, errorText } from "../../api";
import type { Job, LogLine, Run, RunDetail, TaskSummary } from "../../types";
import { Button } from "../../shared/Button";
import { useShell } from "../../shared/app";
import { WorkCard } from "../../shared/WorkCard";
import { findWork, itemFromBranch } from "../../shared/work";
import { useChanged, useNow, usePoll, useStored } from "../../shared/hooks";
import { DetailsCard, EmptyState, ListLoading, PageHeader, SidebarHeader } from "../../shared/Layout";
import { StatusPill } from "../../shared/StatusPill";
import { CiIcon, ciStatus, elapsed } from "../../shared/ci";
import { ago } from "../agents/transcript";
import { choiceOf, useAgentSettings } from "../agents/Composer";
import { isOpen } from "../agents/status";
import { Bot, Branch, Chevron, Commit, External, Refresh, Square } from "../../shared/Icons";

/** GitHub Actions: runs, their jobs and logs; re-run and cancel. */
export function ActionsSection() {
  const shell = useShell();
  const { fail, state, tasks } = shell;
  const [workflow, setWorkflow] = useStored<string>("actions.workflow", "");
  const [mine, setMine] = useStored("actions.mine", false);
  const [workflows, setWorkflows] = useState<string[]>([]);
  const [runs, setRuns] = useState<Run[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useStored<number | null>("actions.selected", null);
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [jobId, setJobId] = useState<number | null>(null);
  // Bumped after a re-run or cancel: reload the list and the run now.
  const [nonce, setNonce] = useState(0);
  const now = useNow(1000, runs?.some((r) => r.status === "running" || r.status === "queued") ?? false);

  const branch = mine ? (state?.branch ?? null) : null;
  const anyLive = runs?.some((r) => r.status === "running" || r.status === "queued") ?? false;
  // The filter shown: an answer for one picked before it is dropped.
  const filterKey = `${workflow}\n${branch}`;
  const wanted = useRef(filterKey);
  wanted.current = filterKey;
  useChanged(() => setRuns(null), [filterKey]);
  usePoll(
    async () => {
      const asked = filterKey;
      try {
        const list = await api.runs(branch, workflow || null);
        if (wanted.current !== asked) return;
        setRuns(list);
        setError(null);
      } catch (e) {
        if (wanted.current !== asked) return;
        setError(errorText(e));
        setRuns([]);
      }
    },
    anyLive ? 10_000 : 30_000,
    [workflow, branch, nonce],
  );
  useEffect(() => {
    api.workflows().then(setWorkflows, () => {});
  }, []);

  // Sent here from a PR's checks: that run, and that job. From a work
  // card's checks: the branch's latest run, failing jobs first.
  useChanged(() => {
    const t = shell.target;
    if (t.section !== "actions") return;
    if (t.runId) {
      setSelected(t.runId);
      if (t.jobId) setJobId(t.jobId);
    } else if (t.branch) {
      api.runs(t.branch, null).then((list) => {
        const latest = list.find((r) => r.status === "failure") ?? list[0];
        if (latest) setSelected(latest.id);
      }, fail);
    }
  }, [shell.target.seq]);

  const live = detail?.run.status === "running" || detail?.run.status === "queued";
  usePoll(
    async () => {
      if (selected == null) return;
      try {
        const d = await api.runDetail(selected);
        setDetail(d);
        // Open the job that failed, else the first.
        setJobId((j) => (j && d.jobs.some((x) => x.id === j) ? j : (d.jobs.find((x) => x.status === "failure") ?? d.jobs[0])?.id ?? null));
      } catch (e) {
        fail(e);
      }
    },
    live ? 5000 : 60_000,
    [selected, nonce],
    selected != null && !error,
  );
  useChanged(() => setDetail(null), [selected]);

  const run = detail?.run.id === selected ? detail : null;
  const job = run?.jobs.find((j) => j.id === jobId) ?? null;
  const task = run ? tasks.find((t) => t.branch === run.run.branch && isOpen(t)) : undefined;

  return (
    <>
      <aside className="sidebar">
        <SidebarHeader />
        <div className="filters">
          <select className="chip-select wide" value={workflow} onChange={(e) => setWorkflow(e.target.value)}>
            <option value="">All workflows</option>
            {workflows.map((w) => (
              <option key={w}>{w}</option>
            ))}
          </select>
          <button className={`chip-toggle${mine ? " on-accent" : ""}`} onClick={() => setMine(!mine)} title="Only runs for the checked-out branch">
            <Branch size={11} /> {state?.branch ?? "this branch"}
          </button>
        </div>
        <div className="task-list">
          {error ? (
            <div className="sidebar-empty">{error}</div>
          ) : runs == null ? (
            <ListLoading what="runs" />
          ) : runs.length === 0 ? (
            <div className="sidebar-empty">No runs.</div>
          ) : (
            runs.map((r) => (
              <button key={r.id} className={`task-row${selected === r.id ? " selected" : ""}`} onClick={() => setSelected(r.id)} title={r.title}>
                <div className="task-row-line">
                  <CiIcon status={r.status} size={13} />
                  <span className="task-row-title">{r.title}</span>
                  <span className="task-row-time num">
                    {r.status === "running" ? elapsed(r.created, now / 1000) : ago(r.updated, now)}
                  </span>
                </div>
                <div className="task-row-line sub">
                  <span className="truncate">
                    {r.workflow} <span className="num">#{r.number}</span>
                  </span>
                  <span className="flex-1" />
                  <Branch size={11} className="shrink-0" />
                  <span className="truncate run-branch">{r.branch}</span>
                </div>
              </button>
            ))
          )}
        </div>
      </aside>
      <main className="main">
        {selected == null || error ? (
          <EmptyState title={error ? "Actions aren't available" : "Pick a run"}>{error}</EmptyState>
        ) : !run ? (
          <div className="boot" />
        ) : (
          <div className="task-page with-inspector">
            <PageHeader
              title={
                <>
                  <h2 title={run.run.title}>{run.run.title}</h2>
                  <StatusPill status={ciStatus(run.run.status, live ? elapsed(run.run.created, now / 1000) : undefined)} />
                </>
              }
            />
            <div className="task-body">
              <div className="changes">
                <div className="changes-column">
                  <div className="job-tabs">
                    {run.jobs.map((j) => (
                      <button key={j.id} className={`job-tab${j.id === jobId ? " active" : ""}`} onClick={() => setJobId(j.id)}>
                        <CiIcon status={j.status} size={13} />
                        <span className="truncate">{j.name}</span>
                        {j.started > 0 && (
                          <span className="muted num small-text">{elapsed(j.started, j.completed > 0 ? j.completed : now / 1000)}</span>
                        )}
                      </button>
                    ))}
                  </div>
                  {job ? <JobLog job={job} /> : <p className="hint">This run has no jobs yet.</p>}
                </div>
              </div>
              <RunCard run={run} job={job} task={task} onChanged={() => setNonce((n) => n + 1)} />
            </div>
          </div>
        )}
      </main>
    </>
  );
}

interface LogSection {
  title: string;
  lines: LogLine[];
  failed: boolean;
}

/** A log as sections, one per `##[group]` (a step, mostly). */
export function sections(lines: LogLine[]): LogSection[] {
  const out: LogSection[] = [];
  for (const l of lines) {
    if (l.kind === "group" || out.length === 0) {
      out.push({ title: l.kind === "group" ? l.text : "Setup", lines: l.kind === "group" ? [] : [l], failed: false });
      continue;
    }
    const s = out[out.length - 1];
    s.lines.push(l);
    if (l.kind === "error") s.failed = true;
  }
  return out;
}

function JobLog({ job }: { job: Job }) {
  const shell = useShell();
  const [lines, setLines] = useState<LogLine[] | null>(null);
  const [open, setOpen] = useState<Set<number>>(new Set());
  const done = job.status !== "running" && job.status !== "queued";
  // The log exists once the job finishes; reload it then.
  useEffect(() => {
    setLines(null);
    if (!done) return;
    api.jobLog(job.id).then(setLines, (e) => {
      setLines([]);
      shell.fail(e);
    });
  }, [job.id, done]); // eslint-disable-line react-hooks/exhaustive-deps
  const secs = useMemo(() => sections(lines ?? []), [lines]);
  // Failed sections start open, everything else folded.
  useEffect(() => setOpen(new Set(secs.flatMap((s, i) => (s.failed ? [i] : [])))), [secs]);

  if (!done)
    return (
      <div className="steps">
        {job.steps.map((s) => (
          <div key={s.number} className="step">
            <CiIcon status={s.status} size={13} />
            <span>{s.name}</span>
          </div>
        ))}
        <p className="hint">The log appears when the job finishes.</p>
      </div>
    );
  if (lines == null) return <p className="hint">Loading the log…</p>;
  if (secs.length === 0) return <p className="hint">This job has no log.</p>;
  return (
    <div className="log">
      {secs.map((s, i) => {
        const isOpen = open.has(i);
        return (
          <section key={i} className={`log-section${s.failed ? " failed" : ""}`}>
            <button
              className="log-title"
              onClick={() => setOpen((o) => (o.has(i) ? new Set([...o].filter((x) => x !== i)) : new Set([...o, i])))}
              aria-expanded={isOpen}
            >
              <Chevron size={12} className={`chev${isOpen ? " open" : ""}`} />
              <span className="truncate">{s.title}</span>
              <span className="flex-1" />
              <span className="muted num small-text">
                {s.lines.length} {s.lines.length === 1 ? "line" : "lines"}
              </span>
            </button>
            {isOpen && (
              <pre className="log-lines">
                {s.lines.map((l, j) => (
                  <div key={j} className={`log-line ${l.kind}`}>
                    {l.text || " "}
                  </div>
                ))}
              </pre>
            )}
          </section>
        );
      })}
    </div>
  );
}

function RunCard({ run, job, task, onChanged }: { run: RunDetail; job: Job | null; task?: TaskSummary; onChanged: () => void }) {
  const shell = useShell();
  const [settings] = useAgentSettings();
  const r = run.run;
  const item = findWork(shell.work, { task: task?.id, branch: r.branch }) ?? itemFromBranch(r.branch);
  const live = r.status === "running" || r.status === "queued";
  const failed = run.jobs.some((j) => j.status === "failure");
  const act = (a: "rerun" | "rerun_failed" | "cancel") =>
    shell
      .act(() => api.runAction(r.id, a), {
        undo: false,
        pending: a === "cancel" ? "Cancelling the run…" : "Starting the re-run…",
      })
      .then(onChanged);

  // A failed run on an agent's branch: hand the failed job (its failing
  // step and the end of its log) to that agent.
  const sendToAgent = async () => {
    if (!task) return;
    const failedJob = job?.status === "failure" ? job : run.jobs.find((j) => j.status === "failure");
    if (!failedJob) return;
    const ok = await shell.act(
      () =>
        api
          .jobFeedback(r.id, failedJob.id)
          .then((message) => api.sendToAgent(task.id, message, choiceOf(settings), settings.mode))
          .then(() => `Sent the failure to ${task.id}`),
      { undo: false, pending: "Sending the failure…" },
    );
    if (ok) shell.go({ section: "work", taskId: task.id });
  };

  return (
    <DetailsCard>
      <WorkCard item={item} from="actions" />
      <div className="inspector-section">
        <div className="inspector-row">
          <Refresh size={13} />
          <span className="truncate">
            {r.workflow} <span className="muted num">#{r.number}{r.attempt > 1 ? ` · attempt ${r.attempt}` : ""}</span>
          </span>
        </div>
        <div className="inspector-row muted">
          <span className="truncate">On {r.event.replace(/_/g, " ")}</span>
        </div>
        <button className="inspector-row action" onClick={() => shell.go({ section: "history", hash: r.sha })} title="Show the commit in History">
          <Commit size={13} />
          <span className="mono">{r.sha.slice(0, 7)}</span>
        </button>
      </div>
      <div className="inspector-actions">
        {task && failed && !live && (
          <Button className="primary" onClick={sendToAgent} title="Send the failing log to the agent working on this branch">
            <Bot size={12} /> Send failure to agent
          </Button>
        )}
        {live ? (
          <Button className="ghost" onClick={() => act("cancel")}>
            <Square size={10} /> Cancel
          </Button>
        ) : (
          <>
            {failed && (
              <Button className={task ? "ghost" : "primary"} onClick={() => act("rerun_failed")}>
                Re-run failed
              </Button>
            )}
            <Button className="ghost" onClick={() => act("rerun")}>
              Re-run all
            </Button>
          </>
        )}
        <Button className="icon-button" title="Open on GitHub" onClick={() => api.openUrl(r.url).catch(shell.fail)}>
          <External size={13} />
        </Button>
      </div>
    </DetailsCard>
  );
}
