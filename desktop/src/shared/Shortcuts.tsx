// The Shortcuts page in Settings (⌘/): every command with its shortcut.
// Click one and press the keys you want; Esc cancels, ⌫ removes it.
// Right-click for Remove or Reset to default (#55).

import { useEffect, useState } from "react";
import { bind, display, effective, isMac, isSafe, resetOne, shortcutOf, type Bindable } from "./keys";
import { setSettings, useSettings } from "./settings";
import type { MenuItem } from "./Overlay";

export interface CommandInfo extends Bindable {
  label: string;
  group: string;
}

export function ShortcutsPage({
  commands,
  menu,
}: {
  commands: CommandInfo[];
  menu: (e: { clientX: number; clientY: number; preventDefault?: () => void }, items: MenuItem[]) => void;
}) {
  const { keymap } = useSettings();
  const [recording, setRecording] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const mac = isMac();
  const keys = effective(commands, keymap);
  const labelOf = (id: string) => commands.find((c) => c.id === id)?.label ?? id;

  const apply = (r: { keymap: typeof keymap; took: string | null }, id: string) => {
    setSettings({ keymap: r.keymap });
    setNote(r.took ? `${display(r.keymap[id] ?? "", mac)} moved here from “${labelOf(r.took)}”, which has no shortcut now` : null);
  };

  const onKey = (id: string) => (e: KeyboardEvent) => {
    // Nothing else sees these keys while recording: not the app's
    // shortcuts, not the dialog's Esc.
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return setRecording(null);
    if ((e.key === "Backspace" || e.key === "Delete") && !e.metaKey && !e.ctrlKey && !e.altKey) {
      apply(bind(commands, keymap, id, null), id);
      return setRecording(null);
    }
    const sc = shortcutOf(e, mac);
    if (!sc) return; // just a modifier, so far
    if (!isSafe(sc)) {
      setNote(`A shortcut needs ${mac ? "⌘, ⌃ or ⌥" : "Ctrl or Alt"}, so it can't swallow typing`);
      return;
    }
    apply(bind(commands, keymap, id, sc), id);
    setRecording(null);
  };

  // Listen at the window, first: a clicked button doesn't get the focus
  // (or the keys) in WebKit on macOS.
  useEffect(() => {
    if (!recording) return;
    const handler = onKey(recording);
    const cancel = (e: MouseEvent) => !(e.target as HTMLElement).closest?.(".shortcut-row.recording") && setRecording(null);
    window.addEventListener("keydown", handler, true);
    window.addEventListener("mousedown", cancel, true);
    return () => {
      window.removeEventListener("keydown", handler, true);
      window.removeEventListener("mousedown", cancel, true);
    };
  });

  const groups = [...new Set(commands.map((c) => c.group))];
  const shown = commands.filter((c) => !filter || c.label.toLowerCase().includes(filter.toLowerCase()));
  const changed = Object.keys(keymap).length > 0;

  return (
    <div className="shortcuts">
      <div className="settings-group">
        <h4>Keyboard shortcuts</h4>
        <div className="shortcuts-bar">
          <input placeholder="Filter commands" value={filter} onChange={(e) => setFilter(e.target.value)} />
          <span className="flex-1" />
          <button
            className="link small-text"
            disabled={!changed}
            onClick={() => {
              setSettings({ keymap: {} });
              setNote("All shortcuts are back to their defaults");
            }}
          >
            Reset all
          </button>
        </div>
        <p className="muted small-text">Click a command, then press the keys. Esc cancels, ⌫ removes the shortcut. Right-click for more.</p>
        {note && <p className="shortcut-note small-text">{note}</p>}
      </div>
      {groups.map((g) => {
        const list = shown.filter((c) => c.group === g);
        if (list.length === 0) return null;
        return (
          <div key={g} className="settings-group">
            <h4 className="muted">{g}</h4>
            <div className="shortcut-list">
              {list.map((c) => {
                const k = keys.get(c.id);
                const isChanged = c.id in keymap;
                return (
                  <button
                    key={c.id}
                    className={`shortcut-row${recording === c.id ? " recording" : ""}`}
                    onClick={() => {
                      setNote(null);
                      setRecording(c.id);
                    }}
                    onContextMenu={(e) =>
                      menu(e, [
                        { label: "Change shortcut…", run: () => setRecording(c.id) },
                        { label: "Remove shortcut", disabled: !k, run: () => apply(bind(commands, keymap, c.id, null), c.id) },
                        { label: "Reset to default", disabled: !isChanged, run: () => apply(resetOne(commands, keymap, c.id), c.id) },
                      ])
                    }
                  >
                    <span className="truncate">{c.label}</span>
                    <span className="flex-1" />
                    {isChanged && <span className="muted small-text">changed</span>}
                    {recording === c.id ? (
                      <span className="shortcut-recording small-text">Press keys…</span>
                    ) : k ? (
                      <kbd>{display(k, mac)}</kbd>
                    ) : (
                      <span className="faint small-text">—</span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
}
