import type {
  Debrief,
  AgentChoice,
  BlameRow,
  BranchInfo,
  CommitDetail,
  ConflictPart,
  Credential,
  FileEntry,
  Graph,
  InboxGithub,
  InboxTask,
  Issue,
  IssueDetail,
  LogLine,
  Mode,
  PlanAction,
  PlanCommit,
  PrBadge,
  Pull,
  PullDetail,
  RepoInfo,
  RepoHead,
  RepoState,
  Run,
  RunDetail,
  StashInfo,
  RunPatch,
  TagInfo,
  TaskDetail,
  TaskSummary,
  TaskPull,
  WorktreeInfo,
  WorkBranch,
  WorkItemInfo,
  RuleSuggestion,
} from "./types";
import type { Place } from "./shared/DiffView";

type Invoke = <T>(cmd: string, args?: Record<string, unknown> | Uint8Array) => Promise<T>;

/** True inside the Tauri window; false in a plain browser (`pnpm dev`). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let invokeImpl: Invoke | null = null;

async function call<T>(cmd: string, args?: Record<string, unknown> | Uint8Array): Promise<T> {
  if (!invokeImpl) {
    invokeImpl = inTauri
      ? (await import("@tauri-apps/api/core")).invoke
      : (await import("./mock")).invoke;
  }
  return invokeImpl<T>(cmd, args);
}

/** What the backend's watcher says changed (`src-tauri/src/watch.rs`):
 * refs (HEAD, branches, tags), the index, the working tree, or agent tasks. */
/** `work`: a branch moved or a task did something, so a work item may have
 * changed (sent after `refs` and `tasks`). */
export type RepoChange = "refs" | "index" | "worktree" | "tasks" | "work";

type Listen = (event: string, fn: () => void) => Promise<() => void>;
let listenImpl: Listen | null = null;

/** Call `fn` whenever the repository changes in one of the `changes` ways.
 * Returns the function that stops listening. */
export function onRepoChange(changes: RepoChange[], fn: () => void): () => void {
  let stopped = false;
  const stops: (() => void)[] = [];
  (async () => {
    if (!listenImpl) {
      listenImpl = inTauri
        ? (await import("@tauri-apps/api/event")).listen
        : (await import("./mock")).listen;
    }
    for (const change of changes) {
      const stop = await listenImpl(`${change}-changed`, fn);
      if (stopped) stop();
      else stops.push(stop);
    }
  })();
  return () => {
    stopped = true;
    stops.forEach((stop) => stop());
  };
}

