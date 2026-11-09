// Across the top of every section: the branch you're on, and whether it's
// in step with its upstream, with the one button that fixes it when not.
// Behind is the loud case: pull before you build on stale work. Diverged
// gets one button too: Pull & push when the remote has someone else's
// commits, Force push when they're this branch's own, rewritten since.

import { useState } from "react";
import { api, errorText } from "../api";
import type { RepoState } from "../types";
import { branchStatus, syncButtons, type SyncAction } from "./sync";
import { useShell, type Shell } from "./app";
import { Button } from "./Button";
import { Alert, ArrowDown, ArrowUp, Branch, Refresh } from "./Icons";

const commits = (n: number) => `${n} ${n === 1 ? "commit" : "commits"}`;

const pendingText = (what: SyncAction, state: RepoState | null) =>
  ({
    fetch: "Fetching…",
    pull: "Pulling…",
    push: state?.upstream ? "Pushing…" : "Publishing…",
    "force-push": "Force-pushing…",
    sync: "Pulling, then pushing…",
  })[what];

/**
 * Fetch, pull, push, force-push or pull-and-push, toasting the result. A
 * push the remote refuses because it has commits this branch doesn't
 * offers to pull them and push again, rather than sending you to a
 * terminal.
 */
export function runSync(shell: Shell, what: SyncAction): Promise<boolean> {
  return shell.act(
    async () => {
      try {
        return await api.sync(what);
      } catch (e) {
        const text = errorText(e);
        if ((what === "push" || what === "force-push") && text.includes("has commits you don't have")) {
          shell.toast(text, "error", false, { label: "Pull and push", run: () => void runSync(shell, "sync") });
          return;
        }
        throw e;
      }
    },
    { undo: what === "pull" || what === "sync", pending: pendingText(what, shell.state) },
  );
}

export function BranchBar({ state }: { state: RepoState }) {
  const shell = useShell();
  const [busy, setBusy] = useState<SyncAction | null>(null);
  const run = (what: SyncAction) => {
    setBusy(what);
    return runSync(shell, what).finally(() => setBusy(null));
  };
  const { tone, text } = branchStatus(state);
  const { branch, upstream, ahead, behind, unlinked, remote } = state;
  const shown = syncButtons(state);
  // One sync at a time: the others wait (dimmed) while one runs.
  const button = (what: SyncAction, i: number) => {
    const loud = i === 0;
    const props = { busy: busy === what, disabled: busy != null && busy !== what, onClick: () => run(what) };
    switch (what) {
      case "pull":
        return (
          <Button
            key={what}
            className={`${loud ? "primary" : "ghost"} small`}
            {...props}
            title={upstream ? `Pull from ${upstream}, rebasing your commits on top` : `Link ${branch} to ${unlinked} and pull from it`}
          >
            <ArrowDown size={12} /> {upstream ? `Pull ${behind}` : "Pull"}
          </Button>
        );
      case "push":
        return (
          <Button
            key={what}
            className="primary small"
            {...props}
            title={upstream ? `Push to ${upstream}` : `Publish ${branch} to ${remote ?? "the remote"}`}
          >
            <ArrowUp size={12} /> {upstream ? `Push ${ahead}` : "Publish"}
          </Button>
        );
      case "sync":
        return (
          <Button
            key={what}
            className="primary small"
            {...props}
            title={`Pull ${commits(behind)} from ${upstream}, rebasing your ${commits(ahead)} on top, then push`}
          >
            <ArrowDown size={12} />
            <ArrowUp size={12} /> Pull &amp; push
          </Button>
        );
      case "force-push":
        return (
          <Button
            key={what}
            className="danger small"
            {...props}
            title={`Replace ${upstream} with ${branch}. Refused if someone else pushed to it since you last pulled.`}
          >
            <ArrowUp size={12} /> Force push
          </Button>
        );
      default:
        return null;
    }
  };
  return (
    <div className={`branch-bar ${tone}`} role="status" aria-label="Current branch">
      <span className="branch-bar-name" title={branch ? `You're on ${branch}` : "Detached HEAD"}>
        {branch ? <Branch size={14} /> : <Alert size={14} />}
        <span className="truncate">{branch ?? "detached HEAD"}</span>
      </span>
      <span className="branch-bar-text truncate">{text}</span>
      <span className="flex-1" />
      {remote !== null && (
        <Button
          className="ghost small"
          busy={busy === "fetch"}
          disabled={busy != null && busy !== "fetch"}
          onClick={() => run("fetch")}
          title="Fetch from all remotes, and bring branches that are only behind (like main) up to date"
        >
          <Refresh size={12} /> Fetch
        </Button>
      )}
      {shown.map(button)}
    </div>
  );
}
