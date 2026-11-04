import type { AgentChoice, RepoInfo, TaskDetail, TaskSummary } from "./types";

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
  listTasks: () => call<TaskSummary[]>("list_tasks"),
  taskDetail: (id: string) => call<TaskDetail>("task_detail", { id }),
  taskPatch: (id: string) => call<string>("task_patch", { id }),
  createTask: (prompt: string, base: string, agent: AgentChoice, allowCommands: boolean) =>
    call<TaskSummary>("create_task", { prompt, base, agent, allowCommands }),
  /** `message` null starts the task's own prompt; otherwise it's a follow-up. */
  sendToAgent: (id: string, message: string | null, agent: AgentChoice, allowCommands: boolean) =>
    call<void>("send_to_agent", { id, message, agent, allowCommands }),
  stopAgent: (id: string) => call<void>("stop_agent", { id }),
  landTask: (id: string, merge: boolean, commitUncommitted: boolean) =>
    call<string>("land_task", { id, merge, commitUncommitted }),
  discardTask: (id: string) => call<string>("discard_task", { id }),
  undo: () => call<string>("undo"),
  openFolder: (id: string) => call<void>("open_folder", { id }),
};

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
