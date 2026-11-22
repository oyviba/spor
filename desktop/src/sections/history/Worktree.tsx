import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api";
import type { FileEntry, StashInfo } from "../../types";
import { useShell } from "../../shared/app";
import { useChanged, useRepoChanges } from "../../shared/hooks";
import { Button } from "../../shared/Button";
import { DetailsCard, EmptyState } from "../../shared/Layout";
import {
  DiffView,
  selectionRange,
  type LineSelection,
} from "../../shared/DiffView";
import { parsePatch } from "../../shared/diff";
import { ConflictRegions } from "./Conflicts";
import {
  Alert,
  Archive,
  ArrowDown,
  ArrowUp,
  Bot,
  Branch,
  Check,
  Minus,
  Plus,
  Refresh,
  Undo,
} from "../../shared/Icons";
import { WorkCard } from "../../shared/WorkCard";
import { findWork } from "../../shared/work";
import { WorkHeader } from "../work/WorkPage";
import { handToAgent } from "../work/handover";
import { useAgentSettings } from "../agents/Composer";

// The working tree in History: an "Uncommitted changes" row above the
// graph, and, once it's selected, its files, the commit box, sync and
// stashes in the panel on the right (where a commit's details would be)
// and the picked file's diff in the page (where a commit's diff would be).

/** What `selected` is in History when it's the working tree, not a commit. */
export const WORKING = "working-tree";

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

/** The working tree: its files, the file picked and its diff, and the line
 * selection in it. Lives in History so the row, the panel and the diff
 * share it. */
export function useWorktree() {
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
  // Stashes are refs; staging is the index; editing is the working tree.
  useRepoChanges(load, ["refs", "index", "worktree"], []);

  const conflicted = files?.filter((f) => f.status === "conflicted") ?? [];
  const staged = files?.filter((f) => f.staged) ?? [];
  const unstaged =
    files?.filter((f) => !f.staged && f.status !== "conflicted") ?? [];

  const entry = files?.find(
    (f) =>
      pick &&
      f.path === pick.path &&
      (f.status === "conflicted" || f.staged === pick.staged),
  );
  // Keep a pick that still exists (maybe on the other side of the index);
  // an empty pick stays empty until a file is chosen, so a commit's diff
  // isn't replaced by one nobody asked for.
  useEffect(() => {
    if (!files || !pick || entry) return;
    const same = files.find((f) => f.path === pick.path);
    setPick(same ? { path: same.path, staged: same.staged } : null);
  }, [files]); // eslint-disable-line react-hooks/exhaustive-deps

  // The picked file's diff, when it or anything about it changes.
  const filesKey = JSON.stringify(files);
  useChanged(() => {
    setSelection(null);
    if (!pick || !entry) return setPatch(null);
    api.fileDiff(pick.path, pick.staged).then(setPatch, (e) => {
      setPatch("");
      fail(e);
    });
  }, [pick?.path, pick?.staged, filesKey]);

  const after = () => load().then(shell.refresh);
  const both = pick
    ? files?.filter((f) => f.path === pick.path && f.status !== "conflicted")
        .length === 2
    : false;

  const stageSelection = (discard: boolean) => {
    if (!pick || !selection || !patch) return;
    const range = selectionRange(patch, selection);
    if (!range) return;
    return shell
      .act(() =>
        api
          .stageLines(pick.path, pick.staged, discard, range.from, range.to)
          .then(() => undefined),
      )
      .then(after);
  };
  // Changed lines picked (context lines in the range don't count).
  const nSelected = useMemo(() => {
    if (!selection || !patch) return 0;
    const file = parsePatch(patch).find((f) => f.path === selection.file);
    return (
      file?.lines
        .slice(selection.start, selection.end + 1)
        .filter((l) => l.kind === "add" || l.kind === "del").length ?? 0
    );
  }, [selection, patch]);

  return {
    files,
    stashes,
    pick,
    setPick,
    entry,
    patch,
    selection,
    setSelection,
    nSelected,
    both,
    filesKey,
    conflicted,
    staged,
    unstaged,
    after,
    stageSelection,
    operation: state?.operation ?? null,
  };
}
export type Worktree = ReturnType<typeof useWorktree>;

