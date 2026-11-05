// Settings → Appearance (#54): System, Light or Dark; a preset for each
// mode; and every color, the corner radius and the text size to adjust.
// The app behind previews the mode being edited as you go. Edits are kept
// as overrides on the preset: Reset goes back to it, Copy as JSON gives a
// theme file, Paste loads one.

import { useEffect, useState } from "react";
import { setSettings, useSettings, type ModeTheme } from "./settings";
import { DEFAULT_THEME, PRESETS, TOKENS, colorsOf, setPreviewMode, useMode, type Mode } from "./theme";
import { Choice } from "./SettingsDialog";

const HEX = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

export function AppearancePage({
  toast,
  prompt,
}: {
  toast: (text: string, tone?: "ok" | "error") => void;
  prompt: (spec: { title: string; placeholder?: string; action: string; detail?: string }) => Promise<{ value: string } | null>;
}) {
  const { theme } = useSettings();
  const shown = useMode();
  const [editing, setEditing] = useState<Mode>(shown);

  // Preview the mode being edited while the page is open.
  useEffect(() => {
    setPreviewMode(editing);
    return () => setPreviewMode(null);
  }, [editing]);

  const mt = theme[editing];
  const colors = colorsOf(mt);
  const setMode = (next: ModeTheme) => setSettings((s) => ({ ...s, theme: { ...s.theme, [editing]: next } }));
  const setColor = (id: string, value: string) => setMode({ ...mt, overrides: { ...mt.overrides, [id]: value } });
  const resetColor = (id: string) => {
    const { [id]: _, ...rest } = mt.overrides;
    setMode({ ...mt, overrides: rest });
  };
  const edited = Object.keys(mt.overrides).length;

  const copy = () => {
    const json = JSON.stringify(
      { name: `Spor ${editing} (custom)`, mode: editing, colors, radius: theme.radius, fontSize: theme.fontSize },
      null,
      2,
    );
    navigator.clipboard.writeText(json).then(
      () => toast("Theme copied as JSON"),
      () => toast("Couldn't copy to the clipboard", "error"),
    );
  };
  const paste = async () => {
    const r = await prompt({
      title: `Load a ${editing} theme`,
      detail: "Paste a theme's JSON (from Copy as JSON): its colors become this mode's.",
      placeholder: '{"colors": {"bg": "#…", …}}',
      action: "Load",
    });
    if (!r) return;
    try {
      const t = JSON.parse(r.value);
      const known = new Set(TOKENS.flatMap((g) => g.tokens.map((x) => x.id)));
      const loaded = Object.fromEntries(
        Object.entries((t.colors ?? {}) as Record<string, unknown>).filter(([k, v]) => known.has(k) && typeof v === "string"),
      ) as Record<string, string>;
      if (Object.keys(loaded).length === 0) throw new Error("no colors");
      setSettings((s) => ({
        ...s,
        theme: {
          ...s.theme,
          [editing]: { preset: editing === "dark" ? "spor-dark" : "spor-light", overrides: loaded },
          radius: typeof t.radius === "number" ? t.radius : s.theme.radius,
          fontSize: typeof t.fontSize === "number" ? t.fontSize : s.theme.fontSize,
        },
      }));
      toast(`Loaded ${Object.keys(loaded).length} colors`);
    } catch {
      toast("That isn't a theme's JSON — it needs a \"colors\" object", "error");
    }
  };

  return (
    <div className="appearance">
      <div className="settings-group">
        <h4>Appearance</h4>
        <Choice
          value={theme.appearance}
          onChange={(v) => {
            setSettings((s) => ({ ...s, theme: { ...s.theme, appearance: v } }));
            if (v !== "system") setEditing(v);
          }}
          options={[
            { id: "system", label: "System", hint: "Light or dark, as the system is" },
            { id: "light", label: "Light" },
            { id: "dark", label: "Dark" },
          ]}
        />
      </div>

      <div className="settings-group">
        <div className="theme-edit-head">
          <h4>Theme</h4>
          <nav className="tabs small" role="tablist">
            {(["light", "dark"] as Mode[]).map((m) => (
              <button key={m} role="tab" aria-selected={editing === m} onClick={() => setEditing(m)}>
                {m === "light" ? "Light" : "Dark"}
              </button>
            ))}
          </nav>
          <span className="flex-1" />
          <button className="link small-text" onClick={copy}>
            Copy as JSON
          </button>
          <button className="link small-text" onClick={paste}>
            Paste…
          </button>
        </div>
        <span className="muted small-text">
          Editing the {editing} theme — the app shows it while you do{shown !== editing && theme.appearance !== "system" ? ", though Appearance says otherwise" : ""}.
        </span>
        <div className="presets">
          {PRESETS.filter((p) => p.mode === editing).map((p) => (
            <button
              key={p.id}
              className={`preset${mt.preset === p.id ? " on" : ""}`}
              onClick={() => setMode({ preset: p.id, overrides: {} })}
              title={edited && mt.preset !== p.id ? "Switching preset drops your edits to this mode" : undefined}
            >
              <span className="swatches">
                {["bg", "card", "fg", "accent", "add", "del"].map((k) => (
                  <span key={k} style={{ background: p.colors[k] }} />
                ))}
              </span>
              {p.label}
            </button>
          ))}
        </div>
      </div>

      {TOKENS.map((g) => (
        <div key={g.group} className="settings-group">
          <h4 className="muted">{g.group}</h4>
          <div className="color-grid">
            {g.tokens.map((t) => {
              const value = colors[t.id] ?? "";
              const changed = t.id in mt.overrides;
              return (
                <div key={t.id} className={`color-row${changed ? " changed" : ""}`}>
                  <label className="color-swatch" style={{ background: value }} title="Pick a color">
                    {/* The picker only takes #rrggbb: a value with alpha keeps its own. */}
                    <input
                      type="color"
                      value={HEX.test(value) && value.length === 7 ? value : "#888888"}
                      onChange={(e) => setColor(t.id, e.target.value)}
                    />
                  </label>
                  <span className="truncate">{t.label}</span>
                  <input
                    className="color-text mono"
                    value={value}
                    spellCheck={false}
                    onChange={(e) => setColor(t.id, e.target.value)}
                  />
                  {changed ? (
                    <button className="icon-button tiny" title="Back to the preset's color" onClick={() => resetColor(t.id)}>
                      ↺
                    </button>
                  ) : (
                    <span className="color-reset-space" />
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ))}

      <div className="settings-group">
        <h4 className="muted">Shape and size</h4>
        <div className="settings-row">
          <label>Corner radius</label>
          <input
            type="range"
            min={0}
            max={14}
            value={theme.radius}
            onChange={(e) => setSettings((s) => ({ ...s, theme: { ...s.theme, radius: Number(e.target.value) } }))}
          />
          <span className="num muted">{theme.radius}px</span>
        </div>
        <div className="settings-row">
          <label>Text size</label>
          <input
            type="range"
            min={11}
            max={16}
            value={theme.fontSize}
            onChange={(e) => setSettings((s) => ({ ...s, theme: { ...s.theme, fontSize: Number(e.target.value) } }))}
          />
          <span className="num muted">{theme.fontSize}px</span>
        </div>
      </div>

      <div className="appearance-reset">
        <button className="ghost small" disabled={!edited} onClick={() => setMode({ ...mt, overrides: {} })}>
          Reset {editing} colors
        </button>
        <button className="ghost small" onClick={() => setSettings((s) => ({ ...s, theme: DEFAULT_THEME }))}>
          Reset everything to Spor
        </button>
      </div>
    </div>
  );
}
