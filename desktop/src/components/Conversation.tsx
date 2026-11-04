import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RunInfo, TaskDetail } from "../types";
import { duration, runs as toRuns, stoppedLabel, summarize, type Block, type RunView, type Tool } from "../transcript";
import { useNow } from "../hooks";
import { Markdown } from "./Markdown";
import { Alert, Chevron, Square, ToolIcon } from "./Icons";

interface Props {
  detail: TaskDetail;
  onStop: () => void;
}

export function Conversation({ detail, onStop }: Props) {
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
  useEffect(() => setOpen(new Set()), [detail.task.id]);

  // Follow new output while you're at the bottom; stay put if you scrolled up.
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  });
  useEffect(() => {
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
          />
        ))}
        {running && latest && <LiveLine run={latest} onStop={onStop} />}
      </div>
    </div>
  );
}

function RunBlock({
  run,
  info,
  open,
  toggle,
}: {
  run: RunView;
  info: RunInfo | undefined;
  open: Set<string>;
  toggle: (key: string) => void;
}) {
  const key = `run-${run.index}`;
  const blocks = (list: Block[]) =>
    list.map((b, i) => <BlockView key={i} block={b} id={`${key}-${i}`} live={run.live && i === list.length - 1} open={open} toggle={toggle} />);
  const stopped = stoppedLabel(run);
  return (
    <section className="run">
      {run.prompt && (
        <div className="bubble-row">
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
          {run.done && (
            <div className={`run-end${run.done.ok ? "" : " bad"}`}>
              {!run.done.ok && <Alert size={13} />}
              <span>{stopped ?? (run.done.ok ? "Done" : run.done.summary)}</span>
              <span className="flex-1" />
              <RunMeta run={run} info={info} />
            </div>
          )}
        </>
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
}: {
  block: Block;
  id: string;
  live: boolean;
  open: Set<string>;
  toggle: (key: string) => void;
}) {
  if (block.kind === "text") return <Markdown text={block.text} />;
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

function LiveLine({ run, onStop }: { run: RunInfo; onStop: () => void }) {
  const now = useNow(1000);
  return (
    <div className="live-line">
      <span className="dot pulse tone-sky" />
      <span className="num">Working {duration(now / 1000 - run.started)}</span>
      <button className="ghost small" onClick={onStop}>
        <Square size={10} /> Stop
      </button>
    </div>
  );
}
