// End-to-end check of a task's setup script (#23) in the real app.
//
//   SPOR_REPO=<scratch repo> node e2e/setup.mjs <app binary> [out dir]
//
// The repository has a committed `.spor/setup` that fails the first time
// (it leaves a marker) and works the second, and a `.spor/copy` listing an
// ignored `.env` that exists in the main checkout.

import { readFileSync, existsSync } from "node:fs";
import { check, gitIn, open } from "./webdriver.mjs";

const [app, out = "e2e-shots"] = process.argv.slice(2);
const repo = process.env.SPOR_REPO;
const git = gitIn(repo);
const w = await open(app, out);

try {
  await w.js("localStorage.clear(); localStorage.setItem('spor.section', JSON.stringify('agents'))");
  await w.refresh();
  await w.waitFor(".new-task textarea");
  await w.fill(".new-task textarea", "Let greet() take an optional title");
  await w.enter(".new-task textarea");
  await w.waitFor(".task-page");
  const worktree = git("worktree", "list", "--porcelain")
    .split("\n")
    .find((l) => l.startsWith("worktree ") && l.includes(".spor/"))
    ?.replace("worktree ", "");
  check(readFileSync(`${worktree}/.env`, "utf8") === "TOKEN=demo\n", ".spor/copy brings the ignored .env into the new folder");

  console.log("A failing setup");
  await w.waitFor(".setup-block.bad", 20000);
  const failed = await w.text(".setup-block");
  check(failed.includes("Setup failed (exit 1)") && failed.includes("pip: can't reach the index"), "a failed setup says so, with its output");
  check(!existsSync(`${worktree}/test_greet.py`), "…and the agent didn't start");
  await w.shot("setup-1-failed");

  console.log("Retry");
  await w.clickText(".setup-block .link", "Retry");
  await w.waitFor(".composer .ask-card", 30000);
  await w.clickText(".ask-card button", "Allow");
  await w.waitFor(".run:nth-of-type(2) .fold", 30000);
  await w.click(".run:nth-of-type(2) .fold");
  await w.until(`return $(".run:nth-of-type(2) .setup-block.good")`, "the second setup");
  check((await w.text(".run:nth-of-type(2) .setup-block")).includes("Set up the folder"), "Retry runs setup again, and it works");
  check(existsSync(`${worktree}/test_greet.py`), "…then the agent runs");
  await w.shot("setup-2-retried");
  console.log("All good");
} finally {
  await w.close();
}