/** The row above the graph: selecting it shows the working tree. */
export function WorktreeRow({
  w,
  selected,
  onSelect,
}: {
  w: Worktree;
  selected: boolean;
  onSelect: () => void;
}) {
  const n = w.files ? new Set(w.files.map((f) => f.path)).size : 0;
  return (
    <button
      className={`worktree-row${selected ? " selected" : ""}`}
      onClick={onSelect}
      aria-pressed={selected}
    >
      <span className={`worktree-dot${n > 0 ? " dirty" : ""}`} />
      <span className="worktree-title">
        {n > 0 ? "Uncommitted changes" : "Working tree clean"}
      </span>
      {w.conflicted.length > 0 && (
        <span className="muted-badge bad">
          {w.conflicted.length}{" "}
          {w.conflicted.length === 1 ? "conflict" : "conflicts"}
        </span>
      )}
      <span className="flex-1" />
      {n > 0 && (
        <span className="num muted small-text">
          {n} {n === 1 ? "file" : "files"}
          {w.staged.length > 0 && ` · ${w.staged.length} staged`}
        </span>
      )}
    </button>
  );
}

/** The panel on the right: files (staged, changes, conflicts), the commit
 * box, syncing and stashes. */
export function WorktreePanel({
  w,
  onOpenFile,
}: {
  w: Worktree;
  onOpenFile: () => void;
}) {
  const shell = useShell();
  const { files, pick, conflicted, staged, unstaged, after } = w;
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
            selected={
              !!pick &&
              pick.path === f.path &&
              (f.status === "conflicted" || pick.staged === f.staged)
            }
            onPick={() => {
              w.setPick({ path: f.path, staged: f.staged });
              onOpenFile();
            }}
            onToggle={() =>
              shell
                .act(() =>
                  (f.staged ? api.unstage([f.path]) : api.stage([f.path])).then(
                    () => undefined,
                  ),
                )
                .then(after)
            }
            onDiscard={
              f.staged || f.status === "conflicted"
                ? undefined
                : () =>
                    shell
                      .act(() =>
                        api
                          .discard([f.path])
                          .then(() => `Discarded changes to ${f.path}`),
                      )
                      .then(after)
            }
          />
        ))}
      </>
    );
  return (
    <DetailsCard wide>
      <div className="inspector-files">
        {files && files.length === 0 && (
          <div className="sidebar-empty">
            Nothing to commit — the working tree is clean.
          </div>
        )}
        {group("Conflicts", conflicted, null)}
        {group(
          "Staged",
          staged,
          <Button
            className="link small-text"
            onClick={() =>
              shell
                .act(() =>
                  api.unstage(staged.map((f) => f.path)).then(() => undefined),
                )
                .then(after)
            }
          >
            Unstage all
          </Button>,
        )}
        {group(
          "Changes",
          unstaged,
          <Button
            className="link small-text"
            onClick={() =>
              shell
                .act(() =>
                  api.stage(unstaged.map((f) => f.path)).then(() => undefined),
                )
                .then(after)
            }
          >
            Stage all
          </Button>,
        )}
      </div>
      <CommitBox
        staged={staged.length}
        conflicts={conflicted.length > 0}
        onCommitted={after}
      />
      <SyncCard
        stashes={w.stashes}
        onChanged={after}
        canStash={(files?.length ?? 0) > 0}
      />
    </DetailsCard>
  );
}

/** The picked file's diff, with staging by line, or a conflicted file's
 * regions. */
