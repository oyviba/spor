// Across the top of every section: the branch you're on, and whether it's
// in step with its upstream, with the one button that fixes it when not.
// Behind is the loud case: pull before you build on stale work.

import { useState } from "react";
import { api } from "../api";
import type { RepoState } from "../types";
import { useShell } from "./app";
import { Button } from "./Button";
import { Alert, ArrowDown, ArrowUp, Branch, Refresh } from "./Icons";

type Sync = "fetch" | "pull" | "push";

const commits = (n: number) => `${n} ${n === 1 ? "commit" : "commits"}`;

/** What the bar says, and how loudly. */
export function branchStatus(state: RepoState): { tone: "ok" | "behind" | "ahead" | "warn"; text: string } {
  const { branch, upstream, ahead, behind } = state;
  if (!branch) return { tone: "warn", text: "Not on a branch (detached HEAD) — commits here belong to no branch" };
  if (!upstream) return { tone: "ahead", text: "Not published — only on this computer" };
  if (ahead > 0 && behind > 0)
    return { tone: "behind", text: `Diverged from ${upstream}: ${commits(behind)} to pull, ${commits(ahead)} to push` };
  if (behind > 0) return { tone: "behind", text: `${commits(behind)} behind ${upstream} — pull to get them` };
  if (ahead > 0) return { tone: "ahead", text: `${commits(ahead)} ahead of ${upstream} — not pushed yet` };
  return { tone: "ok", text: `Up to date with ${upstream}` };
}

export function BranchBar({ state }: { state: RepoState }) {
  const shell = useShell();
  const [busy, setBusy] = useState<Sync | null>(null);
  const run = (what: Sync) => {
    setBusy(what);
    const pending = { fetch: "Fetching…", pull: "Pulling…", push: state.upstream ? "Pushing…" : "Publishing…" }[what];
    return shell.act(() => api.sync(what), { undo: what === "pull", pending }).finally(() => setBusy(null));
  };
  // One sync at a time: the others wait (dimmed) while one runs.
  const sync = (what: Sync) => ({ busy: busy === what, disabled: busy != null && busy !== what, onClick: () => run(what) });
  const { tone, text } = branchStatus(state);
  const { branch, upstream, ahead, behind } = state;
  return (
    <div className={`branch-bar ${tone}`} role="status" aria-label="Current branch">
      <span className="branch-bar-name" title={branch ? `You're on ${branch}` : "Detached HEAD"}>
        {branch ? <Branch size={14} /> : <Alert size={14} />}
        <span className="truncate">{branch ?? "detached HEAD"}</span>
      </span>
      <span className="branch-bar-text truncate">{text}</span>
      <span className="flex-1" />
      <Button className="ghost small" {...sync("fetch")} title="Fetch from all remotes, to see what's new">
        <Refresh size={12} /> Fetch
      </Button>
      {branch && upstream && behind > 0 && (
        <Button className="primary small" {...sync("pull")} title={`Pull from ${upstream}, with rebase`}>
          <ArrowDown size={12} /> Pull {behind}
        </Button>
      )}
      {branch && (!upstream || ahead > 0) && (
        <Button
          className={`${behind > 0 ? "ghost" : "primary"} small`}
          {...sync("push")}
          disabled={(busy != null && busy !== "push") || behind > 0}
          title={!upstream ? "Publish the branch to origin" : behind > 0 ? "Pull first" : `Push to ${upstream}`}
        >
          <ArrowUp size={12} /> {upstream ? `Push ${ahead}` : "Publish"}
        </Button>
      )}
    </div>
  );
}
