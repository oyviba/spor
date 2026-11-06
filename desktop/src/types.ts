// Mirrors the serde types in src-tauri/src/lib.rs.

export interface RepoInfo {
  root: string;
  name: string;
  branch: string | null;
  branches: string[];
  default_base: string | null;
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
  resumable: boolean;
  /** Taken back: its changes are gone, follow-ups resume from before it. */
  undone: boolean;
}

export interface TaskSummary {
  id: string;
  title: string;
  prompt: string;
  base: string;
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
  mode: Mode | null;
  /** The GitHub issue it was started from. */
  issue: number | null;
  /** The pull request it was published as. */
  pull: number | null;
}

/** How much an agent may do without asking (#21). */
export type Mode = "supervised" | "acceptEdits" | "auto" | "full";

/** Claude Code's `--effort`. */
export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export interface Ask {
  id: string;
  tool: string;
  detail: string;
  /** What "Allow for this session" covers ("`npm test` commands"); null: only once. */
  session: string | null;
}

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
  | { kind: "ask"; id: string; tool: string; detail: string; session: string | null }
  | { kind: "answer"; id: string; allow: boolean; message: string; always: boolean }
  | { kind: "steer"; text: string }
  | { kind: "setup"; ok: boolean | null; code: number | null };

export interface TaskDetail {
  task: TaskSummary;
  transcript: Item[];
  runs: RunInfo[];
  conflicts: string[];
  /** The run "Undo this run" would take back. */
  undoable: number | null;
  /** Questions the agent is waiting on. */
  asks: Ask[];
  /** The running agent can be answered, steered and interrupted. */
  listening: boolean;
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

/** One line of `git blame`. `hash` is empty for an uncommitted line. */
export interface BlameRow {
  hash: string;
  author: string;
  time: number;
  summary: string;
  line: number;
  text: string;
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
