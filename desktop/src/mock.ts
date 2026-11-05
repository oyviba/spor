// A stand-in backend for working on the UI in a plain browser (`pnpm dev`).
// It keeps a few tasks in memory, one of which is "working" and grows its
// transcript over time, and answers the same commands as src-tauri.

import type { AgentChoice, Ask, Issue, IssueDetail, Item, Mode, Pull, RepoInfo, RunInfo, TaskDetail, TaskPull, TaskSummary } from "./types";

const now = () => Math.floor(Date.now() / 1000);
const t0 = now();

interface MockTask {
  summary: TaskSummary;
  transcript: Item[];
  runs: RunInfo[];
  patch: string;
  /** Items still to "arrive" while it runs. */
  script: Item[];
  /** Questions it's waiting on: it doesn't go on until they're answered. */
  asks?: Ask[];
}

const repo: RepoInfo = {
  root: "/Users/you/code/postplan",
  name: "postplan",
  branch: "main",
  branches: ["main", "release/1.4", "feat/share-links"],
  default_base: "main",
  claude: true,
};

const stats = (over: Partial<TaskSummary["stats"]> = {}): TaskSummary["stats"] => ({
  commits: 0,
  uncommitted: 0,
  files: 0,
  additions: 0,
  deletions: 0,
  base_moved: false,
  last_commit: t0,
  missing: false,
  unpushed: 0,
  ...over,
});

const run = (over: Partial<RunInfo>): RunInfo => ({
  index: 0,
  agent: "claude",
  label: "Claude Code",
  state: "finished",
  exit_code: 0,
  started: t0 - 600,
  ended: t0 - 300,
  cost_usd: 0.42,
  resumable: true,
  prompt: "",
  undone: false,
  ...over,
});

const summary = (id: string, prompt: string, created: number, over: Partial<TaskSummary> = {}): TaskSummary => ({
  id,
  title: prompt.split("\n")[0],
  prompt,
  base: "main",
  branch: `agent/${id}`,
  worktree: `/Users/you/code/postplan.spor/${id}`,
  created,
  landed: null,
  closed: false,
  stats: stats(),
  run: null,
  runs: 0,
  command: `cd /Users/you/code/postplan.spor/${id} && claude '${prompt}'`,
  asks: 0,
  mode: "acceptEdits",
  issue: null,
  pull: null,
  ...over,
});

const SHARE_PATCH = `diff --git a/src/drafts/share.ts b/src/drafts/share.ts
new file mode 100644
--- /dev/null
+++ b/src/drafts/share.ts
@@ -0,0 +1,18 @@
+import { randomBytes } from "node:crypto";
+import { db } from "../db";
+
+const DAY = 24 * 60 * 60 * 1000;
+
+/** A link anyone can open until it expires. */
+export async function createShareLink(draftId: string, days = 7) {
+  const token = randomBytes(18).toString("base64url");
+  const expiresAt = new Date(Date.now() + days * DAY);
+  await db.shareLinks.insert({ draftId, token, expiresAt });
+  return { url: \`/s/\${token}\`, expiresAt };
+}
+
+export async function resolveShareLink(token: string) {
+  const link = await db.shareLinks.find({ token });
+  if (!link || link.expiresAt < new Date()) return null;
+  return db.drafts.get(link.draftId);
+}
diff --git a/src/routes/drafts.ts b/src/routes/drafts.ts
--- a/src/routes/drafts.ts
+++ b/src/routes/drafts.ts
@@ -1,6 +1,7 @@
 import { Router } from "express";
 import { requireUser } from "../auth";
 import { db } from "../db";
+import { createShareLink, resolveShareLink } from "../drafts/share";

 export const drafts = Router();

@@ -21,4 +22,17 @@ drafts.put("/:id", requireUser, async (req, res) => {
   await db.drafts.update(req.params.id, req.body);
   res.sendStatus(204);
 });
+
+drafts.post("/:id/share", requireUser, async (req, res) => {
+  const days = Number(req.body.days ?? 7);
+  res.json(await createShareLink(req.params.id, days));
+});
+
+drafts.get("/s/:token", async (req, res) => {
+  const draft = await resolveShareLink(req.params.token);
+  if (!draft) return res.sendStatus(404);
+  res.json(draft);
+});
-// TODO: sharing
`;

const LOGIN_PATCH = `diff --git a/src/auth/session.ts b/src/auth/session.ts
--- a/src/auth/session.ts
+++ b/src/auth/session.ts
@@ -12,7 +12,9 @@ export function requireUser(req, res, next) {
   const session = sessions.get(req.cookies.sid);
-  if (!session) return res.redirect("/login");
+  if (!session) {
+    return res.redirect(\`/login?next=\${encodeURIComponent(req.originalUrl)}\`);
+  }
   req.user = session.user;
   next();
 }
`;

