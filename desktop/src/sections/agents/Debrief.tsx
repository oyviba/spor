import { useEffect, useState } from "react";
import { api } from "../../api";
import { Button } from "../../shared/Button";
import { useShell } from "../../shared/app";
import { issueFrom } from "../../shared/newIssue";
import type { Debrief as DebriefData, TaskDetail } from "../../types";

/** The debrief's open questions as one follow-up, unanswered ones left out. */
export function answersMessage(questions: string[], answers: string[]): string | null {
  const lines = questions
    .map((q, i) => [q, (answers[i] ?? "").trim()] as const)
    .filter(([, a]) => a)
    .map(([q, a]) => `- ${q}\n  Answer: ${a}`);
  return lines.length ? `Answers to your open questions:\n\n${lines.join("\n")}\n\nGo on with these in mind.` : null;
}

interface Props {
  detail: TaskDetail;
  /** Send the agent a follow-up. */
  onSend: (text: string) => unknown;
  /** Start a new task from a follow-up the agent noticed. */
  onStartTask: (prompt: string) => unknown;
}

/** What the last run says about itself, in the same five parts every time
 * (#104). It is asked for once the run has ended. */
export function Debrief({ detail, onSend, onStartTask }: Props) {
  const id = detail.task.id;
  const run = detail.runs.at(-1)?.index;
  const [fetched, setFetched] = useState<{ id: string; run: number; debrief: DebriefData } | null>(null);
  const [asking, setAsking] = useState(false);
  const [answers, setAnswers] = useState<string[]>([]);
  const wants = detail.wants_debrief;
  const shell = useShell();
  const task = { id, branch: detail.task.branch };
  /** "File as issue" (#148): the item, with a link back to the task. */
  const file = (what: string, text: string) => (
    <Button className="link small-text" onClick={() => shell.newIssue(issueFrom({ task, what, text }))} title="Open a GitHub issue for this">
      File as issue
    </Button>
  );
  useEffect(() => {
    if (!wants || run == null) return;
    let live = true;
    setAsking(true);
    api
      .debriefTask(id)
      .then((d) => live && d && setFetched({ id, run, debrief: d }))
      .catch(() => {})
      .finally(() => live && setAsking(false));
    return () => {
      live = false;
    };
  }, [id, run, wants]);
  useEffect(() => setAnswers([]), [id, run]);

  const d = detail.debrief ?? (fetched && fetched.id === id && fetched.run === run ? fetched.debrief : null);
  if (!d) {
    return asking ? (
      <div className="inspector-section">
        <span className="small-text muted">Asking the agent for a debrief…</span>
      </div>
    ) : null;
  }
  const message = answersMessage(d.open_questions, answers);
  return (
    <div className="inspector-section debrief">
      <div className="small-text muted">Debrief</div>
      <p className="small-text">{d.changed}</p>
      <p className="small-text">
        <b>Checked</b> {d.checked || "Nothing was run"}
      </p>
      {d.risks.length > 0 && (
        <>
          <b className="small-text">Look closely at</b>
          <ul className="small-text">
            {d.risks.map((r, i) => (
              <li key={i}>
                {r.path && <code>{r.path}</code>} {r.note}{" "}
                {file("a risk in the debrief", r.path ? `\`${r.path}\` ${r.note}` : r.note)}
              </li>
            ))}
          </ul>
        </>
      )}
      {d.open_questions.length > 0 && (
        <>
          <b className="small-text">Open questions</b>
          {d.open_questions.map((q, i) => (
            <div key={i} className="small-text debrief-question">
              <span>
                {q} {file("an open question in the debrief", q)}
              </span>
              <input
                value={answers[i] ?? ""}
                onChange={(e) => setAnswers((a) => Object.assign([...a], { [i]: e.target.value }))}
                placeholder="Your answer"
                aria-label={`Your answer to: ${q}`}
              />
            </div>
          ))}
          <Button disabled={!message} onClick={() => message && onSend(message)}>
            Send answers
          </Button>
        </>
      )}
      {d.follow_ups.length > 0 && (
        <>
          <b className="small-text">Follow-ups</b>
          {d.follow_ups.map((f, i) => (
            <div key={i} className="small-text debrief-followup">
              <span>
                {f} {file("a follow-up in the debrief", f)}
              </span>
              <Button className="ghost" onClick={() => onStartTask(f)} title="Start a new task for this">
                New task
              </Button>
            </div>
          ))}
        </>
      )}
    </div>
  );
}
