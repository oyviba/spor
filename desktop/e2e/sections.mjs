// End-to-end check of Changes, History, Pull requests and Actions in the
// real app, through WebDriver, checking the repository with git as it goes.
//
//   SPOR_REPO=<scratch repo> FAKE_GH_STATE=<file> node e2e/sections.mjs <app binary> [out dir]
//
// It expects the scratch repository the README describes: on `main` with a
// staged README.md, an edited greet.py, an untracked NOTES.md, a branch
// `fix/readme-install` that conflicts with main in README.md, and an agent
// task on `agent/let-greet-take-optional`. GitHub comes from a stand-in
// `gh` on PATH that records what's done in $FAKE_GH_STATE.

import { readdirSync, readFileSync } from "node:fs";
import { check, gitIn, open, sleep } from "./webdriver.mjs";

const [app, out = "e2e-shots"] = process.argv.slice(2);
const repo = process.env.SPOR_REPO;
const ghState = () => JSON.parse(readFileSync(process.env.FAKE_GH_STATE, "utf8"));
const git = gitIn(repo);
const w = await open(app, out);

const section = async (n, ready) => {
  await w.js(`document.querySelectorAll(".rail-item")[arguments[0]].click()`, n);
  await w.waitFor(ready);
};
const undoKey = () => w.js(`document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`);
// Dismiss toasts the way a person would (removing them from the DOM would
// pull nodes out from under React).
const toastGone = () => w.js(`document.querySelectorAll(".toast .icon-button").forEach(b => b.click())`);
const rowClick = (css, i, shift = false) =>
  w.js(`document.querySelectorAll(arguments[0])[arguments[1]].dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: arguments[2] }))`, css, i, shift);

