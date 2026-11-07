// Blame (#57): who last changed each line of a file, as of a commit or in
// the working tree. Lines from the same commit are grouped; the gutter
// shows the commit once per group, and clicking it opens it in the graph.

import { useMemo, useState } from "react";
import { api, errorText } from "../../api";
import type { BlameRow } from "../../types";
import { ago } from "../agents/transcript";
import { useChanged, useNow } from "../../shared/hooks";

interface Props {
  path: string;
  /** null: the working tree. */
  rev: string | null;
  onShowCommit: (hash: string) => void;
  onFileHistory: (path: string) => void;
  /** Open the conversation of the agent task that made a commit (#108). */
  onWhy: (taskId: string) => void;
}

export function Blame({ path, rev, onShowCommit, onFileHistory, onWhy }: Props) {
  const [rows, setRows] = useState<BlameRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  const now = useNow(60_000);

  useChanged(() => {
    setRows(null);
    setError(null);
    api.blame(path, rev).then(setRows, (e) => setError(errorText(e)));
  }, [path, rev]);

  // Older lines fade: recency as 0 (oldest) … 1 (newest) for the age bar.
  const age = useMemo(() => {
    const times = (rows ?? []).filter((r) => r.hash).map((r) => r.time);
    const lo = Math.min(...times);
    const hi = Math.max(...times);
    return (r: BlameRow) => (!r.hash ? 1 : hi > lo ? (r.time - lo) / (hi - lo) : 1);
  }, [rows]);

  if (error) return <p className="hint">{error}</p>;
  if (!rows) return <div className="boot" />;
  const commits = new Set(rows.map((r) => r.hash).filter(Boolean)).size;

  return (
    <div className="blame">
      <div className="blame-head">
        <span className="mono truncate">{path}</span>
        <span className="muted small-text">
          {rev ? `at ${rev.slice(0, 7)}` : "working tree"} · {rows.length} lines from {commits}{" "}
          {commits === 1 ? "commit" : "commits"}
        </span>
        <span className="flex-1" />
        <button className="link small-text" onClick={() => onFileHistory(path)}>
          File history
        </button>
      </div>
      <table className="blame-table">
        <tbody>
          {rows.map((r, i) => {
            const first = i === 0 || rows[i - 1].hash !== r.hash;
            return (
              <tr
                key={i}
                className={`${first ? "group-start" : ""}${hover && hover === r.hash ? " hover" : ""}${r.hash ? "" : " uncommitted"}`}
                onMouseEnter={() => setHover(r.hash || null)}
                onMouseLeave={() => setHover(null)}
              >
                <td className="blame-age" style={{ "--age": age(r) } as React.CSSProperties} />
                <td className="blame-gutter">
                  {first &&
                    (r.hash ? (
                      <>
                      <button className="blame-commit" title={`${r.summary}\n${r.author}`} onClick={() => onShowCommit(r.hash)}>
                        <span className="mono">{r.hash.slice(0, 7)}</span>
                        <span className="truncate">{r.summary}</span>
                        <span className="muted num">{ago(r.time, now)}</span>
                      </button>
                      {r.task && (
                        <button
                          className="link small-text blame-why"
                          title="Open the agent's conversation behind this commit"
                          onClick={() => onWhy(r.task!)}
                        >
                          Why?
                        </button>
                      )}
                      </>
                    ) : (
                      <span className="blame-commit muted">Not committed yet</span>
                    ))}
                </td>
                <td className="ln num">{r.line}</td>
                <td className="code">{r.text}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
