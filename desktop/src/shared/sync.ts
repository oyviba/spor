// What the branch bar says about the checked-out branch and its upstream,
// and which sync buttons it offers — pure, so it's tested without a window.

import type { RepoState } from "../types.ts";

/** fetch, pull (rebasing), push (or publish), force-push (a rewritten
 * branch, with a lease), or sync: pull, then push. */
export type SyncAction = "fetch" | "pull" | "push" | "force-push" | "sync";

const commits = (n: number) => `${n} ${n === 1 ? "commit" : "commits"}`;

/** What the bar says, and how loudly. */
export function branchStatus(state: RepoState): { tone: "ok" | "behind" | "ahead" | "warn"; text: string } {
  const { branch, upstream, ahead, behind, rewritten, unlinked, remote } = state;
  if (!branch) return { tone: "warn", text: "Not on a branch (detached HEAD) — commits here belong to no branch" };
  if (!upstream && unlinked)
    return { tone: "behind", text: `${unlinked} is on the remote but not linked to ${branch} — pull to link them` };
  if (!upstream && remote === null) return { tone: "ahead", text: "Only on this computer — the repository has no remote" };
  if (!upstream) return { tone: "ahead", text: "Not published — only on this computer" };
  if (ahead > 0 && behind > 0 && rewritten)
    return {
      tone: "warn",
      text: `Rewritten since you pushed (amend or rebase) — force-push to replace what's on ${upstream}`,
    };
  if (ahead > 0 && behind > 0)
    return { tone: "behind", text: `Diverged from ${upstream}: ${commits(behind)} to pull, ${commits(ahead)} to push` };
  if (behind > 0) return { tone: "behind", text: `${commits(behind)} behind ${upstream} — pull to get them` };
  if (ahead > 0) return { tone: "ahead", text: `${commits(ahead)} ahead of ${upstream} — not pushed yet` };
  return { tone: "ok", text: `Up to date with ${upstream}` };
}

/** Which sync buttons the bar shows, the first the loud one. */
export function syncButtons(state: RepoState): SyncAction[] {
  const { branch, upstream, ahead, behind, rewritten, unlinked, remote } = state;
  if (!branch || remote === null) return [];
  if (!upstream) return unlinked ? ["pull"] : ["push"];
  if (ahead > 0 && behind > 0) return rewritten ? ["force-push"] : ["sync", "pull"];
  if (behind > 0) return ["pull"];
  if (ahead > 0) return ["push"];
  return [];
}