const tasks: MockTask[] = [
  {
    summary: summary("expiring-share-links", "Add expiring share links to drafts", t0 - 95, {
      stats: stats({ files: 2, additions: 31, deletions: 1, uncommitted: 2 }),
      run: run({ state: "running", exit_code: null, started: t0 - 52, ended: null, cost_usd: null }),
      runs: 1,
    }),
    transcript: [
      { kind: "prompt", text: "Add expiring share links to drafts" },
      { kind: "text", text: "I'll look at how drafts are stored and routed first." },
      { kind: "tool", name: "Glob", detail: "src/**/*.ts" },
      { kind: "tool", name: "Read", detail: "src/routes/drafts.ts" },
      { kind: "tool", name: "Read", detail: "src/db/index.ts" },
      { kind: "tool", name: "Grep", detail: "shareLinks" },
      {
        kind: "text",
        text: "Drafts live in `db.drafts` and there's no sharing table yet. Plan:\n\n1. A `shareLinks` table with a token and an expiry\n2. `POST /drafts/:id/share` to create one\n3. `GET /s/:token` to open it until it expires",
      },
      { kind: "tool", name: "Write", detail: "src/drafts/share.ts" },
      { kind: "tool", name: "Edit", detail: "src/routes/drafts.ts" },
    ],
    script: [
      { kind: "ask", id: "req-1", tool: "Bash", detail: "pnpm tsc --noEmit", session: "`pnpm tsc` commands" },
      { kind: "tool", name: "Bash", detail: "pnpm tsc --noEmit" },
      { kind: "tool", name: "Bash", detail: "pnpm test drafts" },
      { kind: "text", text: "Types check and the draft tests pass. Adding a test for expiry next." },
      { kind: "tool", name: "Write", detail: "src/drafts/share.test.ts" },
    ],
    runs: [run({ state: "running", exit_code: null, started: t0 - 52, ended: null, cost_usd: null })],
    patch: SHARE_PATCH,
  },
  {
    summary: summary("login-redirect", "Send people back where they were after logging in", t0 - 3600 * 2, {
      stats: stats({ files: 1, additions: 3, deletions: 1, commits: 1, last_commit: t0 - 3000 }),
      run: run({ started: t0 - 3400, ended: t0 - 3010, cost_usd: 0.18 }),
      runs: 1,
      issue: 31,
      pull: 40,
    }),
    transcript: [
      { kind: "prompt", text: "Send people back where they were after logging in" },
      { kind: "text", text: "Let me find where unauthenticated requests are redirected." },
      { kind: "tool", name: "Grep", detail: "redirect(\"/login\")" },
      { kind: "tool", name: "Read", detail: "src/auth/session.ts" },
      { kind: "tool", name: "Read", detail: "src/routes/login.ts" },
      { kind: "tool", name: "Edit", detail: "src/auth/session.ts" },
      { kind: "tool", name: "Edit", detail: "src/routes/login.ts" },
      { kind: "tool", name: "Bash", detail: "pnpm test auth" },
      { kind: "tool", name: "Bash", detail: "git commit -am 'Return to the requested page after login'" },
      {
        kind: "text",
        text: "Done. `requireUser` now redirects to `/login?next=<original url>`, and the login route sends people to `next` after a successful sign-in — **only** if it's a relative path, so it can't be used as an open redirect.\n\nThe auth tests pass (14/14).",
      },
      { kind: "done", ok: true, summary: "Done.", cost_usd: 0.18, turns: 9, seconds: 390 },
    ],
    script: [],
    runs: [run({ started: t0 - 3400, ended: t0 - 3010, cost_usd: 0.18 })],
    patch: LOGIN_PATCH,
  },
  {
    summary: summary("dark-mode-dashboard", "Dark mode for the dashboard", t0 - 3600 * 5, {
      stats: stats({ files: 6, additions: 140, deletions: 38, commits: 2, base_moved: true, last_commit: t0 - 3600 * 4 }),
      run: run({ state: "failed", exit_code: 1, started: t0 - 3600 * 4 - 400, ended: t0 - 3600 * 4, cost_usd: 0.61 }),
      runs: 1,
    }),
    transcript: [
      { kind: "prompt", text: "Dark mode for the dashboard" },
      { kind: "tool", name: "Read", detail: "src/styles/theme.css" },
      { kind: "tool", name: "Edit", detail: "src/styles/theme.css" },
      { kind: "tool", name: "Bash", detail: "pnpm build" },
      { kind: "tool_error", text: "Bash: permission denied — shell commands are off for this task" },
      { kind: "text", text: "I couldn't run the build to check the result, so the chart colours are untested." },
      { kind: "done", ok: false, summary: "Exited with code 1", cost_usd: 0.61, turns: 14, seconds: 400 },
    ],
    script: [],
    runs: [run({ state: "failed", exit_code: 1, started: t0 - 3600 * 4 - 400, ended: t0 - 3600 * 4, cost_usd: 0.61 })],
    patch: "",
  },
  {
    summary: summary("csv-export", "CSV export for the analytics table", t0 - 86400 * 2, {
      landed: "9f2c1e4",
      run: run({ started: t0 - 86400 * 2, ended: t0 - 86400 * 2 + 300 }),
      runs: 1,
    }),
    transcript: [{ kind: "prompt", text: "CSV export for the analytics table" }],
    script: [],
    runs: [],
    patch: "",
  },
];

