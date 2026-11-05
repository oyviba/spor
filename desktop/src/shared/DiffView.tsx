import { useEffect, useRef, useState } from "react";
import type { LineComment } from "../types";
import { parsePatch, type DiffFile, type DiffLine } from "./diff";
import { Chevron, MessageSquare, Plus } from "./Icons";
import { useSettings } from "./settings";

/** A line's place in a diff: removed lines are numbered in the old file. */
export interface Place {
  side: "old" | "new";
  line: number;
}

/** Lines picked in one file, as indices into that file's lines. */
export interface LineSelection {
  file: string;
  start: number;
  end: number;
}

interface Props {
  patch: string | null;
  /** Shown when the patch has no files. */
  empty?: string;
  /** Review: `+` on a line adds a comment. */
  comments?: LineComment[];
  onComment?: (file: string, line: number, side: Place["side"], text: string) => void;
  focus?: Pick<LineComment, "file" | "line" | "side"> | null;
  /** Staging: click a changed line to pick it, shift-click to extend. */
  selection?: LineSelection | null;
  onSelect?: (sel: LineSelection | null) => void;
  /** Rendered under the header of the file the selection is in. */
  selectionBar?: React.ReactNode;
  /** One file only (Changes): no file header, no collapsing. */
  bare?: boolean;
  /** A Blame link in each file's header. */
  onBlame?: (path: string) => void;
}

const LINES_PER_FILE = 400;

export const placeOf = (l: DiffLine): Place =>
  l.kind === "del" ? { side: "old", line: l.oldNo ?? 0 } : { side: "new", line: l.newNo ?? 0 };

/** The places of the first and last changed lines in a selection. */
export function selectionRange(patch: string, sel: LineSelection): { from: Place; to: Place } | null {
  const file = parsePatch(patch).find((f) => f.path === sel.file);
  if (!file) return null;
  const picked = file.lines.slice(sel.start, sel.end + 1).filter((l) => l.kind === "add" || l.kind === "del");
  if (picked.length === 0) return null;
  return { from: placeOf(picked[0]), to: placeOf(picked[picked.length - 1]) };
}

export function DiffView(p: Props) {
  if (p.patch == null) return <p className="hint">Loading changes…</p>;
  const files = parsePatch(p.patch);
  if (files.length === 0) return <p className="hint">{p.empty ?? "No changes."}</p>;
  return (
    <div className="diff-files">
      {files.map((f) => (
        <FileDiff key={f.path} file={f} {...p} />
      ))}
    </div>
  );
}

