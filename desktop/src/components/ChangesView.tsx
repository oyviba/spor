import { useEffect, useRef, useState } from "react";
import type { LineComment } from "../types";
import { parsePatch, type DiffFile, type DiffLine } from "../diff";
import { Chevron, MessageSquare, Plus } from "./Icons";

interface Props {
  patch: string | null;
  comments: LineComment[];
  onComment: (file: string, line: number, side: LineComment["side"], text: string) => void;
  focus: { file: string; line: number; side: LineComment["side"] } | null;
}

const LINES_PER_FILE = 400;

/** Everything the task changed against its base; click a line to comment. */
export function ChangesView({ patch, comments, onComment, focus }: Props) {
  if (patch == null) return <div className="changes"><p className="hint">Loading changes…</p></div>;
  const files = parsePatch(patch);
  if (files.length === 0)
    return (
      <div className="changes">
        <p className="hint">No changes yet.</p>
      </div>
    );
  return (
    <div className="changes">
      <div className="changes-column">
        {files.map((f) => (
          <FileDiff
            key={f.path}
            file={f}
            comments={comments.filter((c) => c.file === f.path)}
            onComment={onComment}
            focus={focus?.file === f.path ? focus : null}
          />
        ))}
      </div>
    </div>
  );
}

function FileDiff({
  file,
  comments,
  onComment,
  focus,
}: {
  file: DiffFile;
  comments: LineComment[];
  onComment: Props["onComment"];
  focus: Props["focus"];
}) {
  const [collapsed, setCollapsed] = useState(false);
  const [all, setAll] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const lines = all ? file.lines : file.lines.slice(0, LINES_PER_FILE);
  // A removed line is numbered in the old file, everything else in the new.
  const placeOf = (l: DiffLine) =>
    l.kind === "del" ? { side: "old" as const, n: l.oldNo ?? 0 } : { side: "new" as const, n: l.newNo ?? 0 };
  return (
    <section className="file">
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
      {!collapsed && (
        <table className="diff">
          <tbody>
            {lines.map((l, i) => {
              if (l.kind === "hunk")
                return (
                  <tr key={i} className="hunk">
                    <td colSpan={4}>{l.text || "⋯"}</td>
                  </tr>
                );
              const { side, n } = placeOf(l);
              const key = `${side}:${n}`;
              const here = comments.filter((c) => c.side === side && c.line === n);
              return (
                <LineRow
                  key={i}
                  line={l}
                  focused={focus?.side === side && focus.line === n}
                  onAdd={() => setEditing(key)}
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
                            onComment(file.path, n, side, text);
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
  onAdd,
  after,
}: {
  line: DiffLine;
  focused: boolean;
  onAdd: () => void;
  after: React.ReactNode;
}) {
  const ref = useRef<HTMLTableRowElement>(null);
  useEffect(() => {
    if (focused) ref.current?.scrollIntoView({ block: "center" });
  }, [focused]);
  return (
    <>
      <tr ref={ref} className={`line ${line.kind}${focused ? " focused" : ""}`}>
        <td className="ln num">{line.oldNo ?? ""}</td>
        <td className="ln num">{line.newNo ?? ""}</td>
        <td className="add-comment">
          <button onClick={onAdd} aria-label="Comment on this line" title="Comment on this line">
            <Plus size={11} />
          </button>
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