export function WorktreeDiff({ w }: { w: Worktree }) {
  const shell = useShell();
  const { files, pick, entry, patch, selection, nSelected, after, filesKey } =
    w;
  if (!pick || !entry)
    return files && files.length === 0 ? (
      <EmptyState title="All committed">
        Edit files and their changes show up here.
      </EmptyState>
    ) : (
      <p className="hint">Pick a file.</p>
    );
  return (
    <>
      {entry.status === "conflicted" && (
        <div className="resolve-bar">
          <Alert size={14} />
          <span>Both sides changed this file.</span>
          <span className="flex-1" />
          <Button
            className="ghost small"
            onClick={() =>
              shell
                .act(
                  () => api.resolve(pick.path, "ours").then(() => undefined),
                  { undo: false },
                )
                .then(after)
            }
          >
            Take ours
          </Button>
          <Button
            className="ghost small"
            onClick={() =>
              shell
                .act(
                  () => api.resolve(pick.path, "theirs").then(() => undefined),
                  { undo: false },
                )
                .then(after)
            }
          >
            Take theirs
          </Button>
          <Button
            className="primary small"
            title="After editing the file to keep what you want"
            onClick={() =>
              shell
                .act(
                  () =>
                    api.resolve(pick.path, "resolved").then(() => undefined),
                  { undo: false },
                )
                .then(after)
            }
          >
            <Check size={12} /> Mark resolved
          </Button>
        </div>
      )}
      {entry.status === "conflicted" ? (
        <ConflictRegions
          key={`${pick.path}:${filesKey}`}
          path={pick.path}
          operation={w.operation}
          onSave={(choices) =>
            shell
              .act(() => api.resolveRegions(pick.path, choices), {
                undo: false,
              })
              .then(after)
          }
        />
      ) : (
        <DiffView
          bare
          patch={patch}
          empty={
            entry.status === "untracked" ? "An empty new file." : "No changes."
          }
          selection={selection}
          onSelect={w.setSelection}
          selectionBar={
            nSelected > 0 && (
              <div className="selection-bar">
                <span className="num">
                  {nSelected} {nSelected === 1 ? "line" : "lines"} picked
                </span>
                <span className="muted small-text">shift-click to extend</span>
                <span className="flex-1" />
                {pick.staged ? (
                  <Button
                    className="primary small"
                    onClick={() => w.stageSelection(false)}
                  >
                    <Minus size={11} /> Unstage lines
                  </Button>
                ) : (
                  <>
                    <Button
                      className="ghost small"
                      onClick={() => w.stageSelection(true)}
                    >
                      <Undo size={11} /> Discard lines
                    </Button>
                    <Button
                      className="primary small"
                      onClick={() => w.stageSelection(false)}
                    >
                      <Plus size={11} /> Stage lines
                    </Button>
                  </>
                )}
              </div>
            )
          }
        />
      )}
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
  onToggle: () => Promise<unknown>;
  onDiscard?: () => Promise<unknown>;
}) {
  const slash = file.path.lastIndexOf("/");
  const name = file.path.slice(slash + 1);
  const dir = slash >= 0 ? file.path.slice(0, slash) : "";
  return (
    <div className={`file-row${selected ? " selected" : ""}`}>
      <button className="file-row-main" onClick={onPick} title={file.path}>
        <span className={`file-letter s-${file.status}`}>
          {LETTER[file.status]}
        </span>
        <span className="file-row-name truncate">{name}</span>
        <span className="file-row-dir truncate">{dir}</span>
      </button>
      <span className="row-actions">
        {onDiscard && (
          <Button
            className="icon-button tiny"
            title="Discard changes (⌘Z undoes)"
            onClick={onDiscard}
          >
            <Undo size={11} />
          </Button>
        )}
        <Button
          className="icon-button tiny"
          title={
            file.staged
              ? "Unstage"
              : file.status === "conflicted"
                ? "Mark resolved"
                : "Stage"
          }
          onClick={onToggle}
        >
          {file.staged ? <Minus size={11} /> : <Plus size={11} />}
        </Button>
      </span>
    </div>
  );
}

