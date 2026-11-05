// The commit graph: a virtualized list (only rows in view are rendered),
// each row an SVG of its lane segments next to the commit's subject. The
// segments are worked out in Rust (src-tauri/src/history.rs); here they
// only become paths.

import { forwardRef, memo, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import type { Graph as GraphData, GraphRow, PrBadge, Segment } from "../../types";
import { ago } from "../agents/transcript";
import { Bot, Tag } from "../../shared/Icons";

export const ROW = 30;
const PAD = 12;
const LANE = 14;
const MID = ROW / 2;
const OVERSCAN = 12;

const x = (lane: number) => PAD + lane * LANE;

export interface GraphHandle {
  scrollTo: (hash: string) => void;
}

interface Props {
  graph: GraphData;
  selected: string | null;
  onSelect: (hash: string) => void;
  onOpen: (hash: string) => void;
  /** Search: rows not in here are dimmed. */
  hits: Set<string> | null;
  currentBranch: string | null;
  /** Remote-tracking branch names, drawn quieter than local ones. */
  remotes: Set<string>;
  nowMs: number;
  /** Open pull requests by head branch: a badge on its label. */
  prs: Map<string, PrBadge>;
  /** Right-click on a row. */
  onContext: (hash: string, e: React.MouseEvent) => void;
  /** A branch dragged onto a row's branch. */
  onDropBranch: (source: string, target: string, e: React.DragEvent) => void;
}

/** What a dragged branch carries. */
export const BRANCH_DRAG = "application/x-spor-branch";

export const Graph = forwardRef<GraphHandle, Props>(function Graph(p, ref) {
  const scroller = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ top: 0, height: 800 });
  const dark = useDark();
  const palette = useMemo(() => p.graph.palette.map((hex) => (dark ? hex : deepen(hex))), [p.graph.palette, dark]);
  const index = useMemo(() => new Map(p.graph.rows.map((r, i) => [r.hash, i])), [p.graph.rows]);
  const width = x(Math.min(p.graph.lanes, 12)) + 4;
  // Stable handlers, so scrolling re-renders only rows coming into view.
  const latest = useRef(p);
  latest.current = p;
  const select = useMemo(() => (hash: string) => latest.current.onSelect(hash), []);
  const openRow = useMemo(() => (hash: string) => latest.current.onOpen(hash), []);
  const context = useMemo(() => (hash: string, e: React.MouseEvent) => latest.current.onContext(hash, e), []);
  const dropBranch = useMemo(
    () => (source: string, target: string, e: React.DragEvent) => latest.current.onDropBranch(source, target, e),
    [],
  );

  useImperativeHandle(ref, () => ({
    scrollTo: (hash) => {
      const i = index.get(hash);
      const el = scroller.current;
      if (i == null || !el) return;
      const y = i * ROW;
      if (y < el.scrollTop || y > el.scrollTop + el.clientHeight - ROW) el.scrollTop = y - el.clientHeight / 3;
    },
  }));

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const measure = () => setView({ top: el.scrollTop, height: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const first = Math.max(0, Math.floor(view.top / ROW) - OVERSCAN);
  const last = Math.min(p.graph.rows.length, Math.ceil((view.top + view.height) / ROW) + OVERSCAN);

  // ↑/↓ move the selection; Enter opens the commit.
  const onKeyDown = (e: React.KeyboardEvent) => {
    const i = p.selected ? (index.get(p.selected) ?? -1) : -1;
    const step = e.key === "ArrowDown" || e.key === "j" ? 1 : e.key === "ArrowUp" || e.key === "k" ? -1 : 0;
    if (step) {
      e.preventDefault();
      const next = p.graph.rows[Math.max(0, Math.min(p.graph.rows.length - 1, i + step))];
      if (next) {
        p.onSelect(next.hash);
        const el = scroller.current!;
        const y = index.get(next.hash)! * ROW;
        if (y < el.scrollTop) el.scrollTop = y;
        else if (y > el.scrollTop + el.clientHeight - ROW) el.scrollTop = y - el.clientHeight + ROW;
      }
    } else if (e.key === "Enter" && p.selected) {
      p.onOpen(p.selected);
    }
  };

  return (
    <div
      className="graph"
      ref={scroller}
      tabIndex={0}
      onKeyDown={onKeyDown}
      onScroll={(e) => setView({ top: e.currentTarget.scrollTop, height: e.currentTarget.clientHeight })}
    >
      <div className="graph-canvas" style={{ height: p.graph.rows.length * ROW }}>
        {p.graph.rows.slice(first, last).map((row, k) => (
          <Row
            key={row.hash}
            row={row}
            top={(first + k) * ROW}
            width={width}
            palette={palette}
            selected={p.selected === row.hash}
            dimmed={!!p.hits && !p.hits.has(row.hash)}
            currentBranch={p.currentBranch}
            remotes={p.remotes}
            nowMs={p.nowMs}
            prs={p.prs}
            onSelect={select}
            onOpen={openRow}
            onContext={context}
            onDropBranch={dropBranch}
          />
        ))}
      </div>
    </div>
  );
});

const Row = memo(function Row({
  row,
  top,
  width,
  palette,
  selected,
  dimmed,
  currentBranch,
  remotes,
  nowMs,
  prs,
  onSelect,
  onOpen,
  onContext,
  onDropBranch,
}: {
  row: GraphRow;
  top: number;
  width: number;
  palette: string[];
  selected: boolean;
  dimmed: boolean;
  currentBranch: string | null;
  remotes: Set<string>;
  nowMs: number;
  prs: Map<string, PrBadge>;
  onSelect: (hash: string) => void;
  onOpen: (hash: string) => void;
  onContext: (hash: string, e: React.MouseEvent) => void;
  onDropBranch: (source: string, target: string, e: React.DragEvent) => void;
}) {
  const [over, setOver] = useState(false);
  // The branch a drop here lands on: a local one if there's one, else a remote one.
  const branches = row.refs.filter((r) => !r.startsWith("tag:"));
  const target = branches.find((r) => !remotes.has(r)) ?? branches[0];
  const node = x(row.lane);
  const color = palette[row.color] ?? "#888";
  return (
    <div
      className={`graph-row${selected ? " selected" : ""}${dimmed ? " dimmed" : ""}${over ? " drop-target" : ""}`}
      style={{ top }}
      onClick={() => onSelect(row.hash)}
      onDoubleClick={() => onOpen(row.hash)}
      onContextMenu={(e) => {
        e.preventDefault();
        onSelect(row.hash);
        onContext(row.hash, e);
      }}
      data-hash={row.hash}
      onDragOver={
        target
          ? (e) => {
              if (!e.dataTransfer.types.includes(BRANCH_DRAG)) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              setOver(true);
            }
          : undefined
      }
      onDragLeave={() => setOver(false)}
      onDrop={
        target
          ? (e) => {
              setOver(false);
              const source = e.dataTransfer.getData(BRANCH_DRAG);
              if (source && source !== target) {
                e.preventDefault();
                onDropBranch(source, target, e);
              }
            }
          : undefined
      }
    >
      <svg width={width} height={ROW} className="graph-svg" aria-hidden>
        {row.segments.map((s, i) => (
          <path
            key={i}
            d={pathOf(s, node)}
            stroke={palette[s.color] ?? "#888"}
            strokeWidth={s.bold ? 2.5 : 1.5}
            fill="none"
            strokeLinecap="round"
          />
        ))}
        <circle cx={node} cy={MID} r={5.5} className="node-halo" />
        {row.head ? (
          <>
            <circle cx={node} cy={MID} r={4.5} fill="var(--fg)" />
            <circle cx={node} cy={MID} r={2.5} className="node-halo" />
          </>
        ) : row.merge ? (
          <>
            <circle cx={node} cy={MID} r={3.5} fill={color} />
            <circle cx={node} cy={MID} r={2} className="node-halo" />
          </>
        ) : (
          <circle cx={node} cy={MID} r={3.5} fill={color} />
        )}
      </svg>
      <div className="graph-text">
        {row.refs.map((r) => (
          <RefBadge key={r} name={r} current={r === currentBranch} remote={remotes.has(r)} color={color} pr={prs.get(r)} />
        ))}
        <span className="graph-subject">{row.subject}</span>
      </div>
      <span className="graph-author truncate">{row.author}</span>
      <span className="graph-time num">{ago(row.time, nowMs)}</span>
    </div>
  );
});

const CHECK_GLYPH: Record<PrBadge["checks"], string> = { none: "", pending: "●", passing: "✓", failing: "✗" };

function RefBadge({
  name,
  current,
  remote,
  color,
  pr,
}: {
  name: string;
  current: boolean;
  remote: boolean;
  color: string;
  pr?: PrBadge;
}) {
  const tag = name.startsWith("tag:");
  const label = tag ? name.slice(4).trim() : name;
  const agent = label.startsWith("agent/");
  return (
    <span
      draggable={!tag}
      onDragStart={
        tag
          ? undefined
          : (e) => {
              e.dataTransfer.setData(BRANCH_DRAG, label);
              e.dataTransfer.setData("text/plain", label);
              e.dataTransfer.effectAllowed = "move";
            }
      }
      className={`ref${current ? " current" : ""}${remote ? " remote" : ""}${tag ? " tag" : ""}`}
      style={!tag && !remote ? ({ "--ref": color } as React.CSSProperties) : undefined}
      title={current ? `${label} (checked out)` : label}
    >
      {tag && <Tag size={10} />}
      {agent && <Bot size={10} />}
      {label}
      {pr && (
        <span
          className={`pr-badge review-${pr.draft ? "draft" : pr.review}`}
          title={`Pull request #${pr.number}${pr.draft ? " (draft)" : ""} — review: ${pr.review}, checks: ${pr.checks}`}
        >
          #{pr.number}
          {CHECK_GLYPH[pr.checks] && <span className={`pr-checks checks-${pr.checks}`}>{CHECK_GLYPH[pr.checks]}</span>}
        </span>
      )}
    </span>
  );
}

function pathOf(s: Segment, node: number): string {
  const lx = x(s.lane);
  const dy = MID * 0.6;
  switch (s.shape) {
    case "through":
      return `M${lx} 0V${ROW}`;
    case "top":
      return `M${lx} 0V${MID}`;
    case "bottom":
      return `M${lx} ${MID}V${ROW}`;
    case "in":
      return `M${lx} 0L${node} ${MID}`;
    case "out":
      return `M${node} ${MID}L${lx} ${ROW}`;
    case "join":
      return `M${lx} 0C${lx} ${dy} ${node} ${MID - dy} ${node} ${MID}`;
    case "fork":
      return `M${node} ${MID}C${node} ${MID + dy} ${lx} ${ROW - dy} ${lx} ${ROW}`;
  }
}

function useDark(): boolean {
  const query = "(prefers-color-scheme: dark)";
  const [dark, setDark] = useState(() => window.matchMedia?.(query).matches ?? false);
  useEffect(() => {
    const m = window.matchMedia?.(query);
    if (!m) return;
    const on = () => setDark(m.matches);
    m.addEventListener("change", on);
    return () => m.removeEventListener("change", on);
  }, []);
  return dark;
}

/** The palette is tuned for dark backgrounds; on light ones, lines and
 * dots need a deeper shade (lightness × 0.8). */
function deepen(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0;
  let s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    h /= 6;
  }
  return `hsl(${Math.round(h * 360)} ${Math.round(s * 100)}% ${Math.round(l * 80)}%)`;
}