let undoable: { message: string; task: MockTask; index: number } | null = null;

// ---- Issues and the tasks' pull requests ----

const issue = (number: number, title: string, over: Partial<Issue> = {}): Issue => ({
  number,
  title,
  author: "ada",
  open: true,
  labels: [],
  assignees: ["you"],
  comments: 0,
  updated: t0 - 3600,
  url: `https://github.com/you/postplan/issues/${number}`,
  ...over,
});

const issues: IssueDetail[] = [
  {
    issue: issue(31, "Logging in always lands on the dashboard", { labels: ["bug", "auth"], comments: 2, updated: t0 - 7200 }),
    body: "When a signed-out person opens a link to a draft, they're sent to log in and then end up on the dashboard instead of the draft.\n\n**Expected:** back to the page they asked for.",
    comments: [
      { author: "grace", body: "Same with share links. Careful not to make it an open redirect.", at: t0 - 8000, verdict: null },
      { author: "ada", body: "Agreed — only relative paths.", at: t0 - 7300, verdict: null },
    ],
    prompt: "",
  },
  {
    issue: issue(34, "Scheduled posts ignore the account's time zone", { labels: ["bug"], comments: 1, updated: t0 - 1800 }),
    body: "A post scheduled for 09:00 goes out at 09:00 UTC, not 09:00 in the account's time zone (Europe/Oslo here).",
    comments: [{ author: "linus", body: "The cron job reads `scheduledAt` as UTC. The account has `timeZone` already.", at: t0 - 1900, verdict: null }],
    prompt: "",
  },
  {
    issue: issue(35, "Keyboard shortcut to duplicate a post", { labels: ["enhancement"], assignees: [], updated: t0 - 86400 }),
    body: "⌘D on a post in the calendar should duplicate it to the next free slot.",
    comments: [],
    prompt: "",
  },
  {
    issue: issue(29, "Export analytics as CSV", { open: false, labels: ["enhancement"], updated: t0 - 86400 * 2 }),
    body: "A button on the analytics table that downloads what it shows.",
    comments: [],
    prompt: "",
  },
];
for (const d of issues) {
  const i = d.issue;
  d.prompt =
    `${i.title} (#${i.number})\n\nGitHub issue #${i.number}: ${i.url}\n` +
    (i.labels.length ? `Labels: ${i.labels.join(", ")}\n` : "") +
    `\n${d.body}\n` +
    (d.comments.length ? `\nComments on the issue:\n\n${d.comments.map((c) => `${c.author} wrote:\n> ${c.body}`).join("\n\n")}\n` : "") +
    `\nResolve this issue in this folder. Keep the change focused on it, add or update tests where it makes sense, make sure it builds and the tests pass, and commit. The pull request will close #${i.number}.`;
}

const pull = (number: number, t: TaskSummary, over: Partial<Pull> = {}): Pull => ({
  number,
  title: t.title,
  author: "you",
  head: t.branch,
  base: t.base,
  state: "open",
  review: "none",
  checks: "pending",
  updated: now(),
  additions: t.stats.additions,
  deletions: t.stats.deletions,
  url: `https://github.com/you/postplan/pull/${number}`,
  ...over,
});

const taskPulls: Record<string, TaskPull> = {};
let nextPull = 41;
{
  const login = tasks.find((t) => t.summary.id === "login-redirect")!.summary;
  taskPulls[login.id] = {
    task: login.id,
    pull: pull(40, login, { review: "changesrequested", checks: "passing", updated: t0 - 1200 }),
    merge_commit: null,
  };
}


