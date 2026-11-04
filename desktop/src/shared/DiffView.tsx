import { useEffect, useRef, useState } from "react";
import type { LineComment } from "../types";
import { parsePatch, type DiffFile, type DiffLine } from "./diff";
import { Chevron, MessageSquare, Plus } from "./Icons";

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
        <table className={`diff${staging ? " selectable" : ""}`}>
          <tbody>
            {lines.map((l, i) => {
              if (l.kind === "hunk")
                return (
                  <tr key={i} className="hunk">
                    <td colSpan={4}>{l.text || "⋯"}</td>
                  </tr>
                );
              const place = placeOf(l);
              const key = `${place.side}:${place.line}`;
              const here = comments.filter((c) => c.side === place.side && c.line === place.line);
              const changed = l.kind === "add" || l.kind === "del";
              const selected = !!sel && i >= sel.start && i <= sel.end && changed;
              return (
                <LineRow
                  key={i}
                  line={l}
                  selected={selected}
                  focused={p.focus?.file === file.path && p.focus.side === place.side && p.focus.line === place.line}
                  onClick={
                    staging && changed
                      ? (shift) =>
                          p.onSelect!(
                            shift && sel
                              ? { file: file.path, start: Math.min(sel.start, i), end: Math.max(sel.end, i) }
                              : selected && sel!.start === sel!.end
                                ? null
                                : { file: file.path, start: i, end: i },
                          )
                      : undefined
                  }
                  onAdd={commenting ? () => setEditing(key) : undefined}
                  after={
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
                  }
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
