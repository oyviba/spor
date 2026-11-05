import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api";
import type { FileEntry, StashInfo } from "../../types";
import { useShell } from "../../shared/app";
import { usePoll } from "../../shared/hooks";
import { DetailsCard, EmptyState, PageHeader, SidebarHeader, Tabs } from "../../shared/Layout";
import { DiffView, selectionRange, type LineSelection } from "../../shared/DiffView";
import { parsePatch } from "../../shared/diff";
import { Alert, Archive, ArrowDown, ArrowUp, Branch, Check, Minus, Plus, PullRequest, Refresh, Undo } from "../../shared/Icons";

interface Pick {
  path: string;
  staged: boolean;
}

const LETTER: Record<FileEntry["status"], string> = {
  staged: "M",
  stageddeleted: "D",
  modified: "M",
  deleted: "D",
  untracked: "U",
  conflicted: "!",
};

/** Changes: what's different in the working tree and the index, staging
 * (whole files or picked lines), committing, and syncing. */
export function ChangesSection() {
  const shell = useShell();
  const { state, fail } = shell;
  const [files, setFiles] = useState<FileEntry[] | null>(null);
  const [pick, setPick] = useState<Pick | null>(null);
  const [patch, setPatch] = useState<string | null>(null);
  const [selection, setSelection] = useState<LineSelection | null>(null);
  const [stashes, setStashes] = useState<StashInfo[]>([]);

  const load = async () => {
    const [f, s] = await Promise.all([api.status(), api.stashes()]);
    setFiles((old) => (JSON.stringify(old) === JSON.stringify(f) ? old : f));
    setStashes((old) => (JSON.stringify(old) === JSON.stringify(s) ? old : s));
  };
  // The working tree changes without git knowing: poll it.
  usePoll(load, 2000, [state?.index]);

  // Sent here (conflict banner): open that file.
  useEffect(() => {
    if (shell.target.section === "changes" && shell.target.path) setPick({ path: shell.target.path, staged: false });
  }, [shell.target.seq]); // eslint-disable-line react-hooks/exhaustive-deps

  const conflicted = files?.filter((f) => f.status === "conflicted") ?? [];
  const staged = files?.filter((f) => f.staged) ?? [];
  const unstaged = files?.filter((f) => !f.staged && f.status !== "conflicted") ?? [];

  // Keep a pick that still exists; else the first file there is.
  const entry = files?.find((f) => pick && f.path === pick.path && (f.status === "conflicted" || f.staged === pick.staged));
  useEffect(() => {
    if (!files) return;
    if (pick && !entry) {
      const same = files.find((f) => f.path === pick.path);
      setPick(same ? { path: same.path, staged: same.staged } : null);
    } else if (!pick && files.length > 0) {
      const first = conflicted[0] ?? unstaged[0] ?? staged[0];
      setPick({ path: first.path, staged: first.staged });
    }
  }, [files]); // eslint-disable-line react-hooks/exhaustive-deps

  // The picked file's diff, when it or anything about it changes.
  const filesKey = JSON.stringify(files);
  useEffect(() => {
    setSelection(null);
    if (!pick || !entry) return setPatch(null);
    api.fileDiff(pick.path, pick.staged).then(setPatch, (e) => {
      setPatch("");
      fail(e);
    });
  }, [pick?.path, pick?.staged, filesKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const after = () => load().then(shell.refresh);
  const both = pick ? files?.filter((f) => f.path === pick.path && f.status !== "conflicted").length === 2 : false;

  const stageSelection = (discard: boolean) => {
    if (!pick || !selection || !patch) return;
    const range = selectionRange(patch, selection);
    if (!range) return;
    shell
      .act(() => api.stageLines(pick.path, pick.staged, discard, range.from, range.to).then(() => undefined))
      .then(after);
  };
  // Changed lines picked (context lines in the range don't count).
  const nSelected = useMemo(() => {
    if (!selection || !patch) return 0;
    const file = parsePatch(patch).find((f) => f.path === selection.file);
    return file?.lines.slice(selection.start, selection.end + 1).filter((l) => l.kind === "add" || l.kind === "del").length ?? 0;
  }, [selection, patch]);

  const group = (title: string, list: FileEntry[], action: React.ReactNode) =>
    list.length > 0 && (
      <>
        <div className="list-label with-action">
          <span>
            {title} <span className="num muted">{list.length}</span>
          </span>
          {action}
        </div>
        {list.map((f) => (
          <FileRow
            key={`${f.staged}:${f.path}`}
            file={f}
            selected={!!pick && pick.path === f.path && (f.status === "conflicted" || pick.staged === f.staged)}
            onPick={() => setPick({ path: f.path, staged: f.staged })}
            onToggle={() =>
              shell
                .act(() => (f.staged ? api.unstage([f.path]) : api.stage([f.path])).then(() => undefined))
                .then(after)
            }
            onDiscard={
              f.staged || f.status === "conflicted"
                ? undefined
                : () => shell.act(() => api.discard([f.path]).then(() => `Discarded changes to ${f.path}`)).then(after)
            }
          />
        ))}
      </>
    );

  return (
    <>
      <aside className="sidebar">
        <SidebarHeader />
        <div className="task-list">
          {files && files.length === 0 && <div className="sidebar-empty">Nothing to commit — the working tree is clean.</div>}
          {group("Conflicts", conflicted, null)}
          {group(
            "Staged",
            staged,
            <button className="link small-text" onClick={() => shell.act(() => api.unstage(staged.map((f) => f.path)).then(() => undefined)).then(after)}>
              Unstage all
            </button>,
          )}
          {group(
            "Changes",
            unstaged,
            <button className="link small-text" onClick={() => shell.act(() => api.stage(unstaged.map((f) => f.path)).then(() => undefined)).then(after)}>
              Stage all
            </button>,
          )}
        </div>
      </aside>
      <main className="main">
        <div className="task-page with-inspector">
          <PageHeader
            title={
              pick ? (
                <h2 className="mono truncate" title={pick.path}>
                  {pick.path}
                </h2>
              ) : (
                <h2>Changes</h2>
              )
            }
          >
            {pick && entry && entry.status !== "untracked" && entry.status !== "deleted" && entry.status !== "stageddeleted" && (
              <button
                className="ghost small"
                title="Who last changed each line (the working tree, uncommitted lines included)"
                onClick={() => shell.go({ section: "history", blame: { path: pick.path, rev: null } })}
              >
                Blame
              </button>
            )}
            {pick && entry?.status !== "conflicted" && (both || pick.staged) && (
              <Tabs
                value={pick.staged ? "staged" : "unstaged"}
                onChange={(v) => setPick({ path: pick.path, staged: v === "staged" })}
                tabs={[
                  { id: "unstaged", label: "Unstaged" },
                  { id: "staged", label: "Staged" },
                ]}
              />
            )}
          </PageHeader>
          <div className="task-body">
            <div className="changes">
              <div className="changes-column">
                {!pick || !entry ? (
                  files && files.length === 0 ? (
                    <EmptyState title="All committed">Edit files and their changes show up here.</EmptyState>
                  ) : (
                    <p className="hint">Pick a file.</p>
                  )
                ) : (
                  <>
                    {entry.status === "conflicted" && (
                      <div className="resolve-bar">
                        <Alert size={14} />
                        <span>Both sides changed this file.</span>
                        <span className="flex-1" />
                        <button className="ghost small" onClick={() => shell.act(() => api.resolve(pick.path, "ours").then(() => undefined), { undo: false }).then(after)}>
                          Take ours
                        </button>
                        <button className="ghost small" onClick={() => shell.act(() => api.resolve(pick.path, "theirs").then(() => undefined), { undo: false }).then(after)}>
                          Take theirs
                        </button>
                        <button
                          className="primary small"
                          title="After editing the file to keep what you want"
                          onClick={() => shell.act(() => api.resolve(pick.path, "resolved").then(() => undefined), { undo: false }).then(after)}
                        >
                          <Check size={12} /> Mark resolved
                        </button>
                      </div>
                    )}
                    <DiffView
                      bare
                      patch={patch}
                      empty={entry.status === "untracked" ? "An empty new file." : "No changes."}
                      selection={entry.status === "conflicted" ? null : selection}
                      onSelect={entry.status === "conflicted" ? undefined : setSelection}
                      selectionBar={
                        nSelected > 0 && (
                          <div className="selection-bar">
                            <span className="num">
                              {nSelected} {nSelected === 1 ? "line" : "lines"} picked
                            </span>
                            <span className="muted small-text">shift-click to extend</span>
                            <span className="flex-1" />
                            {pick.staged ? (
                              <button className="primary small" onClick={() => stageSelection(false)}>
                                <Minus size={11} /> Unstage lines
                              </button>
                            ) : (
                              <>
                                <button className="ghost small" onClick={() => stageSelection(true)}>
                                  <Undo size={11} /> Discard lines
                                </button>
                                <button className="primary small" onClick={() => stageSelection(false)}>
                                  <Plus size={11} /> Stage lines
                                </button>
                              </>
                            )}
                          </div>
                        )
                      }
                    />
                  </>
                )}
              </div>
            </div>
            <SyncCard stashes={stashes} onChanged={after} canStash={(files?.length ?? 0) > 0} />
          </div>
          <CommitBox staged={staged.length} conflicts={conflicted.length > 0} onCommitted={after} />
        </div>
      </main>
    </>
  );
}

function FileRow({
  file,
  selected,
  onPick,
  onToggle,
  onDiscard,
}: {
  file: FileEntry;
  selected: boolean;
  onPick: () => void;
  onToggle: () => void;
  onDiscard?: () => void;
}) {
  const slash = file.path.lastIndexOf("/");
  const name = file.path.slice(slash + 1);
  const dir = slash >= 0 ? file.path.slice(0, slash) : "";
  return (
    <div className={`file-row${selected ? " selected" : ""}`}>
      <button className="file-row-main" onClick={onPick} title={file.path}>
        <span className={`file-letter s-${file.status}`}>{LETTER[file.status]}</span>
        <span className="file-row-name truncate">{name}</span>
        <span className="file-row-dir truncate">{dir}</span>
      </button>
      <span className="row-actions">
        {onDiscard && (
          <button className="icon-button tiny" title="Discard changes (⌘Z undoes)" onClick={onDiscard}>
            <Undo size={11} />
          </button>
        )}
        <button className="icon-button tiny" title={file.staged ? "Unstage" : file.status === "conflicted" ? "Mark resolved" : "Stage"} onClick={onToggle}>
          {file.staged ? <Minus size={11} /> : <Plus size={11} />}
        </button>
      </span>
    </div>
  );
}

function SyncCard({ stashes, onChanged, canStash }: { stashes: StashInfo[]; onChanged: () => void; canStash: boolean }) {
  const shell = useShell();
  const { state } = shell;
  const [busy, setBusy] = useState<string | null>(null);
  const run = (what: "fetch" | "pull" | "push") => {
    setBusy(what);
    shell.act(() => api.sync(what), { undo: what === "pull" }).finally(() => {
      setBusy(null);
      onChanged();
    });
  };
  return (
    <DetailsCard>
      <div className="inspector-section">
        <div className="inspector-row">
          <Branch size={13} />
          <span className="truncate">
            <b>{state?.branch ?? "detached HEAD"}</b>
            {state?.upstream ? <span className="muted"> → {state.upstream}</span> : <span className="muted"> · not published</span>}
          </span>
        </div>
        {state?.upstream && (
          <div className="inspector-row">
            <span className="num muted">
              {state.ahead} to push · {state.behind} to pull
            </span>
          </div>
        )}
      </div>
      <div className="inspector-actions">
        <button className="ghost" disabled={!!busy} onClick={() => run("fetch")}>
          <Refresh size={12} className={busy === "fetch" ? "spin" : ""} /> Fetch
        </button>
        <button className={state && state.behind > 0 ? "primary" : "ghost"} disabled={!!busy || !state?.upstream} onClick={() => run("pull")}>
          <ArrowDown size={12} /> Pull{state && state.behind > 0 ? ` ${state.behind}` : ""}
        </button>
        <button className={state && state.ahead > 0 ? "primary" : "ghost"} disabled={!!busy} onClick={() => run("push")} title={state?.upstream ? "Push" : "Publish the branch to origin"}>
          <ArrowUp size={12} /> {state?.upstream ? `Push${state.ahead > 0 ? ` ${state.ahead}` : ""}` : "Publish"}
        </button>
      </div>
      {state?.branch && state.upstream && state.branch !== shell.repo.default_base && (
        <div className="inspector-actions">
          <button
            className="ghost"
            title="This branch's pull request, or the page to open one"
            onClick={() => api.pullRequestUrl().then(api.openUrl).catch(shell.fail)}
          >
            <PullRequest size={12} /> Pull request on GitHub
          </button>
        </div>
      )}
      <div className="inspector-section">
        <div className="inspector-row">
          <Archive size={13} />
          <span>Stashes</span>
          <span className="flex-1" />
          <button
            className="link small-text"
            disabled={!canStash}
            title="Right-click to give it a message"
            onClick={() => shell.act(() => api.stash(null), { undo: false }).then(onChanged)}
            onContextMenu={(e) =>
              shell.menu(e, [
                {
                  label: "Stash changes with a message…",
                  disabled: !canStash,
                  run: async () => {
                    const r = await shell.prompt({ title: "Stash changes", placeholder: "What's in it", action: "Stash" });
                    if (r) shell.act(() => api.stash(r.value), { undo: false }).then(onChanged);
                  },
                },
              ])
            }
          >
            Stash changes
          </button>
        </div>
        {stashes.map((s) => (
          <div
            key={s.index}
            className="inspector-row stash-row"
            onContextMenu={(e) =>
              shell.menu(e, [
                { label: "Pop (apply and drop)", run: () => shell.act(() => api.stashPop(s.index), { undo: false }).then(onChanged) },
                { label: "Apply and keep", run: () => shell.act(() => api.stashApply(s.index), { undo: false }).then(onChanged) },
                { label: "Drop", danger: true, separator: true, run: () => shell.act(() => api.stashDrop(s.index)).then(onChanged) },
              ])
            }
          >
            <span className="truncate muted" title={`${s.message} — right-click for more`}>
              {s.message}
            </span>
            <button className="link small-text" onClick={() => shell.act(() => api.stashPop(s.index), { undo: false }).then(onChanged)}>
              Pop
            </button>
            <button className="link small-text" title="Apply it and keep it in the list" onClick={() => shell.act(() => api.stashApply(s.index), { undo: false }).then(onChanged)}>
              Apply
            </button>
            <button className="link small-text" title="Drop it (⌘Z undoes)" onClick={() => shell.act(() => api.stashDrop(s.index)).then(onChanged)}>
              Drop
            </button>
          </div>
        ))}
      </div>
    </DetailsCard>
  );
}

function CommitBox({ staged, conflicts, onCommitted }: { staged: number; conflicts: boolean; onCommitted: () => void }) {
  const shell = useShell();
  const [message, setMessage] = useState("");
  const [amend, setAmend] = useState(false);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const can = message.trim().length > 0 && (staged > 0 || amend) && !conflicts && !busy;
  const submit = async () => {
    if (!can) return;
    setBusy(true);
    const ok = await shell.act(() => api.commit(message, amend));
    setBusy(false);
    if (ok) {
      setMessage("");
      setAmend(false);
      onCommitted();
    }
  };
  return (
    <div className="composer-dock">
      <div className="column">
        <div className="composer">
          <textarea
            ref={input}
            value={message}
            rows={2}
            placeholder={conflicts ? "Resolve the conflicts first" : staged > 0 ? "Commit message — the first line is the subject" : "Stage files to commit them"}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                submit();
              }
            }}
          />
          <div className="composer-footer">
            <button
              className={`chip-toggle${amend ? " on" : ""}`}
              onClick={async () => {
                const next = !amend;
                setAmend(next);
                if (next && !message.trim()) setMessage(await api.lastMessage().catch(() => ""));
              }}
              title="Fold the staged changes into the last commit, with this message"
            >
              Amend last commit
            </button>
            <span className="flex-1" />
            <span className="hint-keys">⌘↩ commit</span>
            <button className="primary" disabled={!can} onClick={submit}>
              {amend ? "Amend" : `Commit${staged > 0 ? ` ${staged} ${staged === 1 ? "file" : "files"}` : ""}`}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
