// Turning a flat transcript into what the conversation shows: one block per
// run, finished runs folded behind "Worked for …", and stretches of tool
// calls summarized as one sentence (see issue #27).

import type { AskKind, Item, Todo } from "../../types";

type Done = Extract<Item, { kind: "done" }>;
export type Tool = Extract<Item, { kind: "tool" | "tool_error" }>;

/** What a run shows between its prompt and its end. */
export type Block =
  | { kind: "text"; text: string }
  | { kind: "tools"; tools: Tool[] }
  | { kind: "output"; lines: string[] }
  /** A question the agent asked, and how it was answered (null: waiting). */
  | ({
      kind: "ask";
      id: string;
      tool: string;
      detail: string;
      answer: { allow: boolean; message: string; always: boolean } | null;
    } & AskKind)
  /** A message sent while it worked. */
  | { kind: "steer"; text: string }
  /** The folder's setup script and its output (ok null: still going). */
  | { kind: "setup"; ok: boolean | null; code: number | null; lines: string[] }
  /** The conversation was compacted here; the agent may have lost detail. */
  | { kind: "compacted" }
  /** MCP servers that failed to connect when the agent started. */
  | { kind: "mcp"; failed: string[] };

export interface RunView {
  index: number;
  prompt: string;
  blocks: Block[];
  done: Done | null;
  /** The run still in progress (the last one, while the agent works). */
  live: boolean;
  /** Its plan as it last stood (TodoWrite, #96), if it kept one. */
  plan: Todo[] | null;
  /**
   * For a run that finished well: everything but its final message folds
   * away, and that message stays in view. Null when the run doesn't fold.
   */
  fold: { label: string; hidden: Block[]; final: Block | null } | null;
}

export function runs(items: Item[], running: boolean): RunView[] {
  const out: RunView[] = [];
  let current: RunView | null = null;
  // Output between the setup markers is the setup script's.
  let setup: Extract<Block, { kind: "setup" }> | null = null;
  const push = (block: Block) => {
    if (!current) {
      current = { index: out.length, prompt: "", blocks: [], done: null, live: false, plan: null, fold: null };
      out.push(current);
    }
    const last = current.blocks.at(-1);
    if (block.kind === "tools" && last?.kind === "tools") last.tools.push(...block.tools);
    else if (block.kind === "output" && last?.kind === "output") last.lines.push(...block.lines);
    else current.blocks.push(block);
  };
  for (const item of items) {
    switch (item.kind) {
      case "prompt":
        current = { index: out.length, prompt: item.text, blocks: [], done: null, live: false, plan: null, fold: null };
        out.push(current);
        break;
      case "text":
        push({ kind: "text", text: item.text });
        break;
      case "tool":
      case "tool_error":
        push({ kind: "tools", tools: [item] });
        break;
      case "output":
        if (setup) setup.lines.push(item.text);
        else push({ kind: "output", lines: [item.text] });
        break;
      case "setup":
        if (item.ok == null) {
          setup = { kind: "setup", ok: null, code: null, lines: [] };
          push(setup);
        } else if (setup) {
          setup.ok = item.ok;
          setup.code = item.code;
          setup = null;
        }
        break;
      case "ask":
        push({ ...askKind(item), kind: "ask", id: item.id, tool: item.tool, detail: item.detail, answer: null });
        break;
      case "answer": {
        const ask = (current as RunView | null)?.blocks.find((b) => b.kind === "ask" && b.id === item.id);
        if (ask?.kind === "ask") ask.answer = { allow: item.allow, message: item.message, always: item.always };
        break;
      }
      case "steer":
        push({ kind: "steer", text: item.text });
        break;
      case "compacted":
        push({ kind: "compacted" });
        break;
      case "init":
        if (item.mcp_failed.length > 0) push({ kind: "mcp", failed: item.mcp_failed });
        break;
      case "done":
        if (current) (current as RunView).done = item;
        break;
      case "todos":
        if (current) (current as RunView).plan = item.todos;
        break;
    }
  }
  const last = out.at(-1);
  if (last && running && !last.done) last.live = true;
  for (const run of out) run.fold = foldOf(run);
  return out;
}

/** Just the kind of question an ask item is (older logs have none: a permission). */
function askKind(item: Extract<Item, { kind: "ask" }>): AskKind {
  switch (item.ask) {
    case "questions":
      return { ask: "questions", questions: item.questions };
    case "plan":
      return { ask: "plan", plan: item.plan };
    default:
      return { ask: "permission" };
  }
}

function foldOf(run: RunView): RunView["fold"] {
  if (run.live || !run.done || !run.done.ok) return null;
  // A refused or failed tool call, or a question answered no, stays in
  // view, so the fold never hides something that went wrong.
  if (
    run.blocks.some(
      (b) =>
        (b.kind === "tools" && b.tools.some((t) => t.kind === "tool_error")) ||
        (b.kind === "ask" && b.ask !== "plan" && !b.answer?.allow) ||
        b.kind === "mcp" ||
        (b.kind === "setup" && b.ok === false),
    )
  )
    return null;
  const finalIndex = run.blocks.findLastIndex((b) => b.kind === "text");
  const final = finalIndex >= 0 ? run.blocks[finalIndex] : null;
  const hidden = run.blocks.filter((_, i) => i !== finalIndex);
  if (hidden.length === 0) return null;
  const seconds = run.done.seconds;
  const worked = seconds != null ? `Worked for ${duration(seconds)}` : "Worked";
  return {
    label:
      (run.plan?.length ? `${worked} · ${planEnd(run.plan)}` : worked) +
      (run.blocks.some((b) => b.kind === "compacted") ? ", compacted" : ""),
    hidden,
    final,
  };
}

