// Back and forward between places (a section, the item open in it, and its
// repository), like a browser's. Links between sections (an Inbox row, a
// work card's checks, a toast's Answer) send you somewhere else; Back takes
// you to where you were, with what you had open there.

import type { Target } from "./app";

export interface Place {
  /** The repository, or null for what spans every one (the Inbox). */
  repo: string | null;
  /** The section and what was open in it. */
  target: Target;
  /** "Work · Rename drafts to posts", for the Back button. */
  label: string;
  /** In Agents: the task whose conversation was open. */
  agentOpen?: { repo: string; taskId: string } | null;
}

export interface Nav {
  back: Place[];
  forward: Place[];
}

export const EMPTY_NAV: Nav = { back: [], forward: [] };

/** How far back is kept. */
const MAX = 50;

export function samePlace(a: Place, b: Place): boolean {
  return (
    a.repo === b.repo &&
    JSON.stringify(a.target) === JSON.stringify(b.target) &&
    JSON.stringify(a.agentOpen ?? null) === JSON.stringify(b.agentOpen ?? null)
  );
}

/** Leaving `from` for somewhere new: it goes on the back stack (once), and
 * what was ahead is dropped, as in a browser. */
export function leave(nav: Nav, from: Place): Nav {
  const top = nav.back[nav.back.length - 1];
  const back = top && samePlace(top, from) ? nav.back : [...nav.back, from].slice(-MAX);
  return { back, forward: [] };
}

/** Step back from `current`: where to go, and the stacks after. */
export function stepBack(nav: Nav, current: Place): { nav: Nav; to: Place } | null {
  const to = nav.back[nav.back.length - 1];
  if (!to) return null;
  return { to, nav: { back: nav.back.slice(0, -1), forward: [current, ...nav.forward].slice(0, MAX) } };
}

/** Step forward again after going back. */
export function stepForward(nav: Nav, current: Place): { nav: Nav; to: Place } | null {
  const to = nav.forward[0];
  if (!to) return null;
  return { to, nav: { back: [...nav.back, current].slice(-MAX), forward: nav.forward.slice(1) } };
}