/** The checked-out branch as the work item it is (#82): its stage, who
 * it's waiting on, and handing it to an agent. On the base, or another
 * branch with nothing on it, there's no item and nothing shows. */
export function CheckoutHeader() {
  const shell = useShell();
  const [settings] = useAgentSettings();
  const branch = shell.state?.branch;
  const item = branch ? findWork(shell.work, { branch }) : undefined;
  if (!item || !branch || item.task) return null;
  const base = item.base ?? shell.repo.default_base ?? "main";
  return (
    <WorkHeader
      item={item}
      title={item.title}
      hint={branch}
      action={
        <Button
          className="ghost"
          onClick={() => handToAgent(shell, settings, branch, base)}
          disabled={!!shell.state?.operation}
          title={`Move ${branch} and your uncommitted changes to their own folder with an agent; you go back to ${base} (⌘Z undoes)`}
        >
          <Bot size={12} /> Hand to an agent
        </Button>
      }
    />
  );
}

function SyncCard({
  stashes,
  onChanged,
  canStash,
}: {
  stashes: StashInfo[];
  onChanged: () => void;
  canStash: boolean;
}) {
  const shell = useShell();
  const { state } = shell;
  const item = state?.branch
    ? findWork(shell.work, { branch: state.branch })
    : undefined;
  const [busy, setBusy] = useState<string | null>(null);
  const run = (what: "fetch" | "pull" | "push") => {
    setBusy(what);
    const pending = {
      fetch: "Fetching…",
      pull: "Pulling…",
      push: state?.upstream ? "Pushing…" : "Publishing…",
    }[what];
    return shell
      .act(() => api.sync(what), { undo: what === "pull", pending })
      .finally(() => {
        setBusy(null);
        onChanged();
      });
  };
  // One sync at a time: the others wait (dimmed) while one runs.
  const syncButton = (what: "fetch" | "pull" | "push") => ({
    busy: busy === what,
    disabled: busy != null && busy !== what,
    onClick: () => run(what),
  });
  return (
    <DetailsCard>
      {item && <WorkCard item={item} from="history" />}
      <div className="inspector-section">
        <div className="inspector-row">
          <Branch size={13} />
          <span className="truncate">
            {!item && <b>{state?.branch ?? "detached HEAD"}</b>}
            {state?.upstream ? (
              <span className="muted">
                {item ? "Tracks " : " → "}
                {state.upstream}
              </span>
            ) : (
              <span className="muted">
                {item ? "Not published" : " · not published"}
              </span>
            )}
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
        <Button
          className="ghost"
          {...syncButton("fetch")}
          title="Fetch from all remotes"
        >
          <Refresh size={12} /> Fetch
        </Button>
        <Button
          className={state && state.behind > 0 ? "primary" : "ghost"}
          {...syncButton("pull")}
          disabled={(busy != null && busy !== "pull") || !state?.upstream}
          title={
            state?.upstream
              ? `Pull from ${state.upstream}, with rebase`
              : "Publish the branch first"
          }
        >
          <ArrowDown size={12} /> Pull
          {state && state.behind > 0 ? ` ${state.behind}` : ""}
        </Button>
        <Button
          className={state && state.ahead > 0 ? "primary" : "ghost"}
          {...syncButton("push")}
          title={
            state?.upstream
              ? `Push to ${state.upstream}`
              : "Publish the branch to origin"
          }
        >
          <ArrowUp size={12} />{" "}
          {state?.upstream
            ? `Push${state.ahead > 0 ? ` ${state.ahead}` : ""}`
            : "Publish"}
        </Button>
      </div>
      {state?.branch &&
        state.upstream &&
        state.branch !== shell.repo.default_base &&
        !item?.pull && (
          <div className="inspector-actions">
            <Button
              className="ghost"
              title="GitHub's page to open a pull request for this branch"
              onClick={() =>
                api.pullRequestUrl().then(api.openUrl).catch(shell.fail)
              }
            >
              Open a pull request
            </Button>
          </div>
        )}
      <div className="inspector-section">
        <div className="inspector-row">
          <Archive size={13} />
          <span>Stashes</span>
          <span className="flex-1" />
          <Button
            className="link small-text"
            disabled={!canStash}
            title={
              canStash ? "Right-click to give it a message" : "Nothing to stash"
            }
            onClick={() =>
              shell
                .act(() => api.stash(null), {
                  undo: false,
                  pending: "Stashing…",
                })
                .then(onChanged)
            }
            onContextMenu={(e) =>
              shell.menu(e, [
                {
                  label: "Stash changes with a message…",
                  disabled: !canStash,
                  run: async () => {
                    const r = await shell.prompt({
                      title: "Stash changes",
                      placeholder: "What's in it",
                      action: "Stash",
                    });
                    if (r)
                      shell
                        .act(() => api.stash(r.value), { undo: false })
                        .then(onChanged);
                  },
                },
              ])
            }
          >
            Stash changes
          </Button>
        </div>
        {stashes.map((s) => (
          <div
            key={s.index}
            className="inspector-row stash-row"
            onContextMenu={(e) =>
              shell.menu(e, [
                {
                  label: "Pop (apply and drop)",
                  run: () =>
                    shell
                      .act(() => api.stashPop(s.index), { undo: false })
                      .then(onChanged),
                },
                {
                  label: "Apply and keep",
                  run: () =>
                    shell
                      .act(() => api.stashApply(s.index), { undo: false })
                      .then(onChanged),
                },
                {
                  label: "Drop",
                  danger: true,
                  separator: true,
                  run: () =>
                    shell.act(() => api.stashDrop(s.index)).then(onChanged),
                },
              ])
            }
          >
            <span
              className="truncate muted"
              title={`${s.message} — right-click for more`}
            >
              {s.message}
            </span>
            <Button
              className="link small-text"
              title="Apply it and drop it from the list"
              onClick={() =>
                shell
                  .act(() => api.stashPop(s.index), { undo: false })
                  .then(onChanged)
              }
            >
              Pop
            </Button>
            <Button
              className="link small-text"
              title="Apply it and keep it in the list"
              onClick={() =>
                shell
                  .act(() => api.stashApply(s.index), { undo: false })
                  .then(onChanged)
              }
            >
              Apply
            </Button>
            <Button
              className="link small-text"
              title="Drop it (⌘Z undoes)"
              onClick={() =>
                shell.act(() => api.stashDrop(s.index)).then(onChanged)
              }
            >
              Drop
            </Button>
          </div>
        ))}
      </div>
    </DetailsCard>
  );
}

