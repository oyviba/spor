// What floats over the app: a context menu, a dialog that asks for a name
// or a yes, the credential prompt from git or ssh, and the ⌘K palette.
// Sections reach the first two through the shell (`shell.menu`,
// `shell.prompt`, `shell.confirm`); the shell owns the rest.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Credential } from "../types";
import { Search } from "./Icons";

export interface MenuItem {
  label: string;
  run: () => void;
  danger?: boolean;
  disabled?: boolean;
  /** A line above this item. */
  separator?: boolean;
}

export interface MenuSpec {
  x: number;
  y: number;
  items: MenuItem[];
}

/** A right-click (or ⋯) menu at the pointer. Closes on pick, Esc or a click outside. */
export function MenuView({ spec, onClose }: { spec: MenuSpec; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: spec.x, top: spec.y });
  // Keep it on screen.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({
      left: Math.max(8, Math.min(spec.x, window.innerWidth - r.width - 8)),
      top: Math.max(8, Math.min(spec.y, window.innerHeight - r.height - 8)),
    });
  }, [spec]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    const down = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && onClose();
    window.addEventListener("keydown", key);
    window.addEventListener("mousedown", down);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("keydown", key);
      window.removeEventListener("mousedown", down);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);
  return (
    <div ref={ref} className="menu floating" role="menu" style={pos}>
      {spec.items.map((item, i) => (
        <div key={i}>
          {item.separator && i > 0 && <div className="menu-separator" />}
          <button
            role="menuitem"
            className={item.danger ? "danger-text" : undefined}
            disabled={item.disabled}
            onClick={() => {
              onClose();
              item.run();
            }}
          >
            {item.label}
          </button>
        </div>
      ))}
    </div>
  );
}

export type DialogSpec =
  | {
      kind: "prompt";
      title: string;
      detail?: string;
      placeholder?: string;
      initial?: string;
      action: string;
      /** A second, optional field (an annotated tag's message). */
      extra?: { placeholder: string };
      resolve: (v: { value: string; extra: string } | null) => void;
    }
  | {
      kind: "confirm";
      title: string;
      detail?: string;
      action: string;
      danger?: boolean;
      resolve: (ok: boolean) => void;
    };

export function DialogView({ spec, onClose }: { spec: DialogSpec; onClose: () => void }) {
  const [value, setValue] = useState(spec.kind === "prompt" ? (spec.initial ?? "") : "");
  const [extra, setExtra] = useState("");
  const cancel = () => {
    if (spec.kind === "prompt") spec.resolve(null);
    else spec.resolve(false);
    onClose();
  };
  const submit = () => {
    if (spec.kind === "prompt") {
      if (!value.trim()) return;
      spec.resolve({ value: value.trim(), extra: extra.trim() });
    } else spec.resolve(true);
    onClose();
  };
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && cancel()}>
      <form
        className="dialog"
        role="dialog"
        aria-label={spec.title}
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
        onKeyDown={(e) => e.key === "Escape" && cancel()}
      >
        <h3>{spec.title}</h3>
        {spec.detail && <p className="muted">{spec.detail}</p>}
        {spec.kind === "prompt" && (
          <>
            <input
              autoFocus
              value={value}
              placeholder={spec.placeholder}
              onChange={(e) => setValue(e.target.value)}
              onFocus={(e) => e.currentTarget.select()}
            />
            {spec.extra && <input value={extra} placeholder={spec.extra.placeholder} onChange={(e) => setExtra(e.target.value)} />}
          </>
        )}
        <div className="dialog-actions">
          <button type="button" className="ghost" onClick={cancel}>
            Cancel
          </button>
          <button
            type="submit"
            autoFocus={spec.kind === "confirm"}
            className={spec.kind === "confirm" && spec.danger ? "danger" : "primary"}
            disabled={spec.kind === "prompt" && !value.trim()}
          >
            {spec.action}
          </button>
        </div>
      </form>
    </div>
  );
}

/** git or ssh asking for a passphrase, a password, or whether to trust a host. */
export function CredentialDialog({ ask, onAnswer }: { ask: Credential; onAnswer: (answer: string | null) => void }) {
  const [value, setValue] = useState("");
  useEffect(() => setValue(""), [ask.id]);
  return (
    <div className="overlay">
      <form
        className="dialog"
        role="dialog"
        aria-label="Git needs a credential"
        onSubmit={(e) => {
          e.preventDefault();
          onAnswer(ask.confirm ? "yes" : value);
        }}
        onKeyDown={(e) => e.key === "Escape" && onAnswer(null)}
      >
        <h3>{ask.confirm ? "Trust this host?" : "Git needs a credential"}</h3>
        <p className="credential-prompt mono">{ask.prompt}</p>
        {!ask.confirm && (
          <input
            autoFocus
            type={ask.secret ? "password" : "text"}
            autoComplete="off"
            value={value}
            onChange={(e) => setValue(e.target.value)}
          />
        )}
        <div className="dialog-actions">
          <button type="button" className="ghost" onClick={() => onAnswer(null)}>
            Cancel
          </button>
          <button type="submit" className="primary" autoFocus={ask.confirm}>
            {ask.confirm ? "Trust and continue" : "Continue"}
          </button>
        </div>
      </form>
    </div>
  );
}

export interface Command {
  id: string;
  label: string;
  /** Shown dimmed after the label: a group or a path. */
  hint?: string;
  keys?: string;
  run: () => void;
}

/** ⌘K: every command, section, recent repository and branch, by typing. */
export function Palette({ commands, onClose }: { commands: Command[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const shown = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return commands.filter((c) => {
      const hay = `${c.label} ${c.hint ?? ""}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  }, [query, commands]);
  useEffect(() => setSel(0), [query]);
  useEffect(() => {
    list.current?.querySelector(".palette-item.selected")?.scrollIntoView({ block: "nearest" });
  }, [sel]);
  const pick = (c: Command | undefined) => {
    if (!c) return;
    onClose();
    c.run();
  };
  return (
    <div className="overlay top" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-label="Command palette">
        <div className="palette-search">
          <Search size={14} />
          <input
            autoFocus
            value={query}
            placeholder="Run a command, open a repository, switch branch…"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") onClose();
              else if (e.key === "ArrowDown") {
                e.preventDefault();
                setSel((s) => Math.min(s + 1, shown.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setSel((s) => Math.max(s - 1, 0));
              } else if (e.key === "Enter") pick(shown[sel]);
            }}
          />
        </div>
        <div className="palette-list" ref={list}>
          {shown.length === 0 && <div className="palette-empty muted">No matches</div>}
          {shown.map((c, i) => (
            <button
              key={c.id}
              className={`palette-item${i === sel ? " selected" : ""}`}
              onMouseEnter={() => setSel(i)}
              onClick={() => pick(c)}
            >
              <span className="truncate">{c.label}</span>
              {c.hint && <span className="palette-hint truncate">{c.hint}</span>}
              <span className="flex-1" />
              {c.keys && <kbd>{c.keys}</kbd>}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
