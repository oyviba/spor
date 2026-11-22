import type { Surfaced } from "../../types";
import { api } from "../../api";
import { useShell } from "../../shared/app";
import { External, FileIcon, MessageSquare } from "../../shared/Icons";

/** What the agent put in the panel with its `surface` tool (a link, a file
 * it made, a note): the things it wants seen while it works. */
export function SurfacedItems({ items, folder, onCopy }: { items: Surfaced[]; folder: string; onCopy: (text: string, what: string) => void }) {
  const shell = useShell();
  if (items.length === 0) return null;
  return (
    <div className="inspector-section surfaced">
      {items.map((s) => {
        switch (s.kind) {
          case "link":
            return (
              <button key={s.kind + s.value} className="inspector-row action" title={s.value} onClick={() => api.openUrl(s.value).catch(shell.fail)}>
                <External size={13} />
                <span className="truncate">{s.title}</span>
              </button>
            );
          case "file":
            return (
              <button key={s.kind + s.value} className="inspector-row action" title={`Copy the path of ${s.value}`} onClick={() => onCopy(`${folder}/${s.value}`, "Path")}>
                <FileIcon size={13} />
                <span className="truncate">
                  {s.title} <code>{s.value}</code>
                </span>
              </button>
            );
          case "note":
            return (
              <div key={s.kind + s.value} className="inspector-row">
                <MessageSquare size={13} />
                <span>
                  <b>{s.title}</b> {s.value}
                </span>
              </div>
            );
        }
      })}
    </div>
  );
}
