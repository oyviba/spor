import { useState } from "react";
import { api, errorText } from "../../api";
import type { Issue, IssueDetail, TaskSummary } from "../../types";
import { useShell } from "../../shared/app";
import { usePoll } from "../../shared/hooks";
import { ListLoading, PageHeader } from "../../shared/Layout";
import { IssueIcon } from "../../shared/Icons";
import { StartTask } from "../issues/IssuesSection";
import { ago } from "../agents/transcript";

/** New… → From issue: pick an open issue and hand it to an agent. An issue
 * someone is already on opens that task instead. */
export function FromIssue({ onStarted, onOpenTask }: { onStarted: (t: TaskSummary) => void; onOpenTask: (id: string) => void }) {
  const shell = useShell();
  const [issues, setIssues] = useState<Issue[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<IssueDetail | null>(null);
  usePoll(
    async () => {
      try {
        setIssues(await api.issues("open"));
        setError(null);
      } catch (e) {
        setError(errorText(e));
        setIssues([]);
      }
    },
    60_000,
    [shell.repo.root],
  );
  const taskOn = (n: number) => shell.tasks.find((t) => t.issue === n && !t.landed);
  const pick = (issue: Issue) => {
    const task = taskOn(issue.number);
    if (task) return onOpenTask(task.id);
    setPicked(null);
    api.issueDetail(issue.number).then(setPicked, shell.fail);
  };
  const now = Date.now();

  return (
    <div className="task-page from-issue">
      <PageHeader title={<h2>Work on an issue</h2>} />
      <div className="task-body">
        <div className="conversation">
          <div className="column">
            {error ? (
              <p className="hint">{error}</p>
            ) : issues == null ? (
              <ListLoading what="issues" />
            ) : issues.length === 0 ? (
              <p className="hint">No open issues.</p>
            ) : (
              <ul className="check-list issue-pick">
                {issues.map((i) => {
                  const task = taskOn(i.number);
                  return (
                    <li key={i.number} className={picked?.issue.number === i.number ? "selected" : undefined}>
                      <IssueIcon size={13} />
                      <button className="link truncate" onClick={() => pick(i)}>
                        {i.title}
                      </button>
                      <span className="flex-1" />
                      <span className="muted small-text num">
                        {task ? "has a task · " : ""}#{i.number} · {ago(i.updated, now)}
                      </span>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>
      </div>
      {picked && <StartTask key={picked.issue.number} detail={picked} onStarted={onStarted} />}
    </div>
  );
}
