// End-to-end check of agent tasks in the real app (Tauri + spor's core),
// through WebDriver.
//
//   tauri-driver &            # needs WebKitWebDriver (Linux: webkit2gtk-driver)
//   SPOR_REPO=/path/to/a/scratch/repo node e2e/run.mjs path/to/spor-desktop out-dir
//
// It drives one task through the whole loop (create, agent works, review
// the diff, comment on a line, send feedback, land, undo) and checks the
// repository with git at each step. Use a throwaway repository: it lands.

import { existsSync, readFileSync } from "node:fs";
import { check, gitIn, open, sleep } from "./webdriver.mjs";

const [app, out = "e2e-shots"] = process.argv.slice(2);
const repo = process.env.SPOR_REPO;
if (!app || !repo) {
  console.error("usage: SPOR_REPO=<repo> node e2e/run.mjs <app binary> [out dir]");
  process.exit(2);
}
const git = gitIn(repo);
const w = await open(app, out);

try {
  await w.js("localStorage.clear(); localStorage.setItem('spor.section', JSON.stringify('agents'))");
  await w.refresh();
  console.log("New task");
  await w.waitFor(".new-task textarea");
  const before = git("rev-parse", "main");
  await w.fill(".new-task textarea", "Let greet() take an optional title");
  await w.enter(".new-task textarea");
  await w.waitFor(".task-page .live-line");
  check(git("branch", "--list", "agent/*").includes("agent/let-greet-take-optional"), "task branch created");
  await sleep(4500);
  await w.shot("1-working");

  console.log("Agent finishes");
  await w.waitFor(".run .run-end", 30000);
  await w.shot("2-finished");
  const transcript = await w.text(".conversation");
  // The stand-in agent's first run has a refused tool call, so it stays
  // open (a run with a failure never folds) and says what failed.
  check(!transcript.includes("Worked for"), "a run with a refused tool call doesn't fold");
  check(transcript.includes("1 failed"), "the refusal is visible in the tool summary");
  check(transcript.includes("now takes an optional"), "agent's final message is shown");

  console.log("Review");
  await w.clickText(".tabs button", "Changes");
  await w.waitFor(".diff tr.add");
  await w.js(`document.querySelectorAll(".diff tr.add .add-comment button")[1].click()`);
  await w.fill(".comment-editor textarea", "Also add a farewell() like this one");
  await w.enter(".comment-editor textarea");
  await w.waitFor(".composer .chip");
  check((await w.count(".line-comment")) === 1, "the comment shows once, on its line");
  await w.shot("3-review");

  console.log("Feedback resumes the session");
  await w.enter(".composer-dock textarea");
  await w.waitFor(".task-page .live-line");
  await w.waitFor(".run:nth-of-type(2) .fold", 30000);
  const second = await w.text(".conversation");
  check(second.includes("Comments on the changes"), "line comment was sent with the feedback");
  check(second.includes("Worked for"), "a clean finished run folds behind 'Worked for'");
  check(second.includes("Added farewell()") || second.includes("Added `farewell()`"), "second run resumed and finished");
  await w.shot("4-second-run");

  console.log("Each run's changes, and undoing a run (#19)");
  const worktree = git("worktree", "list", "--porcelain").split("\n").find((l) => l.includes("let-greet-take-optional"))?.replace("worktree ", "");
  const read = (f) => readFileSync(`${worktree}/${f}`, "utf8");
  await w.clickText(".run:nth-of-type(2) .run-actions .link", "Changes in this run");
  await w.waitFor(".source-bar select");
  await w.until(`return document.querySelector(".source-bar select").value === "1" && document.querySelectorAll(".diff tr.add").length > 0`, "run 2's diff");
  const added = await w.js(`return [...document.querySelectorAll(".diff tr.add")].map(r => r.innerText).join(" | ")`);
  check(added.includes("def farewell") && !added.includes("title=None"), "Changes can show just one run's diff");
  await w.clickText(".tabs button", "Conversation");
  await w.waitFor(".run:nth-of-type(2) .run-actions");
  await w.clickText(".run:nth-of-type(2) .run-actions .link", "Undo this run");
  await w.clickText(".run:nth-of-type(2) .run-actions .link", "Undo this run?");
  await w.waitFor(".run.undone");
  check(!read("greet.py").includes("farewell") && read("greet.py").includes("title=None"), "Undo this run puts the folder back as the first run left it");
  const metaDir = `${repo}/.git/spor/tasks/let-greet-take-optional.d`;
  check(existsSync(`${metaDir}/run-1.undone`), "the run is marked undone");
  await w.shot("4b-run-undone");
  await w.js(`document.querySelector(".toast .link").click()`);
  await w.until(`return !document.querySelector(".run.undone")`, "the redo");
  check(read("greet.py").includes("farewell") && !existsSync(`${metaDir}/run-1.undone`), "Undo on the toast brings the run back");

  console.log("Land");
  await w.click(".inspector .split .primary");
  await w.waitFor(".toast");
  await sleep(500);
  check(git("log", "-1", "--format=%s", "main") === "Let greet() take an optional title", "landed as one commit on main");
  check(git("log", "-1", "--format=%B", "main").includes("Spor-Task:"), "landing commit has the Spor-Task trailer");
  check(!git("branch", "--list", "agent/*"), "task branch removed after landing");
  await w.shot("5-landed");

  console.log("Undo");
  await w.js(`document.querySelector(".toast .link").click()`);
  await sleep(1500);
  check(git("rev-parse", "main") === before, "undo put main back");
  check(git("branch", "--list", "agent/*").includes("agent/let-greet-take-optional"), "undo restored the task branch");
  await w.shot("6-undone");
  console.log("All good");
} finally {
  await w.close();
}
