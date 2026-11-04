import type {
  AgentChoice,
  BranchInfo,
  CommitDetail,
  FileEntry,
  Graph,
  LogLine,
  Mode,
  Pull,
  PullDetail,
  RepoInfo,
  RepoState,
  Run,
  RunDetail,
  StashInfo,
  RunPatch,
  TaskDetail,
  TaskSummary,
} from "./types";
import type { Place } from "./shared/DiffView";

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

/** True inside the Tauri window; false in a plain browser (`pnpm dev`). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let invokeImpl: Invoke | null = null;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!invokeImpl) {
    invokeImpl = inTauri
      ? (await import("@tauri-apps/api/core")).invoke
      : (await import("./mock")).invoke;
  }
  return invokeImpl<T>(cmd, args);
}

export const api = {
  currentRepo: () => call<RepoInfo | null>("current_repo"),
  openRepo: (path: string) => call<RepoInfo>("open_repo", { path }),
  repoState: () => call<RepoState>("repo_state"),
  /** Pops the shared undo stack: any section's last change. */
  undo: () => call<string>("undo"),
  openUrl: (url: string) => call<void>("open_url", { url }),

  // History
  graph: (limit: number) => call<Graph>("graph", { limit }),
  searchCommits: (query: string) => call<string[]>("search_commits", { query }),
  commitDetail: (hash: string) => call<CommitDetail>("commit_detail", { hash }),
  branches: () => call<BranchInfo[]>("branches"),
  /** `stash`: stash uncommitted changes first (after a BLOCKED error). */
  checkout: (branch: string, stash = false) => call<string>("checkout", { branch, stash }),
  /** Create a branch at `at` and switch to it. */
  createBranch: (name: string, at: string, stash = false) => call<string>("create_branch", { name, at, stash }),
  deleteBranch: (name: string) => call<string>("delete_branch", { name }),
  cherryPick: (hash: string) => call<string>("cherry_pick", { hash }),
  revert: (hash: string) => call<string>("revert", { hash }),
  mergeBranch: (branch: string) => call<string>("merge_branch", { branch }),

  // Changes
  status: () => call<FileEntry[]>("status"),
  fileDiff: (path: string, staged: boolean) => call<string>("file_diff", { path, staged }),
  stage: (paths: string[]) => call<void>("stage", { paths }),
  unstage: (paths: string[]) => call<void>("unstage", { paths }),
  stageLines: (path: string, staged: boolean, discard: boolean, from: Place, to: Place) =>
    call<void>("stage_lines", { path, staged, discard, from, to }),
  discard: (paths: string[]) => call<void>("discard", { paths }),
  commit: (message: string, amend: boolean) => call<string>("commit", { message, amend }),
  lastMessage: () => call<string>("last_message"),
  sync: (action: "fetch" | "pull" | "push") => call<string>("sync", { action }),
  stashes: () => call<StashInfo[]>("stashes"),
  stash: (message: string | null) => call<string>("stash", { message }),
  stashPop: (index: number) => call<string>("stash_pop", { index }),
  resolve: (path: string, how: "ours" | "theirs" | "resolved") => call<void>("resolve", { path, how }),
  continueOperation: () => call<string>("continue_operation"),
  abortOperation: () => call<string>("abort_operation"),

  // GitHub
  pulls: (filter: "open" | "yours" | "review" | "closed") => call<Pull[]>("pulls", { filter }),
  pullDetail: (number: number) => call<PullDetail>("pull_detail", { number }),
  pullDiff: (number: number) => call<string>("pull_diff", { number }),
  pullAction: (number: number, action: string, body?: string) =>
    call<string>("pull_action", { number, action, body: body ?? null }),
  runs: (branch: string | null, workflow: string | null) => call<Run[]>("runs", { branch, workflow }),
  workflows: () => call<string[]>("workflows"),
  runDetail: (id: number) => call<RunDetail>("run_detail", { id }),
  runAction: (id: number, action: "rerun" | "rerun_failed" | "cancel") => call<string>("run_action", { id, action }),
  jobLog: (id: number) => call<LogLine[]>("job_log", { id }),

  // Agents
  listTasks: () => call<TaskSummary[]>("list_tasks"),
  taskDetail: (id: string) => call<TaskDetail>("task_detail", { id }),
  taskPatch: (id: string) => call<string>("task_patch", { id }),
  createTask: (prompt: string, base: string, agent: AgentChoice, mode: Mode) =>
    call<TaskSummary>("create_task", { prompt, base, agent, mode }),
  /** `message` null starts the task's own prompt; otherwise it's a follow-up. */
  sendToAgent: (id: string, message: string | null, agent: AgentChoice, mode: Mode) =>
    call<void>("send_to_agent", { id, message, agent, mode }),
  /** Answer a question the agent asked; `message` says what to do instead. */
  answerAsk: (id: string, requestId: string, allow: boolean, message = "") =>
    call<void>("answer_ask", { id, requestId, allow, message }),
  /** A message into the turn the agent is on (#22). */
  steer: (id: string, text: string) => call<void>("steer", { id, text }),
  /** Change the running agent's permission mode. */
  setMode: (id: string, mode: Mode) => call<void>("set_mode", { id, mode }),
  stopAgent: (id: string) => call<void>("stop_agent", { id }),
  landTask: (id: string, merge: boolean, commitUncommitted: boolean) =>
    call<string>("land_task", { id, merge, commitUncommitted }),
  discardTask: (id: string) => call<string>("discard_task", { id }),
  openFolder: (id: string) => call<void>("open_folder", { id }),
  /** One run's changes, between its checkpoints. */
  runPatch: (id: string, index: number) => call<RunPatch>("run_patch", { id, index }),
  /** Take back the task's last run (⌘Z brings it back). */
  undoRun: (id: string) => call<string>("undo_run", { id }),
};

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
