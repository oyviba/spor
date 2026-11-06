import { useLayoutEffect, useRef, useState } from "react";
import type { RunInfo, TaskDetail } from "../../types";
import { duration, runs as toRuns, stoppedLabel, summarize, type Block, type RunView, type Tool } from "./transcript";
import { useChanged, useNow } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { Markdown } from "../../shared/Markdown";
import { Alert, Check, Chevron, Square, Terminal, ToolIcon, X } from "../../shared/Icons";
import { verbOf } from "./Composer";

interface Props {
  detail: TaskDetail;
  onStop: () => unknown;
  onRunChanges: (index: number) => void;
  /** Undo the last run, or (`from`) that run and every run after it. */
  onUndoRun: (from: number | null) => unknown;
  /** After a failed setup: run it again, or start without it. */
  onRetrySetup: () => unknown;
  onSkipSetup: () => unknown;
}

export function Conversation({ detail, onStop, onRunChanges, onUndoRun, onRetrySetup, onSkipSetup }: Props) {
  const setupActions = { onRetrySetup, onSkipSetup, last: detail.runs.length };
  const latest = detail.runs.at(-1);
  const running = latest?.state === "running";
  const views = toRuns(detail.transcript, running);
  // Which folds and tool groups are open, kept per task while the app runs.
  const [open, setOpen] = useState<Set<string>>(new Set());
  const toggle = (key: string) =>
    setOpen((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  useChanged(() => setOpen(new Set()), [detail.task.id]);

  // Follow new output while you're at the bottom; stay put if you scrolled up.
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  });
  useChanged(() => {
    pinned.current = true;
  }, [detail.task.id]);

  if (views.length === 0) {
    return (
      <div className="conversation empty-conversation">
        <div className="column">
          <div className="bubble-row">
            <div className="bubble">{detail.task.prompt}</div>
          </div>
          <p className="hint">
            No agent has run here yet. Start one from the box below, or run your own in the task's folder.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div
      className="conversation"
      ref={scroller}
      onScroll={(e) => {
        const el = e.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      }}
    >
      <div className="column">
        {views.map((run) => (
          <RunBlock
            key={run.index}
            run={run}
            info={detail.runs[run.index]}
            open={open}
            toggle={toggle}
            canUndo={detail.undoable_from.includes(run.index)}
            later={detail.runs.filter((r) => r.index > run.index && !r.undone).length}
            onRunChanges={() => onRunChanges(run.index)}
            onUndoRun={() => onUndoRun(detail.undoable === run.index ? null : run.index)}
            setup={run.index === setupActions.last - 1 && !running ? setupActions : null}
          />
        ))}
        {running && latest && <LiveLine run={latest} waiting={detail.asks.length > 0} onStop={onStop} />}
      </div>
    </div>
  );
}

function RunBlock({
  run,
  info,
  open,
  toggle,
  canUndo,
  later,
  onRunChanges,
  onUndoRun,
  setup,
}: {
  run: RunView;
  info: RunInfo | undefined;
  open: Set<string>;
  toggle: (key: string) => void;
  canUndo: boolean;
  /** Runs after this one that "Undo from here" takes back too. */
  later: number;
  onRunChanges: () => void;
  onUndoRun: () => unknown;
  /** Retry / skip for a failed setup, on the latest run only. */
  setup: { onRetrySetup: () => unknown; onSkipSetup: () => unknown } | null;
}) {
  const [confirm, setConfirm] = useState(false);
  const undone = !!info?.undone;
  // Setup failed: the agent never started, so there's nothing of its own to
  // show or undo; the setup block says what happened.
  const setupFailed = run.blocks.some((b) => b.kind === "setup" && b.ok === false);
  const key = `run-${run.index}`;
  const blocks = (list: Block[]) =>
    list.map((b, i) => (
      <BlockView
        key={i}
        block={b}
        id={`${key}-${i}`}
        live={run.live && i === list.length - 1}
        open={open}
        toggle={toggle}
        setup={setup}
      />
    ));
  const stopped = stoppedLabel(run);
  return (
    <section className={`run${undone ? " undone" : ""}`}>
      {run.prompt && (
        <div className="bubble-row">
          {undone && <span className="undone-badge" title="Its changes were taken back; follow-ups start from before it">Undone</span>}
          <div className="bubble">{run.prompt}</div>
        </div>
      )}
      {run.fold ? (
        <>
          <button className="fold" onClick={() => toggle(key)} aria-expanded={open.has(key)}>
            <span>{run.fold.label}</span>
            <Chevron size={12} className={`chev${open.has(key) ? " open" : ""}`} />
            <span className="flex-1" />
            <RunMeta run={run} info={info} />
          </button>
          {open.has(key) && <div className="fold-body">{blocks(run.fold.hidden)}</div>}
          {run.fold.final && <BlockView block={run.fold.final} id={`${key}-final`} live={false} open={open} toggle={toggle} />}
        </>
      ) : (
        <>
          {blocks(run.blocks)}
          {run.done && !setupFailed && (
            <div className={`run-end${run.done.ok ? "" : " bad"}`}>
              {!run.done.ok && <Alert size={13} />}
              <span>{stopped ?? (run.done.ok ? "Done" : run.done.summary)}</span>
              <span className="flex-1" />
              <RunMeta run={run} info={info} />
            </div>
          )}
        </>
      )}
      {run.done && !undone && !setupFailed && (
        <div className="run-actions">
          <button className="link small-text" onClick={onRunChanges}>
            Changes in this run
          </button>
          {canUndo && (
            <Button
              className={`link small-text${confirm ? " danger-link" : ""}`}
              onClick={() => (confirm ? onUndoRun() : setConfirm(true))}
              onBlur={() => setConfirm(false)}
              title={
                later > 0
                  ? `Put the folder back as it was before this run, taking back the ${later + 1} runs from here, and start follow-ups from before it (⌘Z brings them back)`
                  : "Put the folder back as it was before this run, and start follow-ups from before it (⌘Z brings it back)"
              }
            >
              {later > 0
                ? confirm
                  ? `Undo ${later + 1} runs? Their changes go`
                  : "Undo from here"
                : confirm
                  ? "Undo this run? Its changes go"
                  : "Undo this run"}
            </Button>
          )}
        </div>
      )}
    </section>
  );
}

function RunMeta({ run, info }: { run: RunView; info: RunInfo | undefined }) {
  const parts: string[] = [];
  if (info) parts.push(info.label);
  if (run.done?.turns) parts.push(`${run.done.turns} turns`);
  const cost = run.done?.cost_usd ?? info?.cost_usd;
  if (cost != null) parts.push(`$${cost.toFixed(2)}`);
  return <span className="run-meta num">{parts.join(" · ")}</span>;
}

function BlockView({
  block,
  id,
  live,
  open,
  toggle,
  setup = null,
}: {
  block: Block;
  id: string;
  live: boolean;
  open: Set<string>;
  toggle: (key: string) => void;
  setup?: { onRetrySetup: () => unknown; onSkipSetup: () => unknown } | null;
}) {
  if (block.kind === "setup") {
    // A failure shows its output; success folds it away.
    const shown = block.ok === false || open.has(id) || block.ok == null;
    return (
      <div className={`setup-block${block.ok === false ? " bad" : block.ok ? " good" : ""}`}>
        <button className="tools-summary" onClick={() => toggle(id)} aria-expanded={shown}>
          {block.ok === false ? <Alert size={13} /> : block.ok ? <Check size={13} /> : <Terminal size={13} />}
          <span>
            {block.ok == null
              ? "Setting up the folder…"
              : block.ok
                ? "Set up the folder"
                : `Setup failed (exit ${block.code ?? 1}) — the agent didn't start`}
          </span>
          {block.lines.length > 0 && <Chevron size={12} className={`chev${shown ? " open" : ""}`} />}
        </button>
        {shown && block.lines.length > 0 && (
          <pre className="setup-output">{block.lines.slice(-40).join("\n")}</pre>
        )}
        {block.ok === false && setup && (
          <div className="run-actions">
            <Button className="link small-text" onClick={setup.onRetrySetup}>
              Retry
            </Button>
            <Button className="link small-text" onClick={setup.onSkipSetup}>
              Start without setup
            </Button>
          </div>
        )}
      </div>
    );
  }
  if (block.kind === "text") return <Markdown text={block.text} />;
  if (block.kind === "steer")
    return (
      <div className="bubble-row">
        <div className="bubble steer" title="Sent while it worked">
          {block.text}
        </div>
      </div>
    );
  if (block.kind === "ask") {
    const a = block.answer;
    return (
      <div className={`ask-line${a ? (a.allow ? " allowed" : " denied") : " waiting"}`}>
        {a?.allow ? <Check size={13} /> : a ? <X size={13} /> : <Alert size={13} />}
        <span>
          Asked to {verbOf(block.tool)}
          {block.detail && (
            <>
              {" "}
              <code>{block.detail}</code>
            </>
          )}
        </span>
        <span className="ask-outcome">
          {a
            ? a.allow
              ? a.always
                ? "Allowed for this session"
                : "Allowed"
              : a.message
                ? `Denied: ${a.message}`
                : "Denied"
            : "Waiting for you"}
        </span>
      </div>
    );
  }
  if (block.kind === "output") {
    const shown = open.has(id) ? block.lines : block.lines.slice(-6);
    return (
      <div className="output">
        {block.lines.length > 6 && (
          <button className="link" onClick={() => toggle(id)}>
            {open.has(id) ? "Show less" : `Show all ${block.lines.length} lines`}
          </button>
        )}
        <pre>{shown.join("\n")}</pre>
      </div>
    );
  }
  return <ToolGroup tools={block.tools} id={id} live={live} expanded={open.has(id)} toggle={toggle} />;
}

/** A stretch of tool calls as one sentence; open it for the list. */
function ToolGroup({
  tools,
  id,
  live,
  expanded,
  toggle,
}: {
  tools: Tool[];
  id: string;
  live: boolean;
  expanded: boolean;
  toggle: (key: string) => void;
}) {
  const { text, failed } = summarize(tools);
  const last = tools.at(-1);
  const lastTool = tools.findLast((t) => t.kind === "tool");
  return (
    <div className="tools">
      <button className="tools-summary" onClick={() => toggle(id)} aria-expanded={expanded}>
        <ToolIcon name={lastTool?.kind === "tool" ? lastTool.name : ""} />
        <span>{text}</span>
        {failed > 0 && <span className="failed">{failed} failed</span>}
        <Chevron size={12} className={`chev${expanded ? " open" : ""}`} />
      </button>
      {expanded ? (
        <ul className="tool-list">
          {tools.map((t, i) => (
            <li key={i} className={t.kind === "tool_error" ? "bad" : ""}>
              {t.kind === "tool" ? (
                <>
                  <ToolIcon name={t.name} size={12} />
                  <span className="tool-name">{t.name}</span>
                  <code>{t.detail}</code>
                </>
              ) : (
                <>
                  <Alert size={12} />
                  <span>{t.text}</span>
                </>
              )}
            </li>
          ))}
        </ul>
      ) : (
        live &&
        last?.kind === "tool" && (
          <div className="tool-now">
            <span className="tool-name">{last.name}</span> <code>{last.detail}</code>
          </div>
        )
      )}
    </div>
  );
}

function LiveLine({ run, waiting, onStop }: { run: RunInfo; waiting: boolean; onStop: () => unknown }) {
  const now = useNow(1000);
  return (
    <div className={`live-line${waiting ? " waiting" : ""}`}>
      <span className={`dot${waiting ? "" : " pulse"}`} />
      <span className="num">
        {waiting ? "Waiting for you" : `Working ${duration(now / 1000 - run.started)}`}
      </span>
      <Button className="ghost small" onClick={onStop}>
        <Square size={10} /> Stop
      </Button>
    </div>
  );
}