// The backend's watcher, for the tasks kept here: what changes them says so.
const listeners = new Map<string, Set<() => void>>();
export async function listen(event: string, fn: () => void): Promise<() => void> {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(fn);
  return () => listeners.get(event)!.delete(fn);
}
const tasksChanged = () => listeners.get("tasks-changed")?.forEach((fn) => fn());
const CHANGES_TASKS = new Set(["create_task", "send_to_agent", "stop_agent", "land_task", "discard_task", "undo", "answer_ask", "steer", "set_mode"]);

// The working task makes progress every few seconds, then finishes.
setInterval(() => {
  const t = tasks.find((x) => x.summary.run?.state === "running");
  if (!t || t.asks?.length) return;
  progress(t);
  tasksChanged();
}, 4000);

function progress(t: MockTask) {
  const next = t.script.shift();
  if (next) {
    t.transcript.push(next);
    if (next.kind === "ask") {
      t.asks = [{ id: next.id, tool: next.tool, detail: next.detail, session: next.session }];
      t.summary.asks = 1;
    }
    return;
  }
  const seconds = now() - (t.summary.run?.started ?? now());
  t.transcript.push({
    kind: "text",
    text: "Share links are in: `POST /drafts/:id/share` returns a link that works for 7 days (or `days` from the body), and expired links return 404. Tests cover creation and expiry.",
  });
  t.transcript.push({ kind: "done", ok: true, summary: "Done", cost_usd: 0.37, turns: 12, seconds });
  const finished = { ...t.summary.run!, state: "finished" as const, exit_code: 0, ended: now(), cost_usd: 0.37 };
  t.summary.run = finished;
  t.runs[t.runs.length - 1] = finished;
}

/** The prompt of a task's `i`th run, from its transcript. */
function promptOf(t: MockTask, i: number): string {
  const prompts = t.transcript.filter((x): x is Extract<Item, { kind: "prompt" }> => x.kind === "prompt");
  return prompts[i]?.text ?? "";
}

function find(id: string): MockTask {
  const t = tasks.find((x) => x.summary.id === id);
  if (!t) throw `No task ${id}`;
  return t;
}

function start(t: MockTask, prompt: string, agent: AgentChoice) {
  if (t.summary.run?.state === "running") throw `An agent is already working on ${t.summary.id}`;
  const r = run({
    index: t.runs.length,
    agent: agent.kind === "command" ? "command" : "claude",
    label:
      agent.kind === "command"
        ? "Custom command"
        : ["Claude Code", agent.kind === "claude" && agent.model, agent.kind === "claude" && agent.effort && `${agent.effort} effort`]
            .filter(Boolean)
            .join(" · "),
    state: "running",
    exit_code: null,
    started: now(),
    ended: null,
    cost_usd: null,
  });
  t.runs.push(r);
  t.summary.run = r;
  t.summary.runs = t.runs.length;
  t.transcript.push({ kind: "prompt", text: prompt });
  t.script = [
    { kind: "text", text: "On it." },
    { kind: "tool", name: "Read", detail: "src/routes/drafts.ts" },
    { kind: "tool", name: "Edit", detail: "src/routes/drafts.ts" },
    { kind: "tool", name: "Bash", detail: "pnpm test" },
  ];
}

const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .split("-")
    .filter((w) => w && !["the", "a", "an", "to", "on", "in", "of", "for", "and", "so", "it"].includes(w))
    .slice(0, 4)
    .join("-") || "task";

/** What talks to a remote or GitHub, or makes a folder, takes as long as it
 * would for real, so the UI's busy states show here too. */
const SLOW: Record<string, number> = {
  sync: 1500,
  publish_task: 2500,
  push_task: 1500,
  create_task: 1200,
  create_issue_task: 1200,
  land_task: 1000,
  pull_action: 1500,
  issue_action: 1200,
  run_action: 1200,
  review_feedback: 800,
  check_feedback: 900,
  job_feedback: 700,
  push_tag: 1200,
  delete_remote_branch: 1200,
  pulls: 900,
  issues: 900,
  runs: 900,
  pull_detail: 700,
  issue_detail: 700,
  run_detail: 700,
};

