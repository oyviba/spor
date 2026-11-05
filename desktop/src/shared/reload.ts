/** A reload that runs one at a time: asked again while it runs (a change
 * came in meanwhile), it runs once more when it's done, however many times
 * it was asked. `settled` is called after each round, unless stopped. */
export function reloader(fn: () => void | Promise<void>, settled: () => void) {
  let stopped = false;
  let running = false;
  let again = false;
  return {
    run: async () => {
      if (running) {
        again = true;
        return;
      }
      running = true;
      do {
        again = false;
        try {
          await fn();
        } catch {
          // The next change tries again; errors show where the call is made.
        }
      } while (again && !stopped);
      running = false;
      if (!stopped) settled();
    },
    stop: () => {
      stopped = true;
    },
  };
}