/** How far a finished run got through its plan: "7 of 7 done", "stopped at 5 of 7". */
export function planEnd(plan: Todo[]): string {
  const done = plan.filter((t) => t.status === "completed").length;
  return done === plan.length ? `${done} of ${plan.length} done` : `stopped at ${done} of ${plan.length}`;
}

/** The label for a run that ended without finishing. */
export function stoppedLabel(run: RunView): string | null {
  if (!run.done || run.done.ok) return null;
  const s = run.done.seconds;
  if (run.done.summary === "Stopped") return s != null ? `You stopped after ${duration(s)}` : "You stopped this run";
  return run.done.summary;
}

// ---- Summaries -------------------------------------------------------------

type Action = "command" | "edit" | "agent" | "web" | "plan" | "read" | "search" | "other";

function actionOf(name: string): Action {
  switch (name) {
    case "Bash":
    case "BashOutput":
    case "KillShell":
      return "command";
    case "Edit":
    case "MultiEdit":
    case "Write":
    case "NotebookEdit":
      return "edit";
    case "Task":
    case "Agent":
      return "agent";
    case "WebFetch":
    case "WebSearch":
      return "web";
    case "TodoWrite":
    case "ExitPlanMode":
      return "plan";
    case "Read":
    case "LS":
      return "read";
    case "Grep":
    case "Glob":
      return "search";
    default:
      return "other";
  }
}

// Lower is more important: what changed the world comes before what looked.
const PRIORITY: Record<Action, number> = {
  command: 0,
  edit: 0,
  agent: 0,
  web: 1,
  plan: 1,
  read: 2,
  search: 2,
  other: 3,
};

function phrase(action: Action, tools: Tool[]): string {
  const n = tools.length;
  const distinct = new Set(tools.map((t) => (t.kind === "tool" ? t.detail : t.text))).size;
  switch (action) {
    case "command":
      return n === 1 ? "Ran a command" : `Ran ${n} commands`;
    case "edit":
      return distinct === 1 ? "Edited a file" : `Edited ${distinct} files`;
    case "agent":
      return n === 1 ? "Started a subagent" : `Started ${n} subagents`;
    case "web":
      return "Searched the web";
    case "plan":
      return "Updated the plan";
    case "read":
      return distinct === 1 ? "Read a file" : `Read ${distinct} files`;
    case "search":
      return n === 1 ? "Searched the code" : `Searched the code ${n} times`;
    case "other":
      return n === 1 ? "Used a tool" : `Used ${n} tools`;
  }
}

/**
 * One sentence for a stretch of tool calls: the two most important kinds of
 * action, in the order they first happened, and a count of the rest.
 * "Ran 3 commands and edited 2 files, and performed 4 other actions".
 */
export function summarize(tools: Tool[]): { text: string; failed: number } {
  const groups = new Map<Action, Tool[]>();
  let failed = 0;
  for (const t of tools) {
    if (t.kind === "tool_error") {
      failed++;
      continue;
    }
    const a = actionOf(t.name);
    groups.set(a, [...(groups.get(a) ?? []), t]);
  }
  const ranked = [...groups.entries()]
    .map(([action, list], order) => ({ action, list, order }))
    .sort((a, b) => PRIORITY[a.action] - PRIORITY[b.action] || a.order - b.order);
  const shown = ranked.slice(0, 2).sort((a, b) => a.order - b.order);
  const rest = ranked.slice(2).reduce((n, g) => n + g.list.length, 0);
  const parts = shown.map((g) => phrase(g.action, g.list));
  if (rest > 0) parts.push(`performed ${rest} other ${rest === 1 ? "action" : "actions"}`);
  if (parts.length === 0) return { text: failed === 1 ? "A tool call failed" : `${failed} tool calls failed`, failed };
  const lowered = parts.map((p, i) => (i === 0 ? p : p.charAt(0).toLowerCase() + p.slice(1)));
  const text =
    lowered.length < 3 ? lowered.join(" and ") : `${lowered.slice(0, -1).join(" and ")}, and ${lowered.at(-1)}`;
  return { text, failed };
}

// ---- Time ------------------------------------------------------------------

/** 42s, 2m 05s, 1h 03m. */
export function duration(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const m = Math.floor(s / 60);
  if (m === 0) return `${s}s`;
  const h = Math.floor(m / 60);
  if (h === 0) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  return `${h}h ${String(m % 60).padStart(2, "0")}m`;
}

/** 7m, 3h, 2d: for list rows. */
export function ago(unixSeconds: number, nowMs: number): string {
  const s = Math.max(0, Math.floor(nowMs / 1000 - unixSeconds));
  if (s < 60) return "now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.floor(h / 24);
  return d < 30 ? `${d}d` : `${Math.floor(d / 30)}mo`;
}

// ---- Tokens and context -------------------------------------------------------

/** 850, 12.3k, 1.2M. */
export function tokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** Past this much of the window, quality starts to slip and compaction is near. */
export const CONTEXT_HIGH = 80;
