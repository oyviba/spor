// End-to-end check of agent tasks in the real app (Tauri + spor's core),
// through WebDriver, in Work: every work item in the repository (#81).
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
  // Stored from before Work replaced Agents: it should open Work.
  await w.js("localStorage.clear(); localStorage.setItem('spor.section', JSON.stringify('agents'))");
  await w.refresh();
  console.log("Work");
  await w.waitFor(`.nav-section[aria-label="Work"][aria-current="page"]`);
  check(true, "a stored Agents section opens Work");
  await w.until(`return [...$$(".task-row[data-kind=branch]")].some(r => r.innerText.includes("Document how to call greet()"))`, "your branch");
  check((await w.text(".task-list")).includes("In progress"), "your own branch with commits ahead of main is listed, in progress");
  await w.clickText(".segmented button", "Agents");
  check(!(await w.count(".task-row[data-kind=branch]")), "the Agents filter leaves it out");
  await w.clickText(".segmented button", "All");

  console.log("New task");
  await w.click(".new-work");
  await w.waitFor(".menu.floating");
  await w.clickText(".menu.floating button", "Agent task");
  await w.waitFor(".new-task textarea");
  const before = git("rev-parse", "main");
  await w.fill(".new-task textarea", "Let greet() take an optional title");
  await w.enter(".new-task textarea");
  await w.waitFor(".task-page .live-line");
  // The branch is three random words (#111), so read it back.
  const taskBranch = git("branch", "--list", "agent/*").replace(/^[*+]\s*/, "").trim();
  const taskId = taskBranch.replace("agent/", "");
  check(/^agent\/[a-z]+-[a-z]+-[a-z]+$/.test(taskBranch), "task branch created, named with three random words");
  await w.until(`return $(".task-row.selected[data-kind=task]")?.innerText.includes("Let greet() take an optional title")`, "the task's row");
  check(true, "the task is listed next to your branch, and selected");

  console.log("Steering it while it works (#22)");
  check((await w.text(".composer-footer .chip-toggle")).includes("queues"), "while it works, ↩ queues by default");
  await w.fill(".composer-dock textarea", "Keep the docstring short");
  check((await w.text(".hint-keys")).includes("⌘↩ steer"), "…and ⌘↩ steers");
  await w.key(".composer-dock textarea", "Enter", { ctrlKey: true });
  await w.until(`return [...$$(".bubble.steer")].some(b => b.innerText.includes("Keep the docstring short"))`, "the steer bubble");
  await w.until(`return $(".conversation").innerText.includes("Noted: Keep the docstring short")`, "the agent taking it", 15000);
  check(true, "a steered message goes into the running turn, and the agent takes it");
  check(!(await w.count(".queued-item")), "…without queueing it");
  await sleep(4500);
  await w.shot("1-working");

  console.log("It asks before running the tests (#21)");
  await w.waitFor(".composer .ask-card", 30000);
  check((await w.text(".ask-card")).includes("python -m pytest -q"), "the question shows in the composer, with the command");
  // The task list reloads apart from the page; give it a beat to catch up.
  await w.until(`return $(".task-row .pill")?.innerText.includes("Needs you")`, "the Needs you pill", 5000);
  check(true, "the task says it needs you");
  check((await w.text(".list-label.needs-you")) === "Needs you", "…and moves to the top of the list");
  await w.until(`return $('.nav-section[aria-label="Inbox"] .rail-badge.asking')?.textContent === "1"`, "the rail's count", 6000);
  check(true, "the Inbox counts it in amber");
  await w.shot("2a-asking");

  console.log("The Inbox has it (#42)");
  await w.js(`$('.nav-section[aria-label="Inbox"]').click()`);
  await w.waitFor(".list-label.needs-you");
  check((await w.text(".task-list")).includes("Let greet() take an optional title"), "the Inbox lists the question");
  await w.clickText(".task-row", "Let greet()");
  await w.waitFor(".composer .ask-card");
  check(true, "clicking it opens the task, with its question");
  await w.clickText(".ask-card button", "Allow");

  console.log("Agent finishes");
  await w.waitFor(".run .fold", 30000);
  await w.shot("2-finished");
  const transcript = await w.text(".conversation");
  check(transcript.includes("The tests pass."), "Allow lets it run the tests");
  await w.click(".run .fold");
  await w.until(`return $(".ask-line.allowed")?.innerText.includes("Allowed")`, "the answered question");
  check(true, "the transcript shows the question, allowed");
  check(!(await w.count(".ask-card")), "the question is gone from the composer");

  console.log("Review");
  await w.clickText(".tabs button", "Changes");
  await w.waitFor(".diff tr.add");
  await w.js(`$$(".diff tr.add .add-comment button")[1].click()`);
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
  const worktree = git("worktree", "list", "--porcelain").split("\n").find((l) => l.includes(taskId))?.replace("worktree ", "");
  const read = (f) => readFileSync(`${worktree}/${f}`, "utf8");
  await w.clickText(".run:nth-of-type(2) .run-actions .link", "Changes in this run");
  await w.waitFor(".source-bar select");
  await w.until(`return $(".source-bar select").value === "1" && $$(".diff tr.add").length > 0`, "run 2's diff");
  const added = await w.js(`return [...$$(".diff tr.add")].map(r => r.innerText).join(" | ")`);
  check(added.includes("def farewell") && !added.includes("title=None"), "Changes can show just one run's diff");
  await w.clickText(".tabs button", "Agent");
  await w.waitFor(".run:nth-of-type(2) .run-actions");
  await w.clickText(".run:nth-of-type(2) .run-actions .link", "Undo this run");
  await w.clickText(".run:nth-of-type(2) .run-actions .link", "Undo this run?");
  await w.waitFor(".run.undone");
  check(!read("greet.py").includes("farewell") && read("greet.py").includes("title=None"), "Undo this run puts the folder back as the first run left it");
  const metaDir = `${repo}/.git/spor/tasks/${taskId}.d`;
  check(existsSync(`${metaDir}/run-1.undone`), "the run is marked undone");
  await w.shot("4b-run-undone");
  await w.clickText(".toast .link", "Undo");
  await w.until(`return !$(".run.undone")`, "the redo");
  check(read("greet.py").includes("farewell") && !existsSync(`${metaDir}/run-1.undone`), "Undo on the toast brings the run back");

  console.log("Land");
  await w.click(".inspector .split .primary");
  await w.waitFor(".toast");
  await sleep(500);
  check(git("log", "-1", "--format=%s", "main") === "Let greet() take an optional title", "landed as one commit on main");
  check(git("log", "-1", "--format=%B", "main").includes("Spor-Task:"), "landing commit has the Spor-Task trailer");
  check(!git("branch", "--list", "agent/*"), "task branch removed after landing");
  await w.until(`return [...$$(".shelf-header")].some(h => h.innerText.includes("Settled"))`, "the Settled shelf");
  check(true, "the landed task moves to Settled");
  await w.shot("5-landed");

  console.log("Undo");
  await w.clickText(".toast .link", "Undo");
  await sleep(1500);
  check(git("rev-parse", "main") === before, "undo put main back");
  check(git("branch", "--list", "agent/*").includes(taskBranch), "undo restored the task branch");
  await w.shot("6-undone");
  console.log("All good");
} finally {
  await w.close();
}
