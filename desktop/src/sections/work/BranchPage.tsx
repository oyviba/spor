import { useEffect, useState } from "react";
import { api } from "../../api";
import type { WorkBranch } from "../../types";
import { useShell } from "../../shared/app";
import { useChanged } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { DetailsCard, PageHeader, Tabs } from "../../shared/Layout";
import { DiffView } from "../../shared/DiffView";
import { StatusPill } from "../../shared/StatusPill";
import { Branch, Commit, History, PullRequest } from "../../shared/Icons";
import { ago } from "../agents/transcript";

type Tab = "commits" | "changes";

/** One of your own branches: its commits and changes since it left the
 * base, and what to do next (switch to it, open a pull request). */
export function BranchPage({ branch }: { branch: WorkBranch }) {
  const shell = useShell();
  const [tab, setTab] = useState<Tab>("commits");
  const [patch, setPatch] = useState<{ key: string; text: string } | null>(null);
  useChanged(() => setTab("commits"), [branch.name]);

  // Reloaded when the branch moves.
  const key = `${branch.name}:${branch.commits[0]?.hash ?? ""}`;
  useEffect(() => {
    if (tab !== "changes" || patch?.key === key) return;
    api.branchPatch(branch.name).then((text) => setPatch({ key, text }), shell.fail);
  }, [tab, key]); // eslint-disable-line react-hooks/exhaustive-deps

  const now = Date.now();
  const title = branch.commits[branch.commits.length - 1]?.subject ?? branch.name;

  return (
    <div className="task-page with-inspector branch-page">
      <PageHeader
        title={
          <>
            <h2 title={title}>{title}</h2>
            {branch.current && <StatusPill status={{ label: "Checked out", tone: "muted", live: false }} />}
          </>
        }
      >
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            { id: "commits", label: <>Commits <span className="tab-count num">{branch.commits.length}</span></> },
            { id: "changes", label: <>Changes <span className="tab-count num">{branch.files}</span></> },
          ]}
        />
      </PageHeader>
      <div className="task-body">
        {tab === "commits" ? (
          <div className="conversation">
            <div className="column">
              {branch.commits.length === 0 ? (
                <p className="hint">
                  No commits on {branch.name} yet. Commit in Changes and they show up here.
                </p>
              ) : (
                <ul className="check-list branch-commits">
                  {branch.commits.map((c) => (
                    <li key={c.hash}>
                      <Commit size={13} />
                      <button className="link truncate" onClick={() => shell.go({ section: "history", hash: c.hash })}>
                        {c.subject}
                      </button>
                      <span className="flex-1" />
                      <span className="muted small-text num">
                        {c.short} · {ago(c.time, now)}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        ) : (
          <div className="changes">
            <div className="changes-column">
              <DiffView patch={patch?.key === key ? patch.text : null} empty={`No changes on ${branch.name} yet.`} />
            </div>
          </div>
        )}
        <DetailsCard>
          <div className="inspector-section">
            <div className="inspector-row">
              <Branch size={13} />
              <span className="truncate">
                <code>{branch.name}</code> <span className="muted">→ {branch.base}</span>
              </span>
            </div>
            <div className="inspector-row">
              <span className="diffstat num">
                <span className="add">+{branch.additions}</span> <span className="del">−{branch.deletions}</span>
              </span>
              <span className="muted num">
                {branch.files} files · {branch.commits.length} commits
              </span>
            </div>
            <button className="inspector-row action" onClick={() => shell.go({ section: "history", branch: branch.name })}>
              <History size={13} />
              <span>Show in History</span>
            </button>
          </div>
          <div className="inspector-actions">
            {branch.commits.length > 0 && (
              <Button
                className="primary"
                title={`Open a pull request from ${branch.name} into ${branch.base} on the host`}
                onClick={() =>
                  api
                    .pullRequestBetween(branch.name, branch.base)
                    .then((url) => api.openUrl(url))
                    .catch(shell.fail)
                }
              >
                <PullRequest size={12} /> Open a pull request
              </Button>
            )}
            {!branch.current && (
              <Button
                className="ghost"
                onClick={() => shell.switchBranch((stash) => api.checkout(branch.name, stash))}
                title="Check it out here"
              >
                Switch to it
              </Button>
            )}
          </div>
        </DetailsCard>
      </div>
    </div>
  );
}