export const api = {
  currentRepo: () => call<RepoInfo | null>("current_repo"),
  openRepo: (path: string) => call<RepoInfo>("open_repo", { path }),
  repoState: () => call<RepoState>("repo_state"),
  /** Pops the shared undo stack: any section's last change. */
  undo: () => call<string>("undo"),
  openUrl: (url: string) => call<void>("open_url", { url }),
  /** The oldest passphrase/password prompt from git or ssh, if one waits. */
  askpassPending: () => call<Credential | null>("askpass_pending"),
  /** `null` cancels. */
  askpassAnswer: (id: number, answer: string | null) => call<void>("askpass_answer", { id, answer }),
  /** The old native (GPUI) app's workspaces, to carry over; null when it had none. */
  legacyWorkspaces: () =>
    call<{ list: { name: string; color: number; repos: string[]; last: string | null }[]; active: number } | null>("legacy_workspaces"),
  /** Branch, ahead/behind and changed files of each repository, for the workspace's list. */
  repoHeads: (paths: string[]) => call<RepoHead[]>("repo_heads", { paths }),
  /** The open tasks of every repository in `roots` (no network). */
  inboxTasks: (roots: string[]) => call<InboxTask[]>("inbox_tasks", { roots }),
  /** Review requests and failing runs on your branches, across `roots`. */
  inboxGithub: (roots: string[]) => call<InboxGithub>("inbox_github", { roots }),

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
  renameBranch: (old: string, name: string) => call<string>("rename_branch", { old, new: name }),
  /** `origin/feat/x`: delete `feat/x` on origin. */
  deleteRemoteBranch: (name: string) => call<string>("delete_remote_branch", { name }),
  rebaseOnto: (onto: string) => call<string>("rebase_onto", { onto }),
  resetTo: (hash: string, mode: "soft" | "mixed" | "hard") => call<string>("reset_to", { hash, mode }),
  checkoutCommit: (hash: string, stash = false) => call<string>("checkout_commit", { hash, stash }),
  tags: () => call<TagInfo[]>("tags"),
  createTag: (name: string, hash: string, message: string | null) => call<string>("create_tag", { name, hash, message }),
  deleteTag: (name: string) => call<string>("delete_tag", { name }),
  pushTag: (name: string, del = false) => call<string>("push_tag", { name, delete: del }),
  worktrees: () => call<WorktreeInfo[]>("worktrees"),
  removeWorktree: (path: string) => call<string>("remove_worktree", { path }),
  /** The commits after `base` an interactive rebase would rewrite, oldest first. */
  rewritePlan: (base: string) => call<PlanCommit[]>("rewrite_plan", { base }),
  rewrite: (base: string, plan: { hash: string; action: PlanAction; message: string | null }[]) =>
    call<string>("rewrite", { base, plan }),
  prBadges: () => call<PrBadge[]>("pr_badges"),
  /** The current branch's PR page, or the page to open one. */
  pullRequestUrl: () => call<string>("pull_request_url"),
  /** Merge `source` into `target`, checking `target` out first if it isn't. */
  mergeInto: (source: string, target: string) => call<string>("merge_into", { source, target }),
  /** The host's page for a pull request from `source` into `target`. */
  pullRequestBetween: (source: string, target: string) => call<string>("pull_request_between", { source, target }),
  /** Who last changed each line, as of `rev` (the working tree when null). */
  blame: (path: string, rev: string | null) => call<BlameRow[]>("blame", { path, rev }),

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
  stashApply: (index: number) => call<string>("stash_apply", { index }),
  stashDrop: (index: number) => call<string>("stash_drop", { index }),
  skipOperation: () => call<string>("skip_operation"),
  conflictRegions: (path: string) => call<ConflictPart[]>("conflict_regions", { path }),
  /** A side per region, in order; writes the file and marks it resolved. */
  resolveRegions: (path: string, choices: string[]) => call<string>("resolve_regions", { path, choices }),
  /** Quiet background fetch: never prompts, never complains. */
  autoFetch: () => call<void>("auto_fetch"),
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
  /** A failed job (its failing step, the end of its log) as a follow-up for an agent. */
  jobFeedback: (run: number, job: number) => call<string>("job_feedback", { run, job }),
  issues: (filter: "assigned" | "yours" | "open" | "closed") => call<Issue[]>("issues", { filter }),
  issueDetail: (number: number) => call<IssueDetail>("issue_detail", { number }),
  issueAction: (number: number, action: "comment" | "close" | "reopen", body?: string) =>
    call<string>("issue_action", { number, action, body: body ?? null }),

  // Agents
  listTasks: () => call<TaskSummary[]>("list_tasks"),
  taskDetail: (id: string) => call<TaskDetail>("task_detail", { id }),
  /** Write the latest run's debrief if it wants one (#104); null when the repository opted out. */
  debriefTask: (id: string) => call<Debrief | null>("debrief_task", { id }),
  taskPatch: (id: string) => call<string>("task_patch", { id }),
  createTask: (prompt: string, base: string, agent: AgentChoice, mode: Mode) =>
    call<TaskSummary>("create_task", { prompt, base, agent, mode }),
  /** A task on a GitHub issue; `prompt` is the issue's (editable) prompt. */
  /** Hand one of your branches to an agent (#80, #82); ⌘Z takes it back. */
  handToAgent: (branch: string, prompt: string, base: string, agent: AgentChoice, mode: Mode) =>
    call<TaskSummary>("hand_to_agent", { branch, prompt, base, agent, mode }),
  createIssueTask: (number: number, prompt: string, base: string, agent: AgentChoice, mode: Mode) =>
    call<TaskSummary>("create_issue_task", { number, prompt, base, agent, mode }),
  /** Push the task's branch and open a pull request for it. */
  publishTask: (id: string, draft: boolean, commitUncommitted: boolean) =>
    call<string>("publish_task", { id, draft, commitUncommitted }),
  /** Push the agent's new commits to the task's pull request; with
   * `commitUncommitted`, what it left uncommitted is committed and goes too. */
  pushTask: (id: string, commitUncommitted = false) => call<string>("push_task", { id, commitUncommitted }),
  /** Every open task's pull request (one `gh` call). */
  taskPulls: () => call<TaskPull[]>("task_pulls"),
  /** New review on the task's PR, written as a follow-up for its agent. */
  reviewFeedback: (id: string) => call<string>("review_feedback", { id }),
  /** The failed Actions jobs on the task's PR, written as a follow-up for its agent. */
  checkFeedback: (id: string) => call<string>("check_feedback", { id }),
  /** Its PR was closed unmerged: mark it closed and remove its folder and branch (⌘Z undoes). */
  closeTask: (id: string) => call<string>("close_task", { id }),
  /** Its PR merged: mark it landed and remove its folder and branch (⌘Z undoes). */
  settleTask: (id: string, commit: string) => call<string>("settle_task", { id, commit }),
  /** `message` null starts the task's own prompt; otherwise it's a follow-up. */
  /** `fresh`: start a new session from the task's prompt and the last debrief, not the full context (#98). */
  sendToAgent: (id: string, message: string | null, agent: AgentChoice, mode: Mode, fresh?: boolean) =>
    call<void>("send_to_agent", { id, message, agent, mode, fresh: fresh ?? null }),
  /** Answer a question the agent asked; `message` says what to do instead,
   * `always` allows it for the rest of the session (#40). */
  answerAsk: (id: string, requestId: string, allow: boolean, message = "", always = false) =>
    call<void>("answer_ask", { id, requestId, allow, message, always }),
  /** Answer `AskUserQuestion` (#102): the labels picked, or words of your
   * own, for each question in order. */
  answerQuestions: (id: string, requestId: string, answers: string[][]) =>
    call<void>("answer_questions", { id, requestId, answers }),
  /** Approve the agent's plan (#101): it starts, in `mode` from here on. */
  approvePlan: (id: string, requestId: string, mode: Mode) => call<void>("approve_plan", { id, requestId, mode }),
  /** Start the next agent without the folder's setup script (#23). */
  skipSetup: (id: string) => call<void>("skip_setup", { id }),
  /** Run the setup script again, before the next agent. */
  rerunSetup: (id: string) => call<void>("rerun_setup", { id }),
  /** Follow-ups for when the agent finishes, kept with the task (#22). */
  queueFollowUp: (id: string, text: string) => call<void>("queue_follow_up", { id, text }),
  unqueue: (id: string, index: number) => call<void>("unqueue", { id, index }),
  /** Send the oldest queued follow-up if the agent is idle; true if one went. */
  sendQueued: (id: string, agent: AgentChoice, mode: Mode) => call<boolean>("send_queued", { id, agent, mode }),
  /** Delete the run checkpoints of tasks that are done, and discarded tasks' transcripts. */
  cleanUpTasks: () => call<string>("clean_up_tasks"),
  /** Repeated corrections across tasks, as candidate lines for CLAUDE.md (#107). Slow: it asks Claude. */
  suggestRules: () => call<RuleSuggestion[]>("suggest_rules"),
  addRule: (rule: string) => call<string>("add_rule", { rule }),
  declineRule: (rule: string) => call<void>("decline_rule", { rule }),
  /** A notification from the operating system, for when the window is in the background. */
  notify: (title: string, body: string) => call<void>("notify_os", { title, body }),
  /** Keep an image for a prompt; returns the path the prompt names it by.
   * The bytes go as the request's raw body. */
  attachImage: (data: Uint8Array) => call<string>("attach_image", data),
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
  undoRun: (id: string, from: number | null = null) => call<string>("undo_run", { id, from }),

  // Work
  /** Your own branches with work on them (not agents'). */
  workBranches: () => call<WorkBranch[]>("work_branches"),
  /** A branch's changes since it left the base. */
  branchPatch: (branch: string) => call<string>("branch_patch", { branch }),
  /** Every work item in the repository, most recently changed first (#78).
   * GitHub's part is cached in the backend, so call it on every `work`
   * change. `seen`: when you last looked at each task, by task id. */
  workItems: (seen: Record<string, number> = {}) => call<WorkItemInfo[]>("work_items", { seen }),
};

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
