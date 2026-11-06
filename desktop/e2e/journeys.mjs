// The five journeys in docs/journeys.md, as end-to-end tests (#75): each one
// in its target form, step by step, counting how often it changes section
// against the budget the document gives it.
//
//   SPOR_REPO=<scratch>/journeys FAKE_GH_STATE=<file> node e2e/journeys.mjs <app binary> [out dir]
//
// The repository is make-repos.sh's `journeys`: greeter on GitHub's
// demo/greeter as the stand-in `gh` has it, pushing to a bare repository
// beside it. Agents are the stand-in `claude`.
//
// A step the app can't do yet is a *gap*: it's marked with what it waits
// for, and its failure is reported, not fatal. A gap that starts working is
// reported too, so docs/journeys.md can say so. Anything else that fails
// fails the suite. CI runs it without failing the build until #74 is done.

import { readFileSync, writeFileSync } from "node:fs";
import { gitIn, open, sleep } from "./webdriver.mjs";

const [app, out = "e2e-shots"] = process.argv.slice(2);
const repo = process.env.SPOR_REPO;
if (!app || !repo) {
  console.error("usage: SPOR_REPO=<repo> FAKE_GH_STATE=<file> node e2e/journeys.mjs <app binary> [out dir]");
  process.exit(2);
}
const ghState = () => JSON.parse(readFileSync(process.env.FAKE_GH_STATE, "utf8"));
const git = gitIn(repo);
const bare = gitIn(`${repo}-origin.git`);
const w = await open(app, out);

const must = (ok, what) => {
  if (!ok) throw new Error(what);
};
const section = (label) => w.js(`$(\`.rail-item[aria-label="\${arguments[0]}"]\`).click()`, label);
const toastGone = () => w.js(`$$(".toast .icon-button").forEach(b => b.click())`);
const answer = async (text) => {
  await w.waitFor(".dialog input");
  await w.fill(".dialog input", text);
  await w.js(`$(".dialog").requestSubmit()`);
};
const button = (css, text) => w.js(`return [...$$(arguments[0])].some(b => b.textContent.includes(arguments[1]))`, css, text);

// Which sections a journey has been in, in order: the rail's current item,
// watched as it changes.
const watchSections = () =>
  w.js(`
    const now = () => document.querySelector('.rail-item[aria-current="page"]')?.getAttribute("aria-label");
    window.__sections = [now()];
    if (!window.__watching) {
      window.__watching = new MutationObserver(() => {
        const s = now();
        if (s && window.__sections.at(-1) !== s) window.__sections.push(s);
      });
      window.__watching.observe(document.querySelector(".rail"), { subtree: true, attributes: true, attributeFilter: ["aria-current"] });
    }`);
const sectionsSeen = () => w.js(`return window.__sections`);

const results = [];

/**
 * One journey: start in `start`, run its steps, and check it changed
 * section no more than `budget` times. A step with `gap` is one the app
 * can't do yet; when it fails the journey stops there if it `blocks` (the
 * rest needs it), else goes on.
 */
async function journey(name, start, budget, run) {
  console.log(`\n${name}`);
  await toastGone();
  await section(start);
  await w.waitFor(`.rail-item[aria-label="${start}"][aria-current="page"]`);
  await sleep(500);
  await watchSections();
  let stopped = false;
  let gaps = 0;
  const step = async (what, fn, gap) => {
    if (stopped) {
      console.log(`  --  ${what} (not reached)`);
      return;
    }
    try {
      await fn();
      if (gap) {
        console.log(`  !!  ${what}: works now, so it's not a gap any more (update docs/journeys.md)`);
        results.push({ name, what, kind: "fixed" });
      } else console.log(`  ok  ${what}`);
    } catch (e) {
      if (gap) {
        console.log(`  ..  ${what}: not yet (${gap.why})`);
        results.push({ name, what, kind: "gap" });
        gaps++;
        if (gap.blocks) stopped = true;
      } else {
        console.log(`  FAIL  ${what}: ${e.message.split("\n")[0]}`);
        results.push({ name, what, kind: "fail" });
        stopped = true;
      }
    }
  };
  try {
    await run(step);
  } catch (e) {
    console.log(`  FAIL  ${e.message.split("\n")[0]}`);
    results.push({ name, what: "the journey", kind: "fail" });
    stopped = true;
  }
  const seen = await sectionsSeen();
  const changes = seen.length - 1;
  const trail = seen.join(" → ");
  if (stopped && gaps === 0) console.log(`  --  the section budget isn't counted for a journey that failed`);
  else if (stopped) console.log(`  ..  ${changes} section changes so far (${trail}); the budget is ${budget}`);
  else if (changes <= budget) console.log(`  ok  ${changes} of ${budget} section changes: ${trail}`);
  else {
    console.log(`  FAIL  ${changes} section changes, over the budget of ${budget}: ${trail}`);
    results.push({ name, what: "the section budget", kind: "fail" });
  }
  await w.shot(name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, ""));
}