function FileDiff({ file, ...p }: Props & { file: DiffFile }) {
  const [collapsed, setCollapsed] = useState(false);
  const [all, setAll] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const lines = all ? file.lines : file.lines.slice(0, LINES_PER_FILE);
  const comments = (p.comments ?? []).filter((c) => c.file === file.path);
  const sel = p.selection?.file === file.path ? p.selection : null;
  const commenting = !!p.onComment;
  const staging = !!p.onSelect;
  const split = useSettings().diffLayout === "split";

  /** What a line needs to draw itself, in either layout. */
  const cellOf = (i: number): Cell => {
    const l = lines[i];
    const place = placeOf(l);
    const key = `${place.side}:${place.line}`;
    const changed = l.kind === "add" || l.kind === "del";
    const selected = !!sel && i >= sel.start && i <= sel.end && changed;
    return {
      line: l,
      selected,
      focused: p.focus?.file === file.path && p.focus.side === place.side && p.focus.line === place.line,
      onClick:
        staging && changed
          ? (shift) =>
              p.onSelect!(
                shift && sel
                  ? { file: file.path, start: Math.min(sel.start, i), end: Math.max(sel.end, i) }
                  : selected && sel!.start === sel!.end
                    ? null
                    : { file: file.path, start: i, end: i },
              )
          : undefined,
      onAdd: commenting ? () => setEditing(key) : undefined,
    };
  };
  /** The comments on a line, and the editor when adding one. */
  const extrasOf = (i: number) => {
    const place = placeOf(lines[i]);
    const key = `${place.side}:${place.line}`;
    const here = comments.filter((c) => c.side === place.side && c.line === place.line);
    return (
      <>
        {here.map((c) => (
          <tr key={c.id} className="comment-row">
            <td colSpan={4}>
              <div className="line-comment">
                <MessageSquare size={12} /> {c.text}
              </div>
            </td>
          </tr>
        ))}
        {editing === key && (
          <CommentEditor
            onCancel={() => setEditing(null)}
            onSave={(text) => {
              p.onComment!(file.path, place.line, place.side, text);
              setEditing(null);
            }}
          />
        )}
      </>
    );
  };
  return (
    <section className={`file${p.bare ? " bare" : ""}`}>
      {!p.bare && (
        <button className="file-header" onClick={() => setCollapsed((c) => !c)} aria-expanded={!collapsed}>
          <Chevron size={12} className={`chev${collapsed ? "" : " open"}`} />
          <span className="mono truncate">
            {file.oldPath ? `${file.oldPath} → ` : ""}
            {file.path}
          </span>
          {file.status !== "modified" && <span className={`file-status ${file.status}`}>{file.status}</span>}
          <span className="flex-1" />
          {p.onBlame && file.status !== "deleted" && file.status !== "binary" && (
            <span
              role="button"
              tabIndex={0}
              className="file-action"
              title={`Who last changed each line of ${file.path}`}
              onClick={(e) => {
                e.stopPropagation();
                p.onBlame!(file.path);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.stopPropagation();
                  p.onBlame!(file.path);
                }
              }}
            >
              Blame
            </span>
          )}
          {comments.length > 0 && (
            <span className="file-comments">
              <MessageSquare size={11} /> {comments.length}
            </span>
          )}
          <span className="diffstat num">
            <span className="add">+{file.additions}</span> <span className="del">−{file.deletions}</span>
          </span>
        </button>
      )}
      {sel && p.selectionBar}
      {!collapsed && (
        <table className={`diff${staging ? " selectable" : ""}${split ? " split" : ""}`}>
          {split && (
            <colgroup>
              <col className="col-ln" />
              <col />
              <col className="col-ln" />
              <col />
            </colgroup>
          )}
          <tbody>
            {split
              ? pairUp(lines).map((row, r) => {
                  if (row.hunk != null)
                    return (
                      <tr key={`h${r}`} className="hunk">
                        <td colSpan={4}>{lines[row.hunk].text || "⋯"}</td>
                      </tr>
                    );
                  return (
                    <SplitRow
                      key={r}
                      left={row.left != null ? cellOf(row.left) : null}
                      right={row.right != null ? cellOf(row.right) : null}
                      after={
                        <>
                          {row.left != null && extrasOf(row.left)}
                          {row.right != null && row.right !== row.left && extrasOf(row.right)}
                        </>
                      }
                    />
                  );
                })
              : lines.map((l, i) => {
                  if (l.kind === "hunk")
                    return (
                      <tr key={i} className="hunk">
                        <td colSpan={4}>{l.text || "⋯"}</td>
                      </tr>
                    );
                  const c = cellOf(i);
                  return (
                    <LineRow
                      key={i}
                      line={l}
                      selected={c.selected}
                      focused={c.focused}
                      onClick={c.onClick}
                      onAdd={c.onAdd}
                      after={extrasOf(i)}
                    />
                  );
                })}
          </tbody>
        </table>
      )}
      {!collapsed && !all && file.lines.length > LINES_PER_FILE && (
        <button className="link show-more" onClick={() => setAll(true)}>
          Show {file.lines.length - LINES_PER_FILE} more lines
        </button>
      )}
    </section>
  );
}

interface Cell {
  line: DiffLine;
  selected: boolean;
  focused: boolean;
  onClick?: (shift: boolean) => void;
  onAdd?: () => void;
}

/** One row of the side-by-side layout: indices into the file's lines. */
interface SplitRowData {
  hunk?: number;
  left?: number;
  right?: number;
}

/**
 * Pair lines for side-by-side: context on both sides; within a block of
 * changes, the removed lines on the left beside the added ones on the right.
 */
export function pairUp(lines: DiffLine[]): SplitRowData[] {
  const rows: SplitRowData[] = [];
  let i = 0;
  while (i < lines.length) {
    const l = lines[i];
    if (l.kind === "hunk") {
      rows.push({ hunk: i });
      i++;
    } else if (l.kind === "ctx") {
      rows.push({ left: i, right: i });
      i++;
    } else {
      const dels: number[] = [];
      const adds: number[] = [];
      while (i < lines.length && lines[i].kind === "del") dels.push(i++);
      while (i < lines.length && lines[i].kind === "add") adds.push(i++);
      for (let k = 0; k < Math.max(dels.length, adds.length); k++) rows.push({ left: dels[k], right: adds[k] });
    }
  }
  return rows;
}

