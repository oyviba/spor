// The History graph's options (#58): columns, labels, row height and lane
// width. Shown from the ⚙ in History's header and as a Settings page.

import { useEffect, useRef, useState } from "react";
import { DEFAULT_GRAPH, setSettings, useSettings, type GraphOptions as Options } from "../../shared/settings";
import { Gear } from "../../shared/Icons";
import { Choice } from "../../shared/SettingsDialog";

const ROWS = [
  { id: "24", label: "Compact" },
  { id: "30", label: "Normal" },
  { id: "36", label: "Roomy" },
];
const LANES = [
  { id: "10", label: "Narrow" },
  { id: "14", label: "Normal" },
  { id: "18", label: "Wide" },
];

const update = (change: (g: Options) => Options) => setSettings((s) => ({ ...s, graph: change(s.graph) }));

export function GraphOptionsPanel() {
  const g = useSettings().graph;
  const column = (key: keyof Options["columns"], label: string) => (
    <label className="check-row">
      <input
        type="checkbox"
        checked={g.columns[key]}
        onChange={(e) => update((o) => ({ ...o, columns: { ...o.columns, [key]: e.target.checked } }))}
      />
      {label}
    </label>
  );
  // A custom size (none of the presets) still shows, as its own option.
  const sized = (list: { id: string; label: string }[], value: number) =>
    list.some((o) => Number(o.id) === value) ? list : [...list, { id: String(value), label: `${value}px` }];
  return (
    <div className="graph-options">
      <div className="settings-group">
        <h4>Columns</h4>
        {column("hash", "Commit hash")}
        {column("author", "Author")}
        {column("date", "Date")}
        <label className="check-row">
          <input
            type="checkbox"
            checked={g.refColumn}
            onChange={(e) => update((o) => ({ ...o, refColumn: e.target.checked }))}
          />
          Branches and tags in a column of their own
        </label>
        <span className="muted small-text">Drag a column's edge in the header to resize it; double-click resets it.</span>
      </div>
      <div className="settings-group">
        <h4>Row height</h4>
        <Choice value={String(g.rowHeight)} onChange={(v) => update((o) => ({ ...o, rowHeight: Number(v) }))} options={sized(ROWS, g.rowHeight)} />
      </div>
      <div className="settings-group">
        <h4>Lane width</h4>
        <Choice value={String(g.laneWidth)} onChange={(v) => update((o) => ({ ...o, laneWidth: Number(v) }))} options={sized(LANES, g.laneWidth)} />
      </div>
      <button className="link small-text" onClick={() => setSettings((s) => ({ ...s, graph: DEFAULT_GRAPH }))}>
        Reset to defaults
      </button>
    </div>
  );
}

/** The ⚙ in History's header and its popover. */
export function GraphOptionsButton() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const key = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", down);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", down);
      window.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <div className="popover-anchor" ref={ref}>
      <button
        className={`icon-button${open ? " active" : ""}`}
        onClick={() => setOpen((o) => !o)}
        aria-label="Graph options"
        aria-expanded={open}
        title="Graph options: columns, row height, lane width"
      >
        <Gear size={14} />
      </button>
      {open && (
        <div className="popover" role="dialog" aria-label="Graph options">
          <GraphOptionsPanel />
        </div>
      )}
    </div>
  );
}