function CommitBox({
  staged,
  conflicts,
  onCommitted,
}: {
  staged: number;
  conflicts: boolean;
  onCommitted: () => void;
}) {
  const shell = useShell();
  const [message, setMessage] = useState("");
  const [amend, setAmend] = useState(false);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const ready =
    message.trim().length > 0 && (staged > 0 || amend) && !conflicts;
  const can = ready && !busy;
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
    <div className="inspector-commit">
      <div className="composer">
        <textarea
          ref={input}
          value={message}
          rows={2}
          placeholder={
            conflicts
              ? "Resolve the conflicts first"
              : staged > 0
                ? "Commit message — the first line is the subject"
                : "Stage files to commit them"
          }
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
              if (next && !message.trim())
                setMessage(await api.lastMessage().catch(() => ""));
            }}
            title="Fold the staged changes into the last commit, with this message"
          >
            Amend last commit
          </button>
          <span className="flex-1" />
          <span className="hint-keys">⌘↩ commit</span>
          <Button
            className="primary"
            disabled={!ready}
            busy={busy}
            onClick={submit}
          >
            {amend
              ? "Amend"
              : `Commit${staged > 0 ? ` ${staged} ${staged === 1 ? "file" : "files"}` : ""}`}
          </Button>
        </div>
      </div>
    </div>
  );
}