export async function invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  await new Promise((r) => setTimeout(r, SLOW[cmd] ?? 60));
  const id = args.id as string;
  const recorded = await fromFixtures(cmd, args);
  if (recorded !== undefined) return recorded as T;
  const result = ((): unknown => {
    switch (cmd) {
      case "current_repo":
        return repo;
      case "open_repo":
        return { ...repo, root: args.path as string };
      case "list_tasks":
        return [
          ...tasks.filter((t) => !(t.summary.landed || t.summary.closed)).sort((a, b) => b.summary.created - a.summary.created),
          ...tasks.filter((t) => t.summary.landed || t.summary.closed),
        ].map((t) => structuredClone(t.summary));
      case "task_detail": {
        const t = find(id);
        const last = t.runs.at(-1);
        const detail: TaskDetail = {
          task: t.summary,
          transcript: t.transcript,
          runs: t.runs.map((r, i) => ({ ...r, prompt: r.prompt || promptOf(t, i) })),
          conflicts: t.summary.id === "dark-mode-dashboard" ? ["src/styles/theme.css"] : [],
          undoable: last && !last.undone && last.state !== "running" ? last.index : null,
          asks: t.asks ?? [],
          listening: t.summary.run?.state === "running",
        };
        return structuredClone(detail);
      }
      case "task_patch":
        return find(id).patch;
      case "create_task": {
        const prompt = (args.prompt as string).trim();
        if (!prompt) throw "Describe the task first";
        const sid = slug(prompt);
        const t: MockTask = {
          summary: summary(sid, prompt, now(), { base: args.base as string }),
          transcript: [],
          runs: [],
          patch: "",
          script: [],
        };
        tasks.unshift(t);
        const agent = args.agent as AgentChoice;
        if (agent.kind !== "none") start(t, prompt, agent);
        return t.summary;
      }
      case "send_to_agent": {
        const t = find(id);
        start(t, (args.message as string | null) ?? t.summary.prompt, args.agent as AgentChoice);
        return null;
      }
      case "stop_agent": {
        const t = find(id);
        if (t.summary.run?.state !== "running") return null;
        const seconds = now() - t.summary.run.started;
        t.script = [];
        t.transcript.push({ kind: "done", ok: false, summary: "Stopped", cost_usd: null, turns: null, seconds });
        t.summary.run = { ...t.summary.run, state: "stopped", ended: now() };
        t.runs[t.runs.length - 1] = t.summary.run;
        return null;
      }
      case "land_task":
      case "discard_task": {
        const t = find(id);
        if (t.summary.run?.state === "running") throw "Stop the agent first";
        const index = tasks.indexOf(t);
        const landing = cmd === "land_task";
        const message = landing ? `Landed ${id} on ${t.summary.base}` : `Discarded ${id}`;
        undoable = { message, task: structuredClone(t), index };
        if (landing) t.summary.landed = "a1b2c3d";
        else tasks.splice(index, 1);
        return message;
      }
      case "undo": {
        if (!undoable) throw "Nothing to undo";
        const { message, task, index } = undoable;
        undoable = null;
        const existing = tasks.findIndex((x) => x.summary.id === task.summary.id);
        if (existing >= 0) tasks[existing] = task;
        else tasks.splice(index, 0, task);
        return `Undid: ${message}`;
      }
      case "open_folder":
        return null;
      case "answer_ask": {
        const t = find(id);
        const ask = t.asks?.find((a) => a.id === args.requestId);
        if (!ask) throw "That question was already answered";
        t.asks = t.asks!.filter((a) => a !== ask);
        t.summary.asks = t.asks.length;
        t.transcript.push({
          kind: "answer",
          id: ask.id,
          allow: !!args.allow,
          message: (args.message as string) ?? "",
          always: !!args.allow && !!args.always,
        });
        if (!args.allow) t.script = t.script.filter((x) => !(x.kind === "tool" && x.detail === ask.detail));
        return null;
      }
      case "steer": {
        const t = find(id);
        if (t.summary.run?.state !== "running") throw "The run has finished";
        t.transcript.push({ kind: "steer", text: args.text as string });
        return null;
      }
      case "set_mode":
        find(id).summary.mode = args.mode as Mode;
        return null;
      case "run_patch":
        return { patch: find(id).patch, from_base: [] };
      case "issues": {
        const filter = args.filter as string;
        return issues
          .map((d) => d.issue)
          .filter((i) =>
            filter === "closed" ? !i.open : i.open && (filter !== "assigned" || i.assignees.includes("you")) && (filter !== "yours" || i.author === "you"),
          );
      }
      case "issue_detail": {
        const d = issues.find((x) => x.issue.number === args.number);
        if (!d) throw `No issue #${args.number}`;
        return structuredClone(d);
      }
      case "issue_action": {
        const d = issues.find((x) => x.issue.number === args.number)!;
        if (args.action === "comment") {
          d.comments.push({ author: "you", body: args.body as string, at: now(), verdict: null });
          return `Commented on #${args.number}`;
        }
        d.issue.open = args.action === "reopen";
        return `${d.issue.open ? "Reopened" : "Closed"} #${args.number}`;
      }
      case "create_issue_task": {
        const n = args.number as number;
        const d = issues.find((x) => x.issue.number === n)!;
        if (tasks.some((t) => t.summary.issue === n && !(t.summary.landed || t.summary.closed))) throw `#${n} already has a task`;
        const prompt = (args.prompt as string).trim();
        const sid = `${n}-${slug(d.issue.title)}`;
        const t: MockTask = {
          summary: summary(sid, prompt, now(), { base: args.base as string, issue: n }),
          transcript: [],
          runs: [],
          patch: "",
          script: [],
        };
        tasks.unshift(t);
        const agent = args.agent as AgentChoice;
        if (agent.kind !== "none") start(t, prompt, agent);
        return t.summary;
      }
      case "publish_task": {
        const t = find(id);
        if (t.summary.run?.state === "running") throw "The agent is still working";
        if (t.summary.stats.commits === 0 && t.summary.stats.uncommitted === 0) throw `${id} has no commits to publish`;
        const n = nextPull++;
        t.summary.pull = n;
        t.summary.stats = { ...t.summary.stats, uncommitted: 0, unpushed: 0 };
        taskPulls[id] = { task: id, pull: pull(n, t.summary, { state: args.draft ? "draft" : "open" }), merge_commit: null };
        // CI passes after a while, and then someone merges it.
        setTimeout(() => (taskPulls[id].pull.checks = "passing"), 8000);
        setTimeout(() => {
          taskPulls[id].pull = { ...taskPulls[id].pull, state: "merged", review: "approved" };
          taskPulls[id].merge_commit = "c0ffee1d2e3f";
        }, 20000);
        return `Opened ${args.draft ? "draft " : ""}pull request #${n}`;
      }
      case "push_task": {
        const t = find(id);
        const k = t.summary.stats.unpushed;
        t.summary.stats = { ...t.summary.stats, unpushed: 0 };
        return k ? `Pushed ${k} commits to #${t.summary.pull}` : `${t.summary.branch} is up to date`;
      }
      case "task_pulls":
        return Object.values(taskPulls).filter((p) => tasks.some((t) => t.summary.id === p.task && !(t.summary.landed || t.summary.closed)));
      case "review_feedback": {
        const t = find(id);
        if (t.summary.pull == null) throw `${id} has no pull request`;
        return `Review on pull request #${t.summary.pull}:\n\ngrace (requested changes):\n> Keep \`next\` relative — reject \`//evil.example\` too.\n\nOn the diff:\n- \`src/routes/login.ts:12\` — Add a test for a protocol-relative URL. (grace)\n\nAddress each point (or say why not), make sure it still builds and the tests pass, and commit.`;
      }
      case "check_feedback": {
        const t = find(id);
        if (t.summary.pull == null) throw `${id} has no pull request`;
        return `CI failed on pull request #${t.summary.pull}:\n\n\`CI / test\` failed at step “Run tests”.\nThe end of its log:\n\`\`\`\nFAIL src/routes/login.test.ts\n  ✕ rejects protocol-relative URLs\n\`\`\`\n\nFind the cause, fix it, make sure it builds and the tests pass locally, and commit.`;
      }
      case "close_task": {
        const t = find(id);
        const index = tasks.indexOf(t);
        const message = `Closed ${id} — its pull request was closed`;
        undoable = { message, task: structuredClone(t), index };
        t.summary.closed = true;
        return message;
      }
      case "settle_task": {
        const t = find(id);
        const index = tasks.indexOf(t);
        const message = `Cleaned up ${id} — its pull request was merged`;
        undoable = { message, task: structuredClone(t), index };
        t.summary.landed = /^[0-9a-f]{7,}$/.test(args.commit as string) ? (args.commit as string) : "e3b9a10";
        return message;
      }
      case "undo_run": {
        const t = find(id);
        const last = t.runs.at(-1);
        if (!last || last.undone || last.state === "running") throw "There's no run to undo";
        last.undone = true;
        return `Undid run ${last.index + 1}`;
      }
      default:
        throw `Unknown command ${cmd}`;
    }
  })();
  if (CHANGES_TASKS.has(cmd)) tasksChanged();
  return result as T;
}

