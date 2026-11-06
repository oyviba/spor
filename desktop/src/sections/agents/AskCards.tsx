// The two questions that aren't "may I?": Claude Code's AskUserQuestion,
// answered by picking options (#102), and its plan, approved or sent back
// with comments (#101). Both sit in the composer, like a permission question.

import { useState } from "react";
import type { Ask, Mode } from "../../types";
import { Button } from "../../shared/Button";
import { Markdown } from "../../shared/Markdown";
import { Check, MessageSquare } from "../../shared/Icons";
import { MODES } from "./Composer";

type QuestionsAsk = Extract<Ask, { ask: "questions" }>;
type PlanAsk = Extract<Ask, { ask: "plan" }>;

/** Each question with its options as buttons, and room for an answer of your own. */
export function QuestionCard({
  ask,
  more,
  onAnswer,
}: {
  ask: QuestionsAsk;
  /** How many other questions wait after this one. */
  more: number;
  onAnswer: (answers: string[][]) => unknown;
}) {
  const [picked, setPicked] = useState<string[][]>(() => ask.questions.map(() => []));
  const [own, setOwn] = useState<string[]>(() => ask.questions.map(() => ""));
  const [busy, setBusy] = useState(false);
  const answers = ask.questions.map((_, i) => [...picked[i], ...(own[i].trim() ? [own[i].trim()] : [])]);
  const ready = answers.every((a) => a.length > 0);
  const pick = (qi: number, label: string, multi: boolean) =>
    setPicked((all) =>
      all.map((p, i) =>
        i !== qi ? p : multi ? (p.includes(label) ? p.filter((l) => l !== label) : [...p, label]) : [label],
      ),
    );
  const submit = () => {
    if (!ready || busy) return;
    setBusy(true);
    Promise.resolve(onAnswer(answers)).finally(() => setBusy(false));
  };
  return (
    <div className="ask-card questions" role="alert">
      <div className="ask-head">
        <MessageSquare size={14} />
        <b>{ask.questions.length === 1 ? "Asks you" : `Asks you ${ask.questions.length} things`}</b>
        {more > 0 && <span className="muted"> · {more} more waiting</span>}
      </div>
      {ask.questions.map((q, qi) => (
        <fieldset key={qi} className="question">
          <legend>
            {q.header && <span className="question-header">{q.header}</span>}
            <span className="question-text">{q.question}</span>
            {q.multi && <span className="muted small-text"> · pick any</span>}
          </legend>
          <div className="choices">
            {q.options.map((o) => {
              const on = picked[qi].includes(o.label);
              return (
                <button
                  key={o.label}
                  type="button"
                  className={`choice${on ? " on" : ""}`}
                  aria-pressed={on}
                  onClick={() => pick(qi, o.label, q.multi)}
                >
                  <span className="choice-label">
                    {on && <Check size={11} />}
                    {o.label}
                  </span>
                  {o.description && <span className="choice-description">{o.description}</span>}
                </button>
              );
            })}
          </div>
          <input
            className="choice-own"
            value={own[qi]}
            placeholder={q.multi ? "And/or say it in your own words" : "Or say it in your own words"}
            onChange={(e) => {
              const v = e.target.value;
              setOwn((all) => all.map((x, i) => (i === qi ? v : x)));
              // An answer of your own replaces a single pick.
              if (!q.multi && v.trim()) setPicked((all) => all.map((p, i) => (i === qi ? [] : p)));
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                submit();
              }
            }}
          />
        </fieldset>
      ))}
      <div className="ask-actions">
        <span className="muted small-text">Or type below to tell it something else instead</span>
        <Button className="primary small" busy={busy} disabled={!ready || busy} onClick={submit}>
          <Check size={11} /> Answer
        </Button>
      </div>
    </div>
  );
}

/** The plan to read, line by line: click a line to quote it in a comment.
 * Approve starts the work in the mode picked beside the button; a comment
 * from the box below sends it back to planning. */
export function PlanCard({
  ask,
  then,
  onThen,
  onApprove,
  onQuote,
}: {
  ask: PlanAsk;
  /** The mode it works in once approved. */
  then: Mode;
  onThen: (m: Mode) => void;
  onApprove: () => unknown;
  /** Put a line of the plan into the comment box. */
  onQuote: (line: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const lines = ask.plan.split("\n");
  return (
    <div className="ask-card plan" role="alert">
      <div className="ask-head">
        <MessageSquare size={14} />
        <b>Has a plan for you to review</b>
        <span className="muted small-text"> · nothing has changed yet</span>
      </div>
      <div className="plan-body">
        {lines.map((line, i) =>
          line.trim() ? (
            <button
              key={i}
              type="button"
              className="plan-line"
              title="Quote this line in a comment"
              onClick={() => onQuote(line.trim())}
            >
              <Markdown text={line} />
            </button>
          ) : (
            <div key={i} className="plan-gap" />
          ),
        )}
      </div>
      <div className="ask-actions">
        <span className="muted small-text">Click a line to comment on it; sending a comment asks for a new plan</span>
        <select
          className="chip-select"
          value={then}
          onChange={(e) => onThen(e.target.value as Mode)}
          title="How it works once the plan is approved"
        >
          {MODES.filter((m) => m.id !== "plan").map((m) => (
            <option key={m.id} value={m.id}>
              then {m.label.toLowerCase()}
            </option>
          ))}
        </select>
        <Button
          className="primary small"
          busy={busy}
          disabled={busy}
          onClick={() => {
            setBusy(true);
            Promise.resolve(onApprove()).finally(() => setBusy(false));
          }}
        >
          <Check size={11} /> Approve plan
        </Button>
      </div>
    </div>
  );
}
