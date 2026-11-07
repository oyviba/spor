// Mirrors the serde types in src-tauri/src/lib.rs.

export interface RepoInfo {
  root: string;
  name: string;
  branch: string | null;
  branches: string[];
  default_base: string | null;
  /** Where new tasks start unless another branch is picked: `origin/main`
   * when origin has the default branch, else the local one. */
  default_start: string | null;
  claude: boolean;
}

export interface Stats {
  commits: number;
  uncommitted: number;
  files: number;
  additions: number;
  deletions: number;
  base_moved: boolean;
  last_commit: number;
  missing: boolean;
  /** Commits its published branch doesn't have yet. */
  unpushed: number;
}

export type RunStateName = "running" | "finished" | "failed" | "stopped";

export interface RunInfo {
  index: number;
  agent: "claude" | "command";
  label: string;
  prompt: string;
  state: RunStateName;
  exit_code: number | null;
  started: number;
  ended: number | null;
  cost_usd: number | null;
  /** Tokens sent to the model (input and cached) and received. */
  tokens_in: number;
  tokens_out: number;
  /** How full the context was at the last turn, in percent (null: no turn yet). */
  context: number | null;
  /** The model that actually ran, from `init`; it can differ from the one asked for. */
  model: string | null;
  /** MCP servers that didn't connect. */
  mcp_failed: string[];
  resumable: boolean;
  /** Taken back: its changes are gone, follow-ups resume from before it. */
  undone: boolean;
}

export interface TaskSummary {
  id: string;
  title: string;
  prompt: string;
  /** The branch it lands on and its pull request goes into. */
  base: string;
  /** Where it started when that isn't `base`: `origin/main`. */
  from: string | null;
  branch: string;
  worktree: string;
  created: number;
  landed: string | null;
  /** Its pull request was closed without merging, and it was cleaned up. */
  closed: boolean;
  stats: Stats;
  run: RunInfo | null;
  runs: number;
  command: string;
  /** Questions the agent is waiting on. */
  asks: number;
  /** The first question in a few words ("asks: Keep the old endpoint?"); null when none waits. */
  ask: string | null;
  /** While it runs: how far through its plan, and what it's doing (#96). */
  progress: Progress | null;
  mode: Mode | null;
  /** The GitHub issue it was started from. */
  issue: number | null;
  /** The pull request it was published as. */
  pull: number | null;
  /** Follow-ups waiting for the agent to finish, kept with the task. */
  queued: string[];
  /** What all its runs cost, in dollars, and the tokens they used. */
  spent: number;
  tokens: number;
}

/** How much an agent may do without asking (#21). */
export type Mode = "supervised" | "acceptEdits" | "auto" | "full" | "plan";

/** Claude Code's `--effort`. */
export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

/** One of `AskUserQuestion`'s questions (#102). */
export interface Question {
  question: string;
  /** A short label for it ("Approach"). */
  header: string;
  options: { label: string; description: string }[];
  /** More than one option can be picked. */
  multi: boolean;
}

/** What a question asks for: permission to use a tool, answers to
 * questions with options (#102), or approval of a plan (#101). */
export type AskKind =
  | { ask: "permission" }
  | { ask: "questions"; questions: Question[] }
  | { ask: "plan"; plan: string };

export type Ask = {
  id: string;
  tool: string;
  /** The gist: the command or file, the first question, the plan's title. */
  detail: string;
  /** What "Allow for this session" covers ("`npm test` commands"); null: only once. */
  session: string | null;
} & AskKind;