function SplitRow({ left, right, after }: { left: Cell | null; right: Cell | null; after: React.ReactNode }) {
  const ref = useRef<HTMLTableRowElement>(null);
  const focused = !!left?.focused || !!right?.focused;
  useEffect(() => {
    if (focused) ref.current?.scrollIntoView({ block: "center" });
  }, [focused]);
  // A context line is the same line on both sides: show it once per side.
  const side = (c: Cell | null, which: "old" | "new") => {
    if (!c)
      return (
        <>
          <td className="ln num empty" />
          <td className="code empty" />
        </>
      );
    const kind = c.line.kind === "ctx" ? "ctx" : c.line.kind;
    const cls = `${kind}${c.selected ? " selected" : ""}${c.focused ? " focused" : ""}`;
    const click = c.onClick ? (e: React.MouseEvent) => c.onClick!(e.shiftKey) : undefined;
    return (
      <>
        <td className={`ln num ${cls}`}>{which === "old" ? (c.line.oldNo ?? "") : (c.line.newNo ?? "")}</td>
        <td
          className={`code ${cls}${click ? " pickable" : ""}`}
          onClick={click}
          onMouseDown={click ? (e) => e.shiftKey && e.preventDefault() : undefined}
        >
          {c.onAdd && (
            <button
              className="split-comment"
              onClick={(e) => {
                e.stopPropagation();
                c.onAdd!();
              }}
              aria-label="Comment on this line"
              title="Comment on this line"
            >
              <Plus size={11} />
            </button>
          )}
          <span className="sign">{kind === "add" ? "+" : kind === "del" ? "−" : " "}</span>
          {c.line.text}
        </td>
      </>
    );
  };
  return (
    <>
      <tr ref={ref} className="line split-line">
        {side(left, "old")}
        {side(right, "new")}
      </tr>
      {after}
    </>
  );
}

function LineRow({
  line,
  focused,
  selected,
  onAdd,
  onClick,
  after,
}: {
  line: DiffLine;
  focused: boolean;
  selected: boolean;
  onAdd?: () => void;
  onClick?: (shift: boolean) => void;
  after: React.ReactNode;
}) {
  const ref = useRef<HTMLTableRowElement>(null);
  useEffect(() => {
    if (focused) ref.current?.scrollIntoView({ block: "center" });
  }, [focused]);
  return (
    <>
      <tr
        ref={ref}
        className={`line ${line.kind}${focused ? " focused" : ""}${selected ? " selected" : ""}`}
        onClick={onClick ? (e) => onClick(e.shiftKey) : undefined}
        // Shift-click extends the pick; don't let it select text too.
        onMouseDown={onClick ? (e) => e.shiftKey && e.preventDefault() : undefined}
      >
        <td className="ln num">{line.oldNo ?? ""}</td>
        <td className="ln num">{line.newNo ?? ""}</td>
        <td className="add-comment">
          {onAdd && (
            <button onClick={onAdd} aria-label="Comment on this line" title="Comment on this line">
              <Plus size={11} />
            </button>
          )}
        </td>
        <td className="code">
          <span className="sign">{line.kind === "add" ? "+" : line.kind === "del" ? "−" : " "}</span>
          {line.text}
        </td>
      </tr>
      {after}
    </>
  );
}

function CommentEditor({ onSave, onCancel }: { onSave: (text: string) => void; onCancel: () => void }) {
  const [text, setText] = useState("");
  return (
    <tr className="comment-row">
      <td colSpan={4}>
        <form
          className="comment-editor"
          onSubmit={(e) => {
            e.preventDefault();
            if (text.trim()) onSave(text.trim());
          }}
        >
          <textarea
            autoFocus
            rows={2}
            value={text}
            placeholder="What should change here? It's sent with your next message."
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") onCancel();
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                if (text.trim()) onSave(text.trim());
              }
            }}
          />
          <div className="row-end">
            <button type="button" className="ghost small" onClick={onCancel}>
              Cancel
            </button>
            <button type="submit" className="primary small" disabled={!text.trim()}>
              Add comment
            </button>
          </div>
        </form>
      </td>
    </tr>
  );
}
