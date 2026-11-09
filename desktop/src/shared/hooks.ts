import { useEffect, useLayoutEffect, useRef, useState } from "react";
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

/** Same values, in order? */
function same(a: unknown[], b: unknown[]): boolean {
  return a.length === b.length && a.every((x, i) => Object.is(x, b[i]));
}

/** An effect for when `deps` change: like `useEffect`, but not run again
 * when a hidden section is shown with them unchanged (sections stay
 * mounted, see App). For resetting what belongs to the old value, and for
 * acting on `shell.target` once. */
export function useChanged(fn: () => void, deps: unknown[]) {
  const last = useRef<unknown[] | null>(null);
  useEffect(() => {
    if (last.current && same(last.current, deps)) return;
    last.current = deps;
    fn();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}

/** How recently a poll can have asked for it not to ask again yet (a
 * section left and come back to). */
const FRESH_MS = 10_000;

/** Call `fn` now and then every `ms`, while `active`. Back on screen with
 * the same `deps` (and active again), it waits if it asked a moment ago:
 * that answer is fresh, or still on its way. */
export function usePoll(
  fn: () => void | Promise<void>,
  ms: number,
  deps: unknown[],
  active = true,
  /** For polls that ask GitHub: stop while the window is hidden or
   * minimized, and catch up when it's shown again. */
  pauseHidden = false,
) {
  const latest = useRef(fn);
  latest.current = fn;
  const last = useRef<{ deps: unknown[]; at: number } | null>(null);
  useEffect(() => {
    if (!active) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      last.current = { deps, at: Date.now() };
      try {
        await latest.current();
      } catch {
        // The next tick tries again; errors show where the call is made.
      }
      if (!stopped && !(pauseHidden && document.hidden)) timer = setTimeout(tick, ms);
    };
    // Shown again: ask if it's been `ms` since the last ask, else wait out
    // the rest. (A tick that ran while hidden left no timer behind.)
    const resume = () => {
      if (document.hidden || stopped) return;
      clearTimeout(timer);
      const left = (last.current?.at ?? 0) + ms - Date.now();
      timer = setTimeout(tick, Math.max(0, left));
    };
    const prev = last.current;
    const wait = prev && same(prev.deps, deps) ? prev.at + Math.min(ms, FRESH_MS) - Date.now() : 0;
    if (wait > 0) timer = setTimeout(tick, wait);
    else tick();
    if (pauseHidden) document.addEventListener("visibilitychange", resume);
    return () => {
      stopped = true;
      clearTimeout(timer);
      if (pauseHidden) document.removeEventListener("visibilitychange", resume);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ms, active, pauseHidden, ...deps]);
}

/** How often GitHub is polled while the app is in view: the backend keeps
 * an answer for a minute, so asking more often only repeats it. */
export const GITHUB_POLL_MS = 2 * 60_000;
/** The same while a run or a check is going and someone is watching. */
export const GITHUB_LIVE_MS = 30_000;

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

/** Where the sections' lists and panes were scrolled to, put back when
 * `shown` changes: a hidden section's scrollers can lose their place.
 * Render it before the sections, so this runs before their own layout
 * effects (a conversation keeping to its newest output). */
export function KeepScroll({ shown }: { shown: unknown }) {
  const places = useRef(new Map<Element, { top: number; left: number }>());
  useEffect(() => {
    const onScroll = (e: Event) => {
      const el = e.target;
      if (el instanceof Element && el.closest(".section-body")) places.current.set(el, { top: el.scrollTop, left: el.scrollLeft });
    };
    document.addEventListener("scroll", onScroll, { capture: true, passive: true });
    return () => document.removeEventListener("scroll", onScroll, { capture: true });
  }, []);
  useLayoutEffect(() => {
    for (const [el, at] of places.current) {
      if (!el.isConnected) places.current.delete(el);
      else if (el.scrollTop !== at.top || el.scrollLeft !== at.left) el.scrollTo(at.left, at.top);
    }
  }, [shown]);
  return null;
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
