// Record what the real backend answers on a demo repository, for the
// browser mock (src/mock.ts) to serve. Run against the scratch repository
// the e2e README describes, with the stand-in `gh` on PATH:
//
//   SPOR_REPO=<demo> node e2e/record-fixtures.mjs <app binary> src/fixtures.json

import { writeFileSync } from "node:fs";
import { open } from "./webdriver.mjs";

const [app, out = "src/fixtures.json"] = process.argv.slice(2);
const w = await open(app, "/tmp");
const call = (cmd, args = {}) =>
  w.js(`return window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1])`, cmd, args);

try {
  await w.waitFor(".nav-section");
  const graph = await call("graph", { limit: 2000 });
  const fixtures = {
    recordedAt: Math.floor(Date.now() / 1000),
    repo: await call("current_repo"),
    state: await call("repo_state"),
    graph,
    branches: await call("branches"),
    commits: {},
    status: await call("status"),
    diffs: {},
    stashes: await call("stashes"),
    pulls: {},
    pullDetails: {},
    pullDiffs: {},
    runs: await call("runs", { branch: null, workflow: null }),
    workflows: await call("workflows"),
    runDetails: {},
    jobLogs: {},
  };
  for (const row of graph.rows) fixtures.commits[row.hash] = await call("commit_detail", { hash: row.hash });
  for (const f of fixtures.status)
    fixtures.diffs[`${f.staged}:${f.path}`] = await call("file_diff", { path: f.path, staged: f.staged });
  for (const filter of ["open", "yours", "review", "closed"]) fixtures.pulls[filter] = await call("pulls", { filter });
  for (const p of [...fixtures.pulls.open, ...fixtures.pulls.closed]) {
    fixtures.pullDetails[p.number] = await call("pull_detail", { number: p.number });
    fixtures.pullDiffs[p.number] = await call("pull_diff", { number: p.number });
  }
  for (const r of fixtures.runs) {
    const d = await call("run_detail", { id: r.id });
    fixtures.runDetails[r.id] = d;
    for (const j of d.jobs)
      if (j.status !== "running" && j.status !== "queued") fixtures.jobLogs[j.id] = await call("job_log", { id: j.id });
  }
  // Paths and times from the recording machine mean nothing elsewhere.
  fixtures.repo.root = "/Users/you/code/greeter";
  fixtures.repo.name = "greeter";
  writeFileSync(out, JSON.stringify(fixtures));
  console.log(`Recorded ${graph.rows.length} commits, ${fixtures.status.length} changed files, ${Object.keys(fixtures.pullDetails).length} PRs, ${fixtures.runs.length} runs → ${out}`);
} finally {
  await w.close();
}
