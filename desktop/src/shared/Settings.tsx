// Settings (⌘,): a dialog with one page per topic. Later topics add a page
// to PAGES.

import { useState } from "react";
import { setSettings, useSettings } from "./settings";
import { X } from "./Icons";

export interface SettingsPage {
  id: string;
  label: string;
  render: () => React.ReactNode;
}

export function DiffSettings() {
  const { diffLayout } = useSettings();
  return (
    <div className="settings-group">
      <h4>Diffs</h4>
      <Choice
        value={diffLayout}
        onChange={(v) => setSettings({ diffLayout: v })}
        options={[
          { id: "unified", label: "Unified", hint: "One column: removed lines above the added ones" },
          { id: "split", label: "Side by side", hint: "The old file on the left, the new one on the right" },
        ]}
      />
    </div>
  );
}

/** A row of mutually exclusive options. */
export function Choice<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { id: T; label: string; hint?: string }[];
}) {
  return (
    <div className="choice" role="radiogroup">
      {options.map((o) => (
        <button
          key={o.id}
          role="radio"
          aria-checked={value === o.id}
          className={`choice-option${value === o.id ? " on" : ""}`}
          onClick={() => onChange(o.id)}
          title={o.hint}
        >
          <span>{o.label}</span>
          {o.hint && <span className="muted small-text">{o.hint}</span>}
        </button>
      ))}
    </div>
  );
}

export function SettingsDialog({ pages, onClose }: { pages: SettingsPage[]; onClose: () => void }) {
  const [page, setPage] = useState(pages[0].id);
  const shown = pages.find((p) => p.id === page) ?? pages[0];
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className="settings"
        role="dialog"
        aria-label="Settings"
        onKeyDown={(e) => {
          // Esc closes, unless a field inside (a shortcut recorder) handles it.
          if (e.key === "Escape" && !e.defaultPrevented) onClose();
        }}
        tabIndex={-1}
      >
        <nav className="settings-nav">
          <h3>Settings</h3>
          {pages.map((p) => (
            <button key={p.id} className={p.id === shown.id ? "active" : ""} onClick={() => setPage(p.id)}>
              {p.label}
            </button>
          ))}
        </nav>
        <div className="settings-body">
          <button className="icon-button settings-close" onClick={onClose} aria-label="Close settings">
            <X size={14} />
          </button>
          {shown.render()}
        </div>
      </div>
    </div>
  );
}