export type Item =
  | { kind: "prompt"; text: string }
  | { kind: "text"; text: string }
  | { kind: "tool"; name: string; detail: string }
  | { kind: "tool_error"; text: string }
  | {
      kind: "done";
      ok: boolean;
      summary: string;
      cost_usd: number | null;
      turns: number | null;
      seconds: number | null;
    }
  | { kind: "output"; text: string }
  | ({ kind: "ask"; id: string; tool: string; detail: string; session: string | null } & AskKind)
  | { kind: "answer"; id: string; allow: boolean; message: string; always: boolean }
  | { kind: "steer"; text: string }
  | { kind: "setup"; ok: boolean | null; code: number | null }
  /** Claude Code started: the model that ran, and MCP servers that failed to connect. */
  | { kind: "init"; model: string; mcp_failed: string[] }
  /** The conversation was compacted here. */
  | { kind: "compacted" }
  /** The agent's plan as it now stands (TodoWrite, #96). */
  | { kind: "todos"; todos: Todo[] }
  /** What a command printed (#97): the end of its output, and whether it's a test run or a build. */
  | {
      kind: "result";
      name: string;
      detail: string;
      ok: boolean;
      output: string;
      truncated: boolean;
      check: "tests" | "build" | null;
    }
  /** The agent thinking (#99). */
  | { kind: "thinking"; text: string }
  /** A subagent started by the Task/Agent call `id`. */
  | { kind: "subagent"; id: string; agent: string; description: string; prompt: string }
  /** Something a subagent did, under the call that started it. */
  | { kind: "nested"; parent: string; item: Item };

/** One step of the agent's plan. */
export interface Todo {
  /** "Migrate the tests". */
  content: string;
  /** "Migrating the tests": the step while it's being done. */
  active: string;
  status: "pending" | "in_progress" | "completed";
}

/** What a running agent is doing (#96). */
export interface Progress {
  /** Steps done, of how many; total 0 without a plan. */
  done: number;
  total: number;
  /** The step in progress ("Migrating the tests"). */
  step: string | null;
  /** Its latest tool call. */
  tool: string | null;
  detail: string | null;
}

export interface TaskDetail {
  task: TaskSummary;
  transcript: Item[];
  runs: RunInfo[];
  conflicts: string[];
  /** Work that reached the base after the task started and landing it
   * would take back out: lines per file, most first. */
  undoes: { path: string; lines: number }[];
  /** The run "Undo this run" would take back. */
  undoable: number | null;
  /** The runs "Undo from here" can start at. */
  undoable_from: number[];
  /** The folder's setup script and how it went (#23). */
  setup: SetupInfo | null;
  /** Questions the agent is waiting on. */
  asks: Ask[];
  /** The running agent can be answered, steered and interrupted. */
  listening: boolean;
  /** The latest run's debrief (#104), once written. */
  debrief: Debrief | null;
  /** The latest run ended and should get one (`debriefTask`). */
  wants_debrief: boolean;
}

/** What a run says about itself, in a fixed shape (#104). */
export interface Debrief {
  run: number;
  changed: string;
  checked: string;
  risks: { path: string; note: string }[];
  open_questions: string[];
  follow_ups: string[];
}

export interface SetupInfo {
  script: string;
  /** null: it hasn't run. "due": it worked, and runs again before the next agent. */
  state: "running" | "worked" | "failed" | "skipped" | "due" | null;
  code: number | null;
}

export interface RunPatch {
  patch: string;
  /** Left out: they came with the base (a rebase or merge), not the agent. */
  from_base: string[];
}

export type AgentChoice =
  | { kind: "claude"; model?: string; effort?: Effort | "" }
  | { kind: "command"; template: string }
  | { kind: "none" };

export interface LineComment {
  id: string;
  file: string;
  line: number;
  /** "old" for a removed line (numbered in the base), "new" otherwise. */
  side: "old" | "new";
  text: string;
}

// ---- Repository ----

export interface RepoState {
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  operation: "merge" | "rebase" | "cherrypick" | "revert" | null;
  conflicts: string[];
  /** Changes when any ref or HEAD moves. */
  refs: string;
  /** Changes when the index (or any ref) does. */
  index: string;
  /** What ⌘Z would undo. */
  undo: string | null;
}

// ---- History ----

export type Shape = "through" | "top" | "bottom" | "in" | "out" | "join" | "fork";

export interface Segment {
  lane: number;
  shape: Shape;
  color: number;
  bold: boolean;
}

