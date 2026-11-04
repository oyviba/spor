// A stand-in backend for working on the UI in a plain browser (`pnpm dev`).
// It keeps a few tasks in memory, one of which is "working" and grows its
// transcript over time, and answers the same commands as src-tauri.

import type { AgentChoice, Item, RepoInfo, RunInfo, TaskDetail, TaskSummary } from "./types";

const now = () => Math.floor(Date.now() / 1000);
const t0 = now();

interface MockTask {
  summary: TaskSummary;
  transcript: Item[];
  runs: RunInfo[];
  patch: string;
  /** Items still to "arrive" while it runs. */
  script: Item[];
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
  stats: stats(),
  run: null,
  runs: 0,
  command: `cd /Users/you/code/postplan.spor/${id} && claude '${prompt}'`,
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

// The working task makes progress every few seconds, then finishes.
setInterval(() => {
  const t = tasks.find((x) => x.summary.run?.state === "running");
  if (!t) return;
  const next = t.script.shift();
  if (next) {
    t.transcript.push(next);
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
}, 4000);

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
    label: agent.kind === "command" ? "Custom command" : "Claude Code",
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
    .filter(Boolean)
    .slice(0, 4)
    .join("-") || "task";

export async function invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  await new Promise((r) => setTimeout(r, 60));
  const id = args.id as string;
  const result = ((): unknown => {
    switch (cmd) {
      case "current_repo":
        return repo;
      case "open_repo":
        return { ...repo, root: args.path as string };
      case "list_tasks":
        return [
          ...tasks.filter((t) => !t.summary.landed).sort((a, b) => b.summary.created - a.summary.created),
          ...tasks.filter((t) => t.summary.landed),
        ].map((t) => structuredClone(t.summary));
      case "task_detail": {
        const t = find(id);
        const detail: TaskDetail = {
          task: t.summary,
          transcript: t.transcript,
          runs: t.runs,
          conflicts: t.summary.id === "dark-mode-dashboard" ? ["src/styles/theme.css"] : [],
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
      default:
        throw `Unknown command ${cmd}`;
    }
  })();
  return result as T;
}
