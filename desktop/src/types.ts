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
}

export type RunStateName = "running" | "finished" | "failed" | "stopped";

export interface RunInfo {
  index: number;
  agent: "claude" | "command";
  label: string;
  state: RunStateName;
  exit_code: number | null;
  started: number;
  ended: number | null;
  cost_usd: number | null;
  resumable: boolean;
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
  stats: Stats;
  run: RunInfo | null;
  runs: number;
  command: string;
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
  | { kind: "output"; text: string };

export interface TaskDetail {
  task: TaskSummary;
  transcript: Item[];
  runs: RunInfo[];
  conflicts: string[];
}

export type AgentChoice =
  | { kind: "claude" }
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
