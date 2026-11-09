// End-to-end check of History (with its uncommitted changes), Pull requests and Actions in the
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

// Sections by their rail label, so adding one doesn't shift the rest.
const section = async (label, ready) => {
  const sel = `.nav-section[aria-label="${label}"]`;
  // A repository's sections aren't shown in the Inbox or Agents: the
  // workspace's badge goes back into the repository first.
  if (!(await w.js(`return !!$(arguments[0])`, sel))) {
    await w.js(`$('.workspace-badge[data-current]').click()`);
    await w.waitFor(sel);
  }
  await w.js(`$(arguments[0]).click()`, sel);
  await w.waitFor(ready);
};
// History with the working tree selected: its files in the panel.
const working = async (ready = ".file-row") => {
  await section("History", ".worktree-row");
  await w.js(`$(".worktree-row").click()`);
  await w.waitFor(ready);
};
// Right-click the first element matching `css` whose text has `text`.
const rightClick = (css, text) =>
  w.js(
    `const el = [...$$(arguments[0])].find(e => e.textContent.includes(arguments[1]));
     const r = el.getBoundingClientRect();
     el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 10 }));`,
    css,
    text,
  );
const menuPick = async (text) => {
  await w.waitFor(".menu.floating");
  await w.clickText(".menu.floating button", text);
};
const answer = async (text) => {
  await w.waitFor(".dialog input");
  await w.fill(".dialog input", text);
  await w.js(`$(".dialog").requestSubmit()`);
};
const undoKey = () => w.js(`document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }))`);
// Dismiss toasts the way a person would (removing them from the DOM would
// pull nodes out from under React).
const toastGone = () => w.js(`$$(".toast .icon-button").forEach(b => b.click())`);
const rowClick = (css, i, shift = false) =>
  w.js(`$$(arguments[0])[arguments[1]].dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: arguments[2] }))`, css, i, shift);

