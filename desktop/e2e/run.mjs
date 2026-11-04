// End-to-end check of the real app (Tauri + spor's core), through WebDriver.
//
//   tauri-driver &            # needs WebKitWebDriver (Linux: webkit2gtk-driver)
//   SPOR_REPO=/path/to/a/scratch/repo node e2e/run.mjs path/to/spor-desktop out-dir
//
// It drives one task through the whole loop (create, agent works, review
// the diff, comment on a line, send feedback, land, undo) and checks the
// repository with git at each step. Use a throwaway repository: it lands.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";

const [app, out = "e2e-shots"] = process.argv.slice(2);
const repo = process.env.SPOR_REPO;
if (!app || !repo) {
  console.error("usage: SPOR_REPO=<repo> node e2e/run.mjs <app binary> [out dir]");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const DRIVER = process.env.WEBDRIVER_URL ?? "http://127.0.0.1:4444";
const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";

async function wd(method, path, body) {
  const res = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = await res.json();
  if (json.value?.error) throw new Error(`${method} ${path}: ${json.value.error}: ${json.value.message}`);
  return json.value;
}

const git = (...args) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8" }).trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function check(ok, what) {
  if (!ok) throw new Error(`FAILED: ${what}`);
  console.log(`  ok  ${what}`);
}

const session = await wd("POST", "/session", {
  capabilities: { alwaysMatch: { "tauri:options": { application: app } } },
});
const S = `/session/${session.sessionId}`;

const find = async (css) => (await wd("POST", `${S}/element`, { using: "css selector", value: css }))[ELEMENT];
async function waitFor(css, ms = 20000) {
  const until = Date.now() + ms;
  for (;;) {
    try {
      return await find(css);
    } catch (e) {
      if (Date.now() > until) throw new Error(`Timed out waiting for ${css}`);
      await sleep(250);
    }
  }
}
const js = (script, ...args) => wd("POST", `${S}/execute/sync`, { script, args });
const click = async (css) => wd("POST", `${S}/element/${await waitFor(css)}/click`, {});
const type = async (css, text) => wd("POST", `${S}/element/${await waitFor(css)}/value`, { text });
// Set a React-controlled textarea's value and fire its input event.
const fill = (css, text) =>
  js(
    `const el = document.querySelector(arguments[0]);
     const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set;
     set.call(el, arguments[1]); el.dispatchEvent(new Event("input", { bubbles: true })); el.focus();`,
    css,
    text,
  );
const enter = (css) =>
  js(
    `document.querySelector(arguments[0]).dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));`,
    css,
  );
async function shot(name) {
  writeFileSync(`${out}/${name}.png`, Buffer.from(await wd("GET", `${S}/screenshot`), "base64"));
}

try {
  await js("localStorage.clear()");
  await wd("POST", `${S}/refresh`, {});
  console.log("New task");
  await waitFor(".new-task textarea");
  const before = git("rev-parse", "main");
  await fill(".new-task textarea", "Let greet() take an optional title");
  await enter(".new-task textarea");
  await waitFor(".task-page .live-line");
  check(git("branch", "--list", "agent/*").includes("agent/let-greet-take-optional"), "task branch created");
  await sleep(4500);
  await shot("1-working");

  console.log("Agent finishes");
  await waitFor(".run .run-end", 30000);
  await shot("2-finished");
  const transcript = await js(`return document.querySelector(".conversation").innerText`);
  // The stand-in agent's first run has a refused tool call, so it stays
  // open (a run with a failure never folds) and says what failed.
  check(!transcript.includes("Worked for"), "a run with a refused tool call doesn't fold");
  check(transcript.includes("1 failed"), "the refusal is visible in the tool summary");
  check(transcript.includes("now takes an optional"), "agent's final message is shown");

  console.log("Review");
  await js(`[...document.querySelectorAll(".tabs button")].find(b => b.textContent.includes("Changes")).click()`);
  await waitFor(".diff tr.add");
  await js(`document.querySelectorAll(".diff tr.add .add-comment button")[1].click()`);
  await fill(".comment-editor textarea", "Also add a farewell() like this one");
  await enter(".comment-editor textarea");
  await waitFor(".composer .chip");
  check((await js(`return document.querySelectorAll(".line-comment").length`)) === 1, "the comment shows once, on its line");
  await shot("3-review");

  console.log("Feedback resumes the session");
  await enter(".composer-dock textarea");
  await waitFor(".task-page .live-line");
  await waitFor(".run:nth-of-type(2) .fold", 30000);
  const second = await js(`return document.querySelector(".conversation").innerText`);
  check(second.includes("Comments on the changes"), "line comment was sent with the feedback");
  check(second.includes("Worked for"), "a clean finished run folds behind 'Worked for'");
  check(second.includes("Added `farewell()`") || second.includes("Added farewell()"), "second run resumed and finished");
  await shot("4-second-run");

  console.log("Land");
  await click(".inspector .split .primary");
  await waitFor(".toast");
  await sleep(500);
  check(git("log", "-1", "--format=%s", "main") === "Let greet() take an optional title", "landed as one commit on main");
  check(git("log", "-1", "--format=%B", "main").includes("Spor-Task:"), "landing commit has the Spor-Task trailer");
  check(!git("branch", "--list", "agent/*"), "task branch removed after landing");
  await shot("5-landed");

  console.log("Undo");
  await js(`document.querySelector(".toast .link").click()`);
  await sleep(1500);
  check(git("rev-parse", "main") === before, "undo put main back");
  check(git("branch", "--list", "agent/*").includes("agent/let-greet-take-optional"), "undo restored the task branch");
  await shot("6-undone");
  console.log("All good");
} finally {
  await wd("DELETE", S).catch(() => {});
}