try {
  await w.js("localStorage.clear(); localStorage.setItem('spor.section', JSON.stringify('changes'))");
  await w.refresh();
  await w.waitFor(".rail-item");
  const head = git("rev-parse", "HEAD");

  console.log("Changes");
  await w.waitFor(".file-row");
  check((await w.count(".file-row")) === 3, "lists the staged, edited and untracked files");
  await w.until(`return document.querySelector(".rail-badge")?.textContent === "3"`, "the rail's count");
  check(true, "the rail counts 3 changed files");
  await w.clickText(".file-row-main", "greet.py");
  await w.until(`return document.querySelector("h2")?.textContent === "greet.py" && document.querySelectorAll(".diff tr.add").length === 4`, "greet.py's diff");
  // Pick the two lines of shout(): click one, shift-click the next.
  await rowClick(".diff tr.add", 2);
  await rowClick(".diff tr.add", 3, true);
  check((await w.text(".selection-bar")).includes("2 lines picked"), "picks two lines with click and shift-click");
  await w.shot("changes-1-picked");
  await w.clickText(".selection-bar button", "Stage lines");
  await w.until(`return document.querySelectorAll(".file-row").length === 4`, "greet.py in both lists");
  check(git("diff", "--cached", "greet.py").includes("+def shout(name):"), "stages just the picked lines");
  check(!git("diff", "greet.py").includes("+def shout") && git("diff", "greet.py").includes("+\n"), "only the two blank lines stay unstaged");
  await toastGone();
  await undoKey();
  await w.until(`return document.querySelectorAll(".file-row").length === 3`, "the undo");
  check(!git("diff", "--cached", "greet.py").includes("shout"), "⌘Z unstages the lines again");

  await w.js(`[...document.querySelectorAll(".file-row")].find(r => r.textContent.includes("NOTES.md")).querySelector(".row-actions .icon-button:last-child").click()`);
  await w.until(`return [...document.querySelectorAll(".file-row")].filter(r => r.textContent.includes("NOTES.md")).length === 1 && document.querySelector(".list-label")?.textContent.includes("2")`, "NOTES.md staged");
  check(git("status", "--porcelain", "NOTES.md").startsWith("A "), "stages a whole file from its row");
  await w.fill(".composer-dock textarea", "Add notes and polish the README");
  await w.key(".composer-dock textarea", "Enter", { ctrlKey: true });
  await w.until(`return document.querySelectorAll(".file-row").length === 1`, "the commit");
  check(git("log", "-1", "--format=%s") === "Add notes and polish the README", "commits with ⌘↩");
  check(git("status", "--porcelain").trim() === "M greet.py", "leaves the unstaged edit alone");
  await w.shot("changes-2-committed");
  await w.js(`document.querySelector(".toast .link").click()`);
  await w.until(`return document.querySelectorAll(".file-row").length === 3`, "the commit's undo");
  check(git("rev-parse", "HEAD") === head, "Undo takes the commit back");
  check(git("status", "--porcelain", "NOTES.md").startsWith("A "), "…keeping its changes staged");

  console.log("History");
  await toastGone();
  await section(1, ".graph-row");
  const total = Number(git("rev-list", "--all", "--count"));
  const shown = await w.until(`const t = document.querySelector(".task-title .small-text")?.textContent; return t && t.match(/[\\d,]+/)[0]`, "the commit count");
  check(Number(shown.replace(/,/g, "")) === total, `shows all ${total} commits`);
  check((await w.text(".graph-row .ref.current")) === "main", "marks the checked-out branch");
  await w.fill(".sidebar-search input", "farewell");
  await w.until(`return document.querySelectorAll(".graph-row.dimmed").length > 0`, "search");
  const hits = await w.count(".graph-row:not(.dimmed)");
  const expected = git("log", "--all", "--format=%h", "-i", "--grep=farewell").split("\n").filter(Boolean).length;
  check(hits >= expected, `search finds the ${expected} commits that mention farewell`);
  await w.fill(".sidebar-search input", "");
  await w.clickText(".branch-name", "feat/farewell");
  await w.until(`return document.querySelector(".graph-row.selected")?.dataset.hash === arguments[0]`, "feat/farewell selected", 5000, git("rev-parse", "feat/farewell"));
  check(true, "clicking a branch selects its tip");
  await w.waitFor(".inspector .card-title");
  check((await w.text(".inspector .card-title")) === "Tidy farewell", "the card shows the commit");
  await w.shot("history-1-selected");
  await w.clickText(".inspector button", "Branch here");
  await w.fill(".inline-form input", "try/farewell");
  await w.js(`document.querySelector(".inline-form").requestSubmit()`);
  // The staged README would be overwritten: the app offers to stash.
  await w.until(`return document.querySelector(".toast.error")?.innerText.includes("Stash and switch")`, "the stash offer");
  check((await w.text(".toast.error")).includes("would overwrite uncommitted changes"), "a blocked switch says why");
  check(!git("branch", "--list", "try/farewell"), "…and changes nothing");
  await w.shot("history-2-blocked");
  await w.clickText(".toast.error .link", "Stash and switch");
  await w.until(`return document.querySelector(".repo-branch")?.textContent.includes("try/farewell")`, "the switch");
  check(git("rev-parse", "--abbrev-ref", "HEAD") === "try/farewell", "Stash and switch creates the branch and switches");
  check(git("stash", "list").includes("spor-auto-stash"), "…with the changes stashed");
  await toastGone();
  await undoKey();
  await w.until(`return document.querySelector(".repo-branch")?.textContent.includes("main")`, "switching back");
  await sleep(300);
  await undoKey();
  await sleep(1200);
  check(git("rev-parse", "--abbrev-ref", "HEAD") === "main" && !git("branch", "--list", "try/farewell"), "⌘Z twice: back on main, branch gone");
  await section(0, ".stash-row");
  await w.clickText(".stash-row .link", "Pop");
  await w.until(`return document.querySelectorAll(".file-row").length === 3`, "the stash popped");
  check(git("stash", "list") === "" && git("status", "--porcelain").includes("NOTES.md"), "Pop brings the stashed work back");

  console.log("A merge that conflicts");
  await toastGone();
  await w.clickText(".inspector .link", "Stash changes");
  await w.until(`return document.querySelectorAll(".stash-row").length === 1`, "the stash");
  check(git("status", "--porcelain") === "", "stash clears the working tree");
  await section(1, ".graph-row");
  await w.js(`[...document.querySelectorAll(".branch-row")].find(r => r.textContent.includes("fix/readme-install")).querySelector("[title^='Merge']").click()`);
  await w.waitFor(".op-banner.bad");
  check((await w.text(".op-banner")).includes("Merge stopped on conflicts in 1 file"), "the banner says the merge stopped");
  await w.shot("conflict-1-banner");
  await w.clickText(".op-banner button", "Resolve in Changes");
  await w.waitFor(".resolve-bar");
  check((await w.text("h2")) === "README.md", "Resolve opens the conflicted file in Changes");
  await w.shot("conflict-2-resolve");
  await w.clickText(".resolve-bar button", "Take theirs");
  await w.until(`return document.querySelector(".op-banner") && !document.querySelector(".op-banner.bad")`, "the conflict resolved");
  await w.clickText(".op-banner button", "Continue");
  await w.until(`return !document.querySelector(".op-banner")`, "the merge to finish");
  check(git("log", "-1", "--format=%P").split(" ").length === 2, "Continue makes the merge commit");
  check(git("show", "HEAD:README.md").includes("Install with uv."), "with their side of README.md");
  // The stash changed README's line 2, the merge line 3: git calls that a
  // conflict, keeps the stash, and leaves README.md to resolve.
  await w.clickText(".stash-row .link", "Pop");
  const said = await w.until(
    `return [...document.querySelectorAll(".toast.error")].map(t => t.innerText).find(t => t.includes("The stash conflicts"))`,
    "the pop's conflict",
  );
  check(said.includes("README.md") && said.includes("The stash is kept"), "a conflicting pop names the file and keeps the stash");
  await w.until(`return document.querySelector(".list-label")?.textContent.startsWith("Conflicts")`, "the conflicts list");
  await w.clickText(".file-row-main", "README.md");
  await w.waitFor(".resolve-bar");
  await w.shot("conflict-3-stash");
  await w.clickText(".resolve-bar button", "Take theirs");
  await w.until(`return !document.querySelector(".resolve-bar")`, "README resolved");
  check(!git("status", "--porcelain").includes("UU"), "Take theirs resolves it");
  check(git("show", ":README.md").includes("politely"), "…with the stash's side");
  check(git("status", "--porcelain").includes("NOTES.md") && git("stash", "list").includes("WIP"), "the rest of the stash is back, and the stash is still there");

  console.log("Pull requests");
  await toastGone();
  await section(2, ".task-row");
  check((await w.count(".task-row")) === 3, "lists the three open pull requests");
  await w.clickText(".task-row", "#12");
  await w.waitFor(".timeline");
  check((await w.text(".task-header")).includes("Checks failing"), "#12 says its checks are failing");
  check((await w.text(".inspector")).includes("Agent task:"), "links #12 to the agent task on its branch");
  await w.fill(".composer-dock textarea", "Close — just the keyword-only nit left.");
  await w.clickText(".composer-dock button", "Comment");
  await w.until(`return document.querySelector(".timeline").innerText.includes("keyword-only nit")`, "the comment");
  check(ghState().comments["12"]?.[0]?.body.includes("keyword-only nit"), "Comment posts through gh");
  await w.shot("pulls-1-conversation");
  await w.clickText(".tabs button", "Files");
  await w.waitFor(".diff tr.add");
  check((await w.count(".file")) === 2, "Files shows the PR's diff");
  await w.clickText(".tabs button", "Checks");
  await w.waitFor(".check-list");
  await w.shot("pulls-2-checks");
  await w.clickText(".check-list .link", "Open in Actions");
  await w.waitFor(".log-section.failed");
  check((await w.text(".job-tab.active")).includes("test"), "Open in Actions lands on the failed job");
  check((await w.text(".log-section.failed")).includes("AssertionError"), "with the failing step open");
  await section(2, ".task-row");
  await w.clickText(".task-row", "#11");
  await w.until(`return document.querySelector(".task-header")?.innerText.includes("#11")`, "#11");
  await w.click(".inspector .split .primary");
  check((await w.text(".inspector .split")).includes("Squash and merge?"), "merging asks once more");
  await w.click(".inspector .split .danger");
  await w.until(`return document.querySelector(".task-header")?.innerText.includes("Merged")`, "the merge");
  check(ghState().merged.includes(11), "Squash and merge merges through gh");
  await w.shot("pulls-3-merged");

  console.log("Actions");
  await toastGone();
  await section(3, ".task-row");
  await w.clickText(".task-row", "CI #88");
  await w.waitFor(".log-section.failed");
  await w.shot("actions-1-failed");
  const tasks = `${repo}/.git/spor/tasks/let-greet-take-optional.d`;
  const runsBefore = readdirSync(tasks).filter((f) => f.endsWith(".meta")).length;
  await w.until(`return [...document.querySelectorAll(".inspector button")].some(b => b.textContent.includes("Send failure"))`, "the agent button");
  await w.clickText(".inspector button", "Send failure to agent");
  await w.waitFor(".task-page .bubble");
  await w.until(`return [...document.querySelectorAll(".bubble")].some(b => b.innerText.includes("CI failed"))`, "the agent page");
  const meta = readFileSync(`${tasks}/run-${runsBefore}.meta`, "utf8");
  check(meta.includes("CI failed") && meta.includes("AssertionError"), "the agent gets the failing log");
  await w.shot("actions-2-sent");
  // Back to the run: a re-run makes it live again, and the card follows.
  await section(3, ".task-row");
  await w.clickText(".task-row", "CI #88");
  await w.waitFor(".log-section.failed");
  await w.clickText(".inspector button", "Re-run failed");
  await w.until(`return document.querySelector(".toast")?.innerText.includes("Re-running")`, "the re-run");
  check(ghState().reruns.includes(501), "Re-run failed goes through gh");
  await toastGone();
  await w.until(`return [...document.querySelectorAll(".inspector button")].some(b => b.textContent.includes("Cancel"))`, "the live run", 30000);
  check(true, "a re-run run is live: Cancel replaces Re-run");
  console.log("All good");
} finally {
  await w.close();
}