try {
  await w.js("localStorage.clear(); localStorage.setItem('spor.section', JSON.stringify('history'))");
  await w.refresh();
  await w.waitFor(".nav-section");
  await working();
  const head = git("rev-parse", "HEAD");

  console.log("Uncommitted changes");
  check((await w.count(".file-row")) === 3, "lists the staged, edited and untracked files");
  check(!(await w.js(`return !!$('.nav-section[aria-label="History"] .rail-badge')`)), "the rail has no count per section");
  await w.clickText(".file-row-main", "greet.py");
  await w.until(`return $(".tabs [aria-selected=true]")?.textContent === "greet.py" && $$(".diff tr.add").length === 4`, "greet.py's diff");
  // Pick the two lines of shout(): click one, shift-click the next.
  await rowClick(".diff tr.add", 2);
  await rowClick(".diff tr.add", 3, true);
  check((await w.text(".selection-bar")).includes("2 lines picked"), "picks two lines with click and shift-click");
  await w.shot("changes-1-picked");
  await w.clickText(".selection-bar button", "Stage lines");
  await w.until(`return $$(".file-row").length === 4`, "greet.py in both lists");
  check(git("diff", "--cached", "greet.py").includes("+def shout(name):"), "stages just the picked lines");
  check(!git("diff", "greet.py").includes("+def shout") && git("diff", "greet.py").includes("+\n"), "only the two blank lines stay unstaged");
  await toastGone();
  await undoKey();
  await w.until(`return $$(".file-row").length === 3`, "the undo");
  check(!git("diff", "--cached", "greet.py").includes("shout"), "⌘Z unstages the lines again");

  await w.js(`[...$$(".file-row")].find(r => r.textContent.includes("NOTES.md")).querySelector(".row-actions .icon-button:last-child").click()`);
  await w.until(`return [...$$(".file-row")].filter(r => r.textContent.includes("NOTES.md")).length === 1 && $(".list-label")?.textContent.includes("2")`, "NOTES.md staged");
  check(git("status", "--porcelain", "NOTES.md").startsWith("A "), "stages a whole file from its row");
  await w.fill(".inspector-commit textarea", "Add notes and polish the README");
  await w.key(".inspector-commit textarea", "Enter", { ctrlKey: true });
  await w.until(`return $$(".file-row").length === 1`, "the commit");
  check(git("log", "-1", "--format=%s") === "Add notes and polish the README", "commits with ⌘↩");
  check(git("status", "--porcelain").trim() === "M greet.py", "leaves the unstaged edit alone");
  await w.shot("changes-2-committed");
  await w.clickText(".toast .link", "Undo");
  await w.until(`return $$(".file-row").length === 3`, "the commit's undo");
  check(git("rev-parse", "HEAD") === head, "Undo takes the commit back");
  check(git("status", "--porcelain", "NOTES.md").startsWith("A "), "…keeping its changes staged");

  console.log("History");
  await toastGone();
  await section("History", ".graph-row");
  const total = Number(git("rev-list", "--all", "--count"));
  const shown = await w.until(`const t = $(".task-title .small-text")?.textContent; return t && t.match(/[\\d,]+/)[0]`, "the commit count");
  check(Number(shown.replace(/,/g, "")) === total, `shows all ${total} commits`);
  check((await w.text(".graph-row .ref.current")) === "main", "marks the checked-out branch");
  await w.fill(".sidebar-search input", "farewell");
  await w.until(`return $$(".graph-row.dimmed").length > 0`, "search");
  const hits = await w.count(".graph-row:not(.dimmed)");
  const expected = git("log", "--all", "--format=%h", "-i", "--grep=farewell").split("\n").filter(Boolean).length;
  check(hits >= expected, `search finds the ${expected} commits that mention farewell`);
  await w.fill(".sidebar-search input", "");
  await w.clickText(".branch-name", "feat/farewell");
  await w.until(`return $(".graph-row.selected")?.dataset.hash === arguments[0]`, "feat/farewell selected", 5000, git("rev-parse", "feat/farewell"));
  check(true, "clicking a branch selects its tip");
  await w.waitFor(".inspector .card-title");
  check((await w.text(".inspector .card-title")) === "Tidy farewell", "the card shows the commit");
  await w.shot("history-1-selected");
  await w.clickText(".inspector button", "Branch here");
  await w.fill(".inline-form input", "try/farewell");
  await w.js(`$(".inline-form").requestSubmit()`);
  // The staged README would be overwritten: the app offers to stash.
  await w.until(`return $(".toast.error")?.innerText.includes("Stash and switch")`, "the stash offer");
  check((await w.text(".toast.error")).includes("would overwrite uncommitted changes"), "a blocked switch says why");
  check(!git("branch", "--list", "try/farewell"), "…and changes nothing");
  await w.shot("history-2-blocked");
  await w.clickText(".toast.error .link", "Stash and switch");
  await w.until(`return $(".repo-branch")?.textContent.includes("try/farewell")`, "the switch");
  check(git("rev-parse", "--abbrev-ref", "HEAD") === "try/farewell", "Stash and switch creates the branch and switches");
  check(git("stash", "list").includes("spor-auto-stash"), "…with the changes stashed");
  await toastGone();
  await undoKey();
  await w.until(`return $(".repo-branch")?.textContent.includes("main")`, "switching back");
  await sleep(300);
  await undoKey();
  await sleep(1200);
  check(git("rev-parse", "--abbrev-ref", "HEAD") === "main" && !git("branch", "--list", "try/farewell"), "⌘Z twice: back on main, branch gone");
  await working(".stash-row");
  await w.clickText(".stash-row .link", "Pop");
  await w.until(`return $$(".file-row").length === 3`, "the stash popped");
  check(git("stash", "list") === "" && git("status", "--porcelain").includes("NOTES.md"), "Pop brings the stashed work back");

  console.log("Branches and tags");
  await toastGone();
  await section("History", ".graph-row");
  await rightClick(".branch-row", "feat/farewell");
  await menuPick("Rename");
  await answer("feat/goodbye");
  await w.until(`return [...$$(".branch-name")].some(b => b.textContent.includes("feat/goodbye"))`, "the rename");
  check(git("branch", "--list", "feat/goodbye") !== "" && git("branch", "--list", "feat/farewell") === "", "Rename renames the branch");
  await toastGone();
  await undoKey();
  await w.until(`return [...$$(".branch-name")].some(b => b.textContent.includes("feat/farewell"))`, "the rename undone");
  check(git("branch", "--list", "feat/farewell") !== "", "⌘Z renames it back");
  const tip = git("rev-parse", "HEAD");
  await w.js(
    `const row = $(\`.graph-row[data-hash="\${arguments[0]}"]\`); const r = row.getBoundingClientRect();
     row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 40, clientY: r.top + 10 }));`,
    tip,
  );
  await menuPick("Tag here");
  await answer("v0.1.0");
  await w.until(`return $(".toast")?.innerText.includes("v0.1.0")`, "the tag");
  check(git("rev-parse", "v0.1.0^{commit}") === tip, "Tag here tags the commit");
  await w.until(`return [...$$(".graph-row .ref.tag")].some(r => r.textContent.includes("v0.1.0"))`, "the tag label");
  check(true, "…and the graph shows it");
  await w.shot("history-3-tagged");
  await toastGone();
  await undoKey();
  await sleep(1200);
  check(git("tag", "--list", "v0.1.0") === "", "⌘Z deletes the tag again");
  await working(".inspector");

  console.log("Hand to an agent");
  await toastGone();
  const before = git("status", "--porcelain");
  git("checkout", "--quiet", "-b", "feat/handoff");
  await w.until(`return [...$$(".work-header button")].some(b => b.textContent.includes("Hand to an agent"))`, "the branch's work header");
  check((await w.text(".work-header .stage-track [aria-current]")) === "In progress", "Changes shows the checked-out branch's stage");
  await w.clickText(".work-header button", "Hand to an agent");
  await answer("Look it over");
  await w.until(`return $(".repo-branch")?.textContent.includes("main")`, "your checkout back on main");
  const folder = `${repo}.spor/handoff`;
  check(git("status", "--porcelain") === "", "Hand to an agent leaves your checkout on main, clean");
  check(gitIn(folder)("branch", "--show-current") === "feat/handoff", "…with the branch in its own folder");
  check(gitIn(folder)("status", "--porcelain") === before, "…and your uncommitted work there, staged as it was");
  await w.waitFor(".task-page .stage-track");
  check((await w.text(".task-header h2")) === "Look it over", "…and opens the task on the work page");
  await w.shot("handoff-1-task");
  await toastGone();
  await undoKey();
  await w.until(`return $(".repo-branch")?.textContent.includes("feat/handoff")`, "the branch back", 30000);
  check(git("status", "--porcelain") === before, "⌘Z brings the branch and its work back to your checkout");
  check(!readdirSync(`${repo}.spor`).includes("handoff"), "…and the folder goes");
  git("checkout", "--quiet", "main");
  git("branch", "--quiet", "-D", "feat/handoff");
  await working(".inspector");

  console.log("A merge that conflicts");
  await toastGone();
  await w.clickText(".inspector .link", "Stash changes");
  await w.until(`return $$(".stash-row").length === 1`, "the stash");
  check(git("status", "--porcelain") === "", "stash clears the working tree");
  await section("History", ".graph-row");
  await w.js(`[...$$(".branch-row")].find(r => r.textContent.includes("fix/readme-install")).querySelector("[title^='Merge']").click()`);
  await w.waitFor(".op-banner.bad");
  check((await w.text(".op-banner")).includes("Merge stopped on conflicts in 1 file"), "the banner says the merge stopped");
  await w.shot("conflict-1-banner");
  await w.clickText(".op-banner button", "Resolve conflicts");
  await w.waitFor(".resolve-bar");
  check((await w.text(".tabs [aria-selected=true]")) === "README.md", "Resolve opens the conflicted file in History");
  await w.shot("conflict-2-resolve");
  await w.clickText(".resolve-bar button", "Take theirs");
  await w.until(`return $(".op-banner") && !$(".op-banner.bad")`, "the conflict resolved");
  await w.clickText(".op-banner button", "Continue");
  await w.until(`return !$(".op-banner")`, "the merge to finish");
  check(git("log", "-1", "--format=%P").split(" ").length === 2, "Continue makes the merge commit");
  check(git("show", "HEAD:README.md").includes("Install with uv."), "with their side of README.md");
  // The stash changed README's line 2, the merge line 3: git calls that a
  // conflict, keeps the stash, and leaves README.md to resolve.
  await w.clickText(".stash-row .link", "Pop");
  const said = await w.until(
    `return [...$$(".toast.error")].map(t => t.innerText).find(t => t.includes("The stash conflicts"))`,
    "the pop's conflict",
  );
  check(said.includes("README.md") && said.includes("The stash is kept"), "a conflicting pop names the file and keeps the stash");
  await w.until(`return $(".list-label")?.textContent.startsWith("Conflicts")`, "the conflicts list");
  await w.clickText(".file-row-main", "README.md");
  await w.waitFor(".resolve-bar");
  await w.shot("conflict-3-stash");
  await w.clickText(".resolve-bar button", "Take theirs");
  await w.until(`return !$(".resolve-bar")`, "README resolved");
  check(!git("status", "--porcelain").includes("UU"), "Take theirs resolves it");
  check(git("show", ":README.md").includes("politely"), "…with the stash's side");
  check(git("status", "--porcelain").includes("NOTES.md") && git("stash", "list").includes("WIP"), "the rest of the stash is back, and the stash is still there");

  console.log("Pull requests");
  await toastGone();
  await section("Pull requests", ".task-row");
  check((await w.count(".task-row")) === 3, "lists the three open pull requests");
  await w.clickText(".task-row", "#12");
  await w.waitFor(".timeline");
  check((await w.text(".task-header")).includes("Checks failing"), "#12 says its checks are failing");
  check((await w.text(".inspector")).includes("Task:"), "the work card links #12 to the task on its branch");
  await w.fill(".composer-dock textarea", "Close — just the keyword-only nit left.");
  await w.clickText(".composer-dock button", "Comment");
  await w.until(`return $(".timeline").innerText.includes("keyword-only nit")`, "the comment");
  check(ghState().comments["12"]?.[0]?.body.includes("keyword-only nit"), "Comment posts through gh");
  await w.shot("pulls-1-conversation");
  await w.clickText(".tabs button", "Changes");
  await w.waitFor(".diff tr.add");
  check((await w.count(".file")) === 2, "Files shows the PR's diff");
  await w.clickText(".tabs button", "Checks");
  await w.waitFor(".check-list");
  await w.shot("pulls-2-checks");
  await w.clickText(".check-list .link", "Open in Actions");
  await w.waitFor(".log-section.failed");
  check((await w.text(".job-tab.active")).includes("test"), "Open in Actions lands on the failed job");
  check((await w.text(".log-section.failed")).includes("AssertionError"), "with the failing step open");
  await section("Pull requests", ".task-row");
  await w.clickText(".task-row", "#11");
  await w.until(`return $(".task-header")?.innerText.includes("#11")`, "#11");
  await w.click(".inspector .split .primary");
  check((await w.text(".inspector .split")).includes("Land #11?"), "landing asks once more");
  await w.click(".inspector .split .danger");
  await w.until(`return $(".task-header .stage-track [aria-current]")?.innerText === "Landed"`, "the landing");
  check(ghState().merged.includes(11), "Land merges through gh");
  await w.shot("pulls-3-merged");

  console.log("Inbox");
  await toastGone();
  await section("Inbox", ".task-row");
  await w.until(`return $$(".task-list .task-row").length === 2`, "the Inbox's rows", 10000);
  const inbox = await w.text(".task-list");
  check(inbox.includes("CI failed on agent/let-greet-take-optional"), "the Inbox lists the failed run on the agent's branch");
  check(inbox.includes("#12") && !inbox.includes("#11"), "…and #12's review request, but not the merged #11");
  await w.shot("inbox-1");
  check(await w.js(`return !!document.querySelector('.nav-section[aria-label="Inbox"] .rail-badge')`), "the Inbox has the rail's badge");
  await w.clickText(".task-row", "CI failed");
  await w.waitFor(".task-page");
  check(
    await w.js(`return document.querySelector('.nav-section[aria-label="Work"]').getAttribute("aria-current") === "page"`),
    "the run is on the agent's branch, so it opens the task in Work",
  );

  console.log("Actions");
  await toastGone();
  await section("Actions", ".task-row");
  await w.clickText(".task-row", "CI #88");
  await w.waitFor(".log-section.failed");
  await w.shot("actions-1-failed");
  const tasks = `${repo}/.git/spor/tasks/let-greet-take-optional.d`;
  const runsBefore = readdirSync(tasks).filter((f) => f.endsWith(".meta")).length;
  await w.until(`return [...$$(".inspector button")].some(b => b.textContent.includes("Send failure"))`, "the agent button");
  await w.clickText(".inspector button", "Send failure to agent");
  await w.waitFor(".task-page .bubble");
  await w.until(`return [...$$(".bubble")].some(b => b.innerText.includes("CI failed"))`, "the agent page");
  const meta = readFileSync(`${tasks}/run-${runsBefore}.meta`, "utf8");
  check(meta.includes("CI failed") && meta.includes("AssertionError"), "the agent gets the failing log");
  await w.shot("actions-2-sent");
  // Back to the run: a re-run makes it live again, and the card follows.
  await section("Actions", ".task-row");
  await w.clickText(".task-row", "CI #88");
  await w.waitFor(".log-section.failed");
  await w.clickText(".inspector button", "Re-run failed");
  await w.until(`return $(".toast")?.innerText.includes("Re-running")`, "the re-run");
  check(ghState().reruns.includes(501), "Re-run failed goes through gh");
  await toastGone();
  await w.until(`return [...$$(".inspector button")].some(b => b.textContent.includes("Cancel"))`, "the live run", 30000);
  check(true, "a re-run run is live: Cancel replaces Re-run");
  console.log("All good");
} finally {
  await w.close();
}