// ---- Git and GitHub, from recordings ----------------------------------------
//
// src/fixtures.json is what the real backend answered on a demo repository
// (e2e/record-fixtures.mjs). Times are moved up to now; changes to the
// working tree are applied to an in-memory copy so the Changes section can
// be clicked through.

type Fixtures = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
let fixtures: Fixtures | null = null;
const undoStack: string[] = [];

async function load(): Promise<Fixtures> {
  if (fixtures) return fixtures;
  const data = (await import("./fixtures.json")).default as Fixtures;
  const shift = Math.floor(Date.now() / 1000) - data.recordedAt;
  const TIMES = new Set(["time", "updated", "created", "at", "started", "completed", "last_commit"]);
  const walk = (v: unknown): unknown => {
    if (Array.isArray(v)) return v.map(walk);
    if (v && typeof v === "object")
      return Object.fromEntries(
        Object.entries(v).map(([k, x]) => [k, TIMES.has(k) && typeof x === "number" && x > 1e9 ? x + shift : walk(x)]),
      );
    return v;
  };
  fixtures = walk(data) as Fixtures;
  return fixtures;
}

async function fromFixtures(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  const f = await load();
  const n = args.number as number;
  const files = f.status as { path: string; staged: boolean; status: string }[];
  const move = (paths: string[], staged: boolean) => {
    for (const file of files)
      if (paths.includes(file.path) && file.status !== "conflicted") {
        file.staged = staged;
        file.status = staged ? "staged" : file.status === "untracked" ? "untracked" : "modified";
      }
  };
  const done = (label: string, result: unknown = label) => {
    undoStack.push(label);
    f.state = { ...f.state, index: String(Math.random()), undo: label };
    return result;
  };
  switch (cmd) {
    case "current_repo":
      return f.repo;
    case "repo_state":
      return f.state;
    case "undo": {
      // A landed or discarded mock task is undone by the agent mock above.
      if (undoable) return undefined;
      const label = undoStack.pop();
      if (!label) throw "Nothing to undo";
      return `Undid: ${label}`;
    }
    case "open_url":
      window.open(args.url as string, "_blank", "noopener");
      return null;
    case "graph":
      return f.graph;
    case "search_commits": {
      const q = (args.query as string).toLowerCase();
      return Object.values(f.commits as Record<string, { hash: string; message: string; author: string }>)
        .filter((c) => c.message.toLowerCase().includes(q) || c.author.toLowerCase().includes(q) || c.hash.startsWith(q))
        .map((c) => c.hash);
    }
    case "commit_detail":
      return f.commits[args.hash as string] ?? Promise.reject("No such commit in the recording");
    case "branches":
      return f.branches;
    case "checkout":
    case "create_branch":
      return done(`Switch to ${(args.branch ?? args.name) as string}`, `Switched to ${(args.branch ?? args.name) as string}`);
    case "delete_branch":
      return done(`Delete ${args.name}`, `Deleted ${args.name}`);
    case "cherry_pick":
    case "revert":
    case "merge_branch":
      return done(`${cmd.replace("_", " ")} ${String(args.hash ?? args.branch).slice(0, 7)}`);
    case "status":
      return files.map((x) => ({ ...x }));
    case "file_diff":
      return f.diffs[`${args.staged}:${args.path}`] ?? f.diffs[`${!args.staged}:${args.path}`] ?? "";
    case "stage":
      move(args.paths as string[], true);
      return done(`Stage ${(args.paths as string[]).join(", ")}`, null);
    case "unstage":
      move(args.paths as string[], false);
      return done(`Unstage ${(args.paths as string[]).join(", ")}`, null);
    case "stage_lines":
      return done(`Stage lines in ${args.path}`, null);
    case "discard":
      f.status = files.filter((x) => !(args.paths as string[]).includes(x.path) || x.staged);
      return done(`Discard ${(args.paths as string[]).join(", ")}`, null);
    case "commit":
      f.status = files.filter((x) => !x.staged);
      return done(`Commit “${String(args.message).split("\n")[0]}”`, "Committed a1b2c3d");
    case "last_message":
      return f.graph.rows.find((r: { head: boolean }) => r.head)?.subject ?? "";
    case "sync":
      return { fetch: "Fetched", pull: "Pulled", push: "Pushed" }[args.action as string];
    case "stashes":
      return f.stashes;
    case "stash":
      return "Stashed your changes";
    case "stash_pop":
      return "Applied and dropped the stash";
    case "resolve":
    case "continue_operation":
    case "abort_operation":
      return "Done";
    case "pulls":
      return f.pulls[args.filter as string] ?? [];
    case "pull_detail":
      return f.pullDetails[n] ?? Promise.reject(`No pull request #${n} in the recording`);
    case "pull_diff":
      return f.pullDiffs[n] ?? "";
    case "pull_action":
      return `${args.action} #${n}`;
    case "runs":
      return (f.runs as { branch: string; workflow: string }[]).filter(
        (r) => (!args.branch || r.branch === args.branch) && (!args.workflow || r.workflow === args.workflow),
      );
    case "workflows":
      return f.workflows;
    case "run_detail":
      return f.runDetails[args.id as number];
    case "run_action":
      return args.action === "cancel" ? "Cancelling the run" : "Re-running";
    case "job_log":
      return f.jobLogs[args.id as number] ?? [];
    case "job_feedback": {
      const lines = (f.jobLogs[args.job as number] ?? []) as { text: string }[];
      return `CI failed:\n\nThe end of its log:\n\`\`\`\n${lines.slice(-200).map((l) => l.text).join("\n")}\n\`\`\`\n\nFind the cause, fix it, make sure it builds and the tests pass locally, and commit.`;
    }
    case "askpass_pending":
      return null;
    case "askpass_answer":
    case "auto_fetch":
      return null;
    case "legacy_workspaces":
      return null;
    case "repo_heads":
      return (args.paths as string[]).map((path) => ({ path, exists: true, branch: "main", ahead: 0, behind: path.endsWith("postplan") ? 0 : 2, changes: 1 }));
    case "tags":
      return f.tags ?? [];
    case "worktrees":
      return [{ path: f.repo.root, branch: f.repo.branch, head: "", main: true, current: true, prunable: false, agent: false }];
    case "pr_badges":
      return (f.pulls?.open ?? []).map((p: { number: number; head: string; review: string; checks: string; state: string }) => ({
        branch: p.head,
        number: p.number,
        draft: p.state === "draft",
        review: p.review,
        checks: p.checks,
      }));
    case "pull_request_url":
      return `https://github.com/you/${f.repo.name}/compare`;
    case "rename_branch":
      return done(`Rename ${args.old} to ${args.new}`, `Renamed ${args.old} to ${args.new}`);
    case "delete_remote_branch":
      return `Deleted ${args.name} from the remote`;
    case "rebase_onto":
      return done(`Rebase onto ${String(args.onto).slice(0, 7)}`);
    case "reset_to":
      return done(`Reset to ${String(args.hash).slice(0, 7)}`, `Reset to ${String(args.hash).slice(0, 7)} (${args.mode})`);
    case "checkout_commit":
      return done(`Switch to ${String(args.hash).slice(0, 7)}`, `Switched to ${String(args.hash).slice(0, 7)}`);
    case "create_tag":
      f.tags = [{ name: args.name, hash: args.hash }, ...(f.tags ?? [])];
      return done(`Tag ${args.name}`, `Tagged ${String(args.hash).slice(0, 7)} as ${args.name}`);
    case "delete_tag":
      f.tags = (f.tags ?? []).filter((t: { name: string }) => t.name !== args.name);
      return done(`Delete tag ${args.name}`, `Deleted tag ${args.name}`);
    case "push_tag":
      return args.delete ? `Deleted tag ${args.name} from origin` : `Pushed tag ${args.name} to origin`;
    case "remove_worktree":
      return `Removed the worktree at ${args.path}`;
    case "rewrite_plan": {
      const rows = f.graph.rows as { hash: string; short: string; subject: string; author: string; time: number }[];
      const i = rows.findIndex((r) => r.hash === args.base);
      if (i <= 0) return [];
      return rows
        .slice(0, i)
        .reverse()
        .map((r) => ({ hash: r.hash, short: r.short, subject: r.subject, message: r.subject, author: r.author, time: r.time }));
    }
    case "rewrite":
      return done(`Rewrite history after ${String(args.base).slice(0, 7)}`, "Rewrote history");
    case "stash_apply":
      return "Applied the stash — it's still in the list";
    case "stash_drop":
      f.stashes = (f.stashes as { index: number }[]).filter((x) => x.index !== args.index);
      return done("Drop stash", "Dropped the stash");
    case "blame": {
      const rows = f.graph.rows as { hash: string; subject: string; author: string; time: number }[];
      return Array.from({ length: 24 }, (_, i) => {
        const r = rows[Math.floor(i / 5) % rows.length];
        return { hash: r.hash, author: r.author, time: r.time, summary: r.subject, line: i + 1, text: `line ${i + 1} of ${args.path}` };
      });
    }
    case "conflict_regions":
      return [
        { kind: "text", text: "# postplan\n\nSchedule posts.\n" },
        { kind: "conflict", ours: "Install with pip.\n", ours_label: "HEAD", theirs: "Install with uv.\n", theirs_label: "fix/readme-install", base: null },
        { kind: "text", text: "\n## Usage\n" },
      ];
    case "resolve_regions":
      return `Resolved ${args.path}`;
    case "merge_into":
      return done(`Merge ${args.source} into ${args.target}`);
    case "pull_request_between":
      return `https://github.com/you/${f.repo.name}/compare/${args.target}...${args.source}?expand=1`;
    case "skip_operation":
      return "Skipped that commit";
    default:
      return undefined;
  }
}