export interface GraphRow {
  hash: string;
  short: string;
  subject: string;
  author: string;
  time: number;
  refs: string[];
  head: boolean;
  merge: boolean;
  lane: number;
  color: number;
  segments: Segment[];
}

export interface Graph {
  rows: GraphRow[];
  palette: string[];
  lanes: number;
  more: boolean;
  main: string | null;
}

export interface CommitDetail {
  hash: string;
  parents: string[];
  author: string;
  email: string;
  date: string;
  time: number;
  message: string;
  patch: string;
  /** The agent task that made it, from the `Spor-Task` trailer (#108). */
  task: TaskRef | null;
}

/** An agent task behind a commit. */
export interface TaskRef {
  id: string;
  title: string;
  prompt: string;
  state: "open" | "landed" | "closed";
}

/** Somewhere a search found words in a task's conversation (#108). */
export interface TranscriptHit {
  task: string;
  title: string;
  /** The run it was said in. */
  run: number;
  kind: "prompt" | "agent" | "steer" | "debrief";
  snippet: string;
}

export interface BranchInfo {
  name: string;
  current: boolean;
  remote: boolean;
  agent: boolean;
}

// ---- Changes ----

export interface FileEntry {
  path: string;
  orig_path: string | null;
  status: "staged" | "stageddeleted" | "modified" | "deleted" | "untracked" | "conflicted";
  staged: boolean;
}

export interface StashInfo {
  index: number;
  message: string;
}

export interface TagInfo {
  name: string;
  /** The commit it's on. */
  hash: string;
}

export interface WorktreeInfo {
  path: string;
  branch: string | null;
  head: string;
  main: boolean;
  current: boolean;
  prunable: boolean;
  agent: boolean;
}

/** A commit an interactive rebase would rewrite, oldest first. */
export interface PlanCommit {
  hash: string;
  short: string;
  subject: string;
  message: string;
  author: string;
  time: number;
}

export type PlanAction = "pick" | "reword" | "squash" | "fixup" | "drop";

export interface PrBadge {
  branch: string;
  number: number;
  draft: boolean;
  review: "none" | "approved" | "changesrequested";
  checks: "none" | "pending" | "passing" | "failing";
}

/** A passphrase, password or yes/no question from git or ssh. */
export interface Credential {
  id: number;
  prompt: string;
  secret: boolean;
  confirm: boolean;
}

// ---- GitHub ----

export interface Pull {
  number: number;
  title: string;
  author: string;
  head: string;
  base: string;
  state: "open" | "draft" | "merged" | "closed";
  review: "none" | "approved" | "changesrequested";
  checks: "none" | "pending" | "passing" | "failing";
  updated: number;
  additions: number;
  deletions: number;
  url: string;
}

export type CiStatus = "queued" | "running" | "success" | "failure" | "cancelled" | "skipped" | "neutral";

export interface Check {
  name: string;
  workflow: string;
  status: CiStatus;
  url: string;
  started: number;
  completed: number;
  run_id: number | null;
  job_id: number | null;
}

export interface PullComment {
  author: string;
  body: string;
  at: number;
  verdict: "approved" | "changesrequested" | "commented" | "dismissed" | null;
}

export interface PullDetail {
  pull: Pull;
  body: string;
  mergeable: "yes" | "conflicting" | "unknown";
  changed_files: number;
  commits: number;
  checks: Check[];
  timeline: PullComment[];
}

export interface Run {
  id: number;
  number: number;
  workflow: string;
  title: string;
  branch: string;
  sha: string;
  event: string;
  status: CiStatus;
  attempt: number;
  created: number;
  updated: number;
  url: string;
}

export interface Job {
  id: number;
  name: string;
  status: CiStatus;
  started: number;
  completed: number;
  url: string;
  steps: { number: number; name: string; status: CiStatus }[];
}

export interface RunDetail {
  run: Run;
  jobs: Job[];
}

export interface LogLine {
  kind: "plain" | "group" | "command" | "error" | "warning" | "notice";
  text: string;
}

