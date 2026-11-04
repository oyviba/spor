import { useEffect, useRef, useState } from "react";
import { inTauri } from "../api";

/** The current time, updated every `ms` while `active`. */
export function useNow(ms = 1000, active = true): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms, active]);
  return now;
}

/** Call `fn` now and then every `ms`, while `active`. */
export function usePoll(fn: () => void | Promise<void>, ms: number, deps: unknown[], active = true) {
  const latest = useRef(fn);
  latest.current = fn;
  useEffect(() => {
    if (!active) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        await latest.current();
      } catch {
        // The next tick tries again; errors show where the call is made.
      }
      if (!stopped) timer = setTimeout(tick, ms);
    };
    tick();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ms, active, ...deps]);
}

/** What the backend's watcher saw change (#38). */
export type Change = "refs" | "index" | "worktree" | "tasks";

/** How often to look anyway, for changes a filesystem doesn't report. */
const BACKSTOP_MS = 30_000;

/** Call `fn` now, whenever the repository changes in one of `kinds` (the
 * backend watches it and says so), and every 30 s as a backstop. In a
 * plain browser there's no watcher: it polls every `fallbackMs`. Calls never
 * overlap; a change during one runs it once more after. */
export function useRepoChanges(
  fn: () => void | Promise<void>,
  kinds: Change[],
  fallbackMs: number,
  deps: unknown[],
  active = true,
) {
  const latest = useRef(fn);
  latest.current = fn;
  const busy = useRef<Promise<void> | null>(null);
  const again = useRef(false);
  const run = () => {
    if (busy.current) {
      again.current = true;
      return;
    }
    busy.current = (async () => {
      try {
        do {
          again.current = false;
          try {
            await latest.current();
          } catch {
            // The next change or tick tries again; errors show where the call is made.
          }
        } while (again.current);
      } finally {
        busy.current = null;
      }
    })();
  };
  usePoll(run, inTauri ? BACKSTOP_MS : fallbackMs, deps, active);
  const key = kinds.join(",");
  useEffect(() => {
    if (!active || !inTauri) return;
    let unlisten: (() => void) | null = null;
    let gone = false;
    import("@tauri-apps/api/event").then(({ listen }) =>
      listen<Record<Change, boolean>>("repo-changed", (e) => {
        if (kinds.some((k) => e.payload[k])) run();
      }).then((u) => (gone ? u() : (unlisten = u))),
    );
    return () => {
      gone = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, active, ...deps]);
}

/** State kept in localStorage; works (in memory) when storage is blocked. */
export function useStored<T>(key: string, initial: T): [T, (v: T | ((old: T) => T)) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = localStorage.getItem(`spor.${key}`);
      return raw == null ? initial : (JSON.parse(raw) as T);
    } catch {
      return initial;
    }
  });
  const set = (v: T | ((old: T) => T)) =>
    setValue((old) => {
      const next = typeof v === "function" ? (v as (o: T) => T)(old) : v;
      try {
        localStorage.setItem(`spor.${key}`, JSON.stringify(next));
      } catch {
        // Private window or blocked storage: keep it for this session only.
      }
      return next;
    });
  return [value, set];
}