try {
  await w.js("localStorage.clear()");
  await w.refresh();
  await w.waitFor(".rail");

  // 1 ──────────────────────────────────────────────────────────────────────
  await journey("1. Morning triage", "Inbox", 1, async (step) => {
    await step("the Inbox has what waits on you: review requests and a failed run on your branch", () =>
      w.until(
        `const t = $(".task-list")?.innerText ?? ""; return t.includes("Review requested") && t.includes("Failed runs")`,
        "the Inbox's lists",
        30000,
      ),
    );
    await step("the first one opens where you deal with it", async () => {
      await w.click(".task-list .task-row");
      await w.until(`return [...$$(".inspector button")].some(b => b.textContent.includes("Re-run failed"))`, "the failed run");
    });
    await step("…and its work card says whose branch it is", async () => {
      must((await w.text(".inspector .work-card")).includes("agent/let-greet-take-optional"), "the card names the branch");
    });
    await step("dealing with it: Re-run failed", async () => {
      await w.clickText(".inspector button", "Re-run failed");
      await w.until(`return $(".toast")?.innerText.includes("Re-running")`, "the re-run");
      must(ghState().reruns.includes(501), "the re-run went through gh");
    });
  });

  // 3 ──────────────────────────────────────────────────────────────────────
  await journey("3. Review a teammate's pull request", "Inbox", 1, async (step) => {
    await step("the review request opens on the work page", async () => {
      await w.until(`return [...$$(".task-row")].some(r => r.innerText.includes("Let greet() take an optional title"))`, "the review row");
      await w.clickText(".task-row", "Let greet() take an optional title");
      await w.waitFor(".timeline");
      must((await w.text(".task-header .stage-track [aria-current]")) === "In review", "it's in review");
    });
    await step("read the conversation", async () => must((await w.text(".timeline")).includes("Made by a spor agent task"), "the description"));
    await step("read the diff and leave a line comment", async () => {
      await w.clickText(".tabs button", "Changes");
      await w.waitFor(".diff tr.add");
      await w.js(`$$(".diff tr.add .add-comment button")[0].click()`);
      await w.fill(".comment-editor textarea", "Make the title keyword-only");
      await w.enter(".comment-editor textarea");
      await w.waitFor(".composer-dock .chip");
    });
    await step(
      "see why a check failed, on the page",
      async () => {
        await w.clickText(".tabs button", "Checks");
        await w.waitFor(".check-list");
        // The failing step's log, folded by step, as Actions shows it.
        await w.until(`return $(".task-body .log-section.failed")`, "the failing step's log", 5000);
      },
      { why: "#80: the Checks tab links to the job in Actions; it doesn't show the failing step's log itself", blocks: false },
    );
    await step("approve, with the line comment", async () => {
      await w.clickText(".tabs button", "Changes");
      await w.clickText(".composer-dock button", "Approve");
      await w.until(`return $(".toast")?.innerText.includes("Approved")`, "the approval");
      const state = ghState();
      must(state.approved.includes(12), "approved through gh");
      must(state.comments["12"]?.some((c) => c.body.includes("keyword-only")), "with the line comment in its body");
    });
  });

  // 2 ──────────────────────────────────────────────────────────────────────
  await journey("2. From an issue to a landed pull request, with an agent", "Issues", 1, async (step) => {
    let pr = 0;
    let task = "";
    await step("pick an issue assigned to you", async () => {
      await w.until(`return [...$$(".task-row")].some(r => r.innerText.includes("optional title"))`, "the issue");
      await w.clickText(".task-row", "optional title");
      await w.until(`return [...$$(".composer-footer .primary")].some(b => b.textContent.includes("Start with agent"))`, "Start with agent");
    });
    await step("start an agent on it: its task opens on the work page", async () => {
      await w.clickText(".composer-footer .primary", "Start with agent");
      await w.waitFor(".task-page .live-line", 30000);
      must((await w.text(".task-header h2")).includes("optional title"), "the task's page");
      // A branch checked out in a folder lists as "+ agent/…".
      task = git("branch", "--list", "agent/7-*").replace(/^[*+]\s*/, "").trim();
      must(task, "its branch is named after the issue");
    });
    await step("answer its question", async () => {
      await w.waitFor(".composer .ask-card", 30000);
      await w.clickText(".ask-card button", "Allow");
      await w.waitFor(".run .fold", 30000);
      must((await w.text(".conversation")).includes("The tests pass."), "it ran the tests");
    });
    await step("review its changes with a line comment, and send the feedback", async () => {
      await w.clickText(".tabs button", "Changes");
      await w.waitFor(".diff tr.add");
      await w.js(`$$(".diff tr.add .add-comment button")[1].click()`);
      await w.fill(".comment-editor textarea", "Also add a farewell() like this one");
      await w.enter(".comment-editor textarea");
      await w.waitFor(".composer .chip");
      await w.enter(".composer-dock textarea");
      await w.waitFor(".run:nth-of-type(2) .fold", 30000);
      must((await w.text(".conversation")).includes("farewell"), "the agent took it");
    });
    await step("publish it: its pull request opens", async () => {
      await w.until(`return [...$$(".inspector button")].some(b => b.textContent.trim() === "Publish" && !b.disabled)`, "Publish");
      await w.js(`[...$$(".inspector button")].find(b => b.textContent.trim() === "Publish").click()`);
      await w.until(`return $(".toast")?.innerText.includes("Published")`, "the pull request", 30000);
      pr = ghState().created.find((p) => p.headRefName === task)?.number;
      must(pr, "gh opened it from the task's branch");
      must(bare("rev-parse", task) === git("rev-parse", task), "the branch is pushed");
    });
    await step("watch its checks, on the same page", async () => {
      await w.until(`return [...$$(".tabs button")].some(b => b.textContent.includes("Checks"))`, "the Checks tab", 40000);
      await w.clickText(".tabs button", "Checks");
      await w.waitFor(".check-list");
      must((await w.text(".check-list")).includes("test"), "the PR's checks");
    });
    await step("land it, and the task cleans up", async () => {
      await toastGone();
      await w.until(`return [...$$(".inspector button")].some(b => b.textContent.includes("Land #"))`, "Land");
      await w.clickText(".inspector button", "Land #");
      await w.clickText(".inspector button", "Land #");
      await w.until(`return $(".toast")?.innerText.includes("Cleaned up")`, "the clean-up", 30000);
      must(ghState().merged.includes(pr), "landed through gh");
      must(!git("branch", "--list", task), "its branch is gone");
    });
  });

  // 5 ──────────────────────────────────────────────────────────────────────
  await journey("5. Three agents at once", "Work", 0, async (step) => {
    const prompts = ["Add a farewell() for mornings", "Add a farewell() for evenings", "Add a farewell() for weekends"];
    await step("start three tasks, staying put for the next", async () => {
      for (const p of prompts) {
        await w.click(".new-work");
        await w.waitFor(".menu.floating");
        await w.clickText(".menu.floating button", "Agent task");
        await w.waitFor(".new-task textarea");
        await w.fill(".new-task textarea", p);
        await w.key(".new-task textarea", "Enter", { ctrlKey: true });
        await w.until(`return $(".toast")?.innerText.includes("Started")`, "the start");
        await toastGone();
      }
    });
    await step("all three are listed, and finish", () =>
      w.until(
        `return arguments[0].every(p => [...$$(".task-row")].some(r => r.innerText.includes(p.slice(0, 20)) && !r.innerText.includes("Working")))`,
        "three finished tasks",
        40000,
        prompts,
      ),
    );
    await step(
      "see which of them collide: all three change greet.py",
      () => w.until(`return [...$$(".task-row, .inspector")].some(e => /collide|overlap|same files/i.test(e.innerText))`, "a collision", 3000),
      { why: "nothing compares tasks with each other yet: only each one with its base", blocks: true },
    );
    await step("land them in order, each rebased on the last", async () => {
      throw new Error("needs the step before");
    });
  });

  // 4 ──────────────────────────────────────────────────────────────────────
  await journey("4. Your own branch", "Work", 1, async (step) => {
    await step("branch from main", async () => {
      await w.click(".new-work");
      await w.waitFor(".menu.floating");
      await w.clickText(".menu.floating button", "Branch");
      await answer("feat/shout");
      await w.until(`return $(".repo-branch")?.textContent.includes("feat/shout")`, "the new branch");
    });
    await step("stage part of a file in Changes, and commit", async () => {
      writeFileSync(
        `${repo}/greet.py`,
        readFileSync(`${repo}/greet.py`, "utf8") + '\n\ndef shout(name):\n    return greet(name).upper()\n\n\n# TODO: whisper()\n',
      );
      await section("Changes");
      await w.waitFor(".file-row");
      await w.clickText(".file-row-main", "greet.py");
      await w.until(`return $$(".diff tr.add").length >= 4`, "greet.py's diff");
      // shout() only: its two lines, not the TODO.
      await w.js(`$$(".diff tr.add")[2].dispatchEvent(new MouseEvent("click", { bubbles: true }))`);
      await w.js(`$$(".diff tr.add")[3].dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }))`);
      await w.clickText(".selection-bar button", "Stage lines");
      await w.until(`return $$(".file-row").length === 2`, "greet.py staged and unstaged");
      await w.fill(".composer-dock textarea", "Add shout()");
      await w.key(".composer-dock textarea", "Enter", { ctrlKey: true });
      await w.until(`return $$(".file-row").length === 1`, "the commit");
      must(git("log", "-1", "--format=%s") === "Add shout()", "committed");
      must(git("show", "HEAD:greet.py").includes("def shout") && !git("show", "HEAD:greet.py").includes("whisper"), "just the picked lines");
    });
    await step("Changes shows the branch as a work item", async () => {
      await w.until(`return $(".work-header .stage-track [aria-current]")?.innerText === "In progress"`, "the work header");
    });
    await step("publish it", async () => {
      await w.until(`return [...$$(".inspector button")].some(b => b.textContent.includes("Publish"))`, "Publish");
      await w.clickText(".inspector button", "Publish");
      await w.until(`return $(".inspector")?.innerText.includes("Tracks origin/feat/shout")`, "the push", 30000);
      must(bare("rev-parse", "feat/shout") === git("rev-parse", "feat/shout"), "pushed to origin");
    });
    await step(
      "open its pull request, from spor",
      async () => must(ghState().created.some((p) => p.headRefName === "feat/shout"), "a pull request through gh"),
      { why: "your own branch's Publish only pushes; Open a pull request goes to GitHub's page", blocks: true },
    );
    await step("fix a failed check", async () => {});
    await step("land it", async () => {});
  });
} finally {
  await w.close();
}

// ── What it came to ─────────────────────────────────────────────────────────
const count = (kind) => results.filter((r) => r.kind === kind);
console.log(`\n${count("gap").length} known gaps, ${count("fixed").length} fixed, ${count("fail").length} failures`);
for (const r of count("fixed")) console.log(`  fixed: ${r.name}: ${r.what}`);
for (const r of count("fail")) console.log(`  failed: ${r.name}: ${r.what}`);
if (count("fail").length || count("fixed").length) process.exit(1);
console.log("All good");