/** An open task's pull request (#24, #47). */
export interface TaskPull {
  task: string;
  pull: Pull;
  /** The commit it was merged as, once merged. */
  merge_commit: string | null;
}

export interface Issue {
  number: number;
  title: string;
  author: string;
  open: boolean;
  labels: string[];
  assignees: string[];
  comments: number;
  updated: number;
  url: string;
}

export interface IssueDetail {
  issue: Issue;
  body: string;
  comments: PullComment[];
  /** The prompt an agent task on it starts from. */
  prompt: string;
}

/** An open task in any known repository, for the Inbox (#42). */
export interface InboxTask {
  /** The repository's root. */
  repo: string;
  id: string;
  title: string;
  asks: number;
  /** The first question in a few words ("asks: Keep the old endpoint?"); null when none waits. */
  ask: string | null;
  /** Why a finished run is worth a look, from its debrief ("2 open questions"). */
  reason?: string | null;
  run: "running" | "finished" | "failed" | "stopped" | null;
  started: number | null;
  ended: number | null;
  created: number;
}

/** What GitHub has waiting for you, across the known repositories. */
export interface InboxGithub {
  reviews: { repo: string; number: number; title: string; author: string; updated: number; url: string }[];
  runs: (Run & { repo: string })[];
  /** What couldn't be asked, one line each. */
  errors: string[];
}

/** One line of `git blame`. `hash` is empty for an uncommitted line. */
export interface BlameRow {
  hash: string;
  author: string;
  time: number;
  summary: string;
  line: number;
  text: string;
  /** The task whose commit this line is from, if spor has it (#108). */
  task: string | null;
}

/** A conflicted file, split into text and conflict regions. */
export type ConflictPart =
  | { kind: "text"; text: string }
  | { kind: "conflict"; ours: string; ours_label: string; theirs: string; theirs_label: string; base: string | null };

/** A repository in the workspace's list. */
export interface RepoHead {
  path: string;
  exists: boolean;
  branch: string | null;
  ahead: number;
  behind: number;
  changes: number;
}

// ---- Work ----

export interface BranchCommit {
  hash: string;
  short: string;
  subject: string;
  author: string;
  time: number;
}

/** A work item (#74, #78): a branch with whatever goes with it (its issue,
 * task folder and agent, pull request, checks), or an issue assigned to you
 * with no branch yet. The words are `docs/concepts.md`'s; the join is
 * `spor::work::items`, served by `work_items`. */
export interface WorkItemInfo {
  /** `branch:<name>` or `issue:<number>`. */
  key: string;
  title: string;
  branch: string | null;
  /** What it lands on: the trunk, or another item's branch when stacked. */
  base: string | null;
  /** The task's folder, or the checkout the branch is in. */
  worktree: string | null;
  /** The task's id. */
  task: string | null;
  issue: number | null;
  pull: {
    number: number;
    title: string;
    state: "open" | "draft" | "merged" | "closed";
    review: "none" | "approved" | "changesrequested";
    author: string;
    url: string;
  } | null;
  checks: "passed" | "failed" | "running" | null;
  owner: { kind: "you" } | { kind: "agent"; name: string } | { kind: "teammate"; login: string };
  stage: "planned" | "inprogress" | "inreview" | "landed" | "closed";
  waiting_on: {
    who: "you" | "agent" | "teammate" | "ci";
    why: "question" | "unseen" | "review" | "checksfailed" | "changesrequested" | "ready" | null;
  } | null;
  updated: number;
}

/** One of your own branches with work on it (#81): commits its base
 * doesn't have, or checked out. */
export interface WorkBranch {
  name: string;
  base: string;
  current: boolean;
  /** Newest first. */
  commits: BranchCommit[];
  files: number;
  additions: number;
  deletions: number;
  updated: number;
}

/** A line worth adding to CLAUDE.md, from corrections given in several tasks (#107). */
export interface RuleSuggestion {
  rule: string;
  tasks: string[];
}
