import { useEffect, useState } from "react";
import { api } from "../../api";
import type { WorkBranch } from "../../types";
import { useShell } from "../../shared/app";
import { useChanged } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { DetailsCard } from "../../shared/Layout";
import { DiffView } from "../../shared/DiffView";
import { Bot, Commit, PullRequest } from "../../shared/Icons";
import { WorkCard } from "../../shared/WorkCard";
import { findWork, itemFromBranch } from "../../shared/work";
import { ago } from "../agents/transcript";
import { useAgentSettings } from "../agents/Composer";
import { WorkPage, shownTab, type WorkTab } from "./WorkPage";
import { RunChecks } from "./pull";
import { handToAgent } from "./handover";

/** One of your own branches on the work page (#80): its commits and
 * changes since it left the base, its Actions runs, and what to do next
 * (switch to it, open a pull request, or hand it to an agent). */
export function BranchPage({ branch }: { branch: WorkBranch }) {
  const shell = useShell();
  const [settings] = useAgentSettings();
  const [tab, setTab] = useState<WorkTab>("changes");
  const [patch, setPatch] = useState<{ key: string; text: string } | null>(null);
  useChanged(() => setTab("changes"), [branch.name]);

  // Reloaded when the branch moves.
  const key = `${branch.name}:${branch.commits[0]?.hash ?? ""}`;
  useEffect(() => {
    if (tab !== "changes" || patch?.key === key) return;
    api.branchPatch(branch.name).then((text) => setPatch({ key, text }), shell.fail);
  }, [tab, key]); // eslint-disable-line react-hooks/exhaustive-deps

  const now = Date.now();
  const item = findWork(shell.work, { branch: branch.name }) ?? { ...itemFromBranch(branch.name), base: branch.base };
  const title = branch.commits[branch.commits.length - 1]?.subject ?? branch.name;
  const tabs: { id: WorkTab; extra?: React.ReactNode }[] = [
    { id: "changes", extra: <span className="tab-count num">{branch.files}</span> },
    ...(item.checks ? [{ id: "checks" as const }] : []),
  ];
  const shown = shownTab(tabs, tab);

  return (
    <WorkPage
      item={item}
      title={title}
      hint={title}
      status={branch.current ? { label: "Checked out", tone: "muted", live: false } : null}
      action={
        <Button
          className="ghost"
          onClick={() => handToAgent(shell, settings, branch.name, branch.base)}
          title={
            branch.current
              ? `Move ${branch.name} and your uncommitted changes to its own folder, with an agent`
              : `Give ${branch.name} its own folder, with an agent`
          }
        >
          <Bot size={12} /> Start an agent on this
        </Button>
      }
      tabs={tabs}
      tab={shown}
      onTab={setTab}
      inspector
      details={
        <DetailsCard>
          <WorkCard item={item} from="work" />
          <div className="inspector-section">
            <div className="inspector-row">
              <span className="diffstat num">
                <span className="add">+{branch.additions}</span> <span className="del">−{branch.deletions}</span>
              </span>
              <span className="muted num">
                {branch.files} files · {branch.commits.length} commits
              </span>
            </div>
          </div>
          <div className="inspector-actions">
            {branch.commits.length > 0 && (
              <Button
                className="ghost"
                title={`GitHub's page for a pull request from ${branch.name} into ${branch.base}`}
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
      }
    >
      {shown === "changes" ? (
        <div className="changes">
          <div className="changes-column">
            {branch.commits.length === 0 ? (
              <p className="hint">No commits on {branch.name} yet. Commit in Changes and they show up here.</p>
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
            <DiffView patch={patch?.key === key ? patch.text : null} empty={`No changes on ${branch.name} yet.`} />
          </div>
        </div>
      ) : (
        <div className="conversation">
          <div className="column">
            <RunChecks branch={branch.name} />
          </div>
        </div>
      )}
    </WorkPage>
  );
}
