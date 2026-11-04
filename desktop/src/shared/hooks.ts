import { useEffect, useRef, useState } from "react";

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
