import { useEffect, useRef, useState } from "react";
import { onRepoChange, type RepoChange } from "../api";
import { reloader } from "./reload";

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

/** How often `useRepoChanges` reloads anyway, in case the watcher missed
 * an event (on a network filesystem, say). */
const BACKSTOP_MS = 30_000;

/** Call `fn` now, whenever the repository changes in one of the `on` ways,
 * and every 30 s in case a change went unseen, while `active`. A change
 * while `fn` runs calls it once more when it's done. */
export function useRepoChanges(
  fn: () => void | Promise<void>,
  on: RepoChange[],
  deps: unknown[],
  active = true,
) {
  const latest = useRef(fn);
  latest.current = fn;
  useEffect(() => {
    if (!active) return;
    let timer: ReturnType<typeof setTimeout>;
    const reload = reloader(
      () => {
        clearTimeout(timer);
        return latest.current();
      },
      () => (timer = setTimeout(reload.run, BACKSTOP_MS)),
    );
    const stop = onRepoChange(on, reload.run);
    reload.run();
    return () => {
      reload.stop();
      clearTimeout(timer);
      stop();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, on.join(), ...deps]);
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
