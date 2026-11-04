// A tiny WebDriver client for driving the real app through tauri-driver,
// plus the checks the e2e scripts share.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";

const DRIVER = process.env.WEBDRIVER_URL ?? "http://127.0.0.1:4444";
const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

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

export function check(ok, what) {
  if (!ok) throw new Error(`FAILED: ${what}`);
  console.log(`  ok  ${what}`);
}

export const gitIn = (repo) => (...args) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8" }).trim();

/** Start the app and return helpers bound to its session. */
export async function open(app, out) {
  mkdirSync(out, { recursive: true });
  const session = await wd("POST", "/session", {
    capabilities: { alwaysMatch: { "tauri:options": { application: app } } },
  });
  const S = `/session/${session.sessionId}`;
  const find = async (css) => (await wd("POST", `${S}/element`, { using: "css selector", value: css }))[ELEMENT];
  const waitFor = async (css, ms = 20000) => {
    const until = Date.now() + ms;
    for (;;) {
      try {
        return await find(css);
      } catch {
        if (Date.now() > until) throw new Error(`Timed out waiting for ${css}`);
        await sleep(250);
      }
    }
  };
  const js = (script, ...args) => wd("POST", `${S}/execute/sync`, { script, args });
  const self = {
    find,
    waitFor,
    js,
    /** Wait until `script` returns something truthy. */
    async until(script, what, ms = 20000, ...args) {
      const end = Date.now() + ms;
      for (;;) {
        const v = await js(script, ...args);
        if (v) return v;
        if (Date.now() > end) throw new Error(`Timed out waiting for ${what}`);
        await sleep(250);
      }
    },
    click: async (css) => wd("POST", `${S}/element/${await waitFor(css)}/click`, {}),
    /** Click the first element matching `css` whose text includes `text`. */
    clickText: (css, text) =>
      js(
        `const el = [...document.querySelectorAll(arguments[0])].find(e => e.textContent.includes(arguments[1]));
         if (!el) throw new Error("No " + arguments[0] + " with " + arguments[1]); el.click();`,
        css,
        text,
      ),
    text: (css) => js(`return document.querySelector(arguments[0])?.innerText ?? ""`, css),
    count: (css) => js(`return document.querySelectorAll(arguments[0]).length`, css),
    // Set a React-controlled field's value and fire its input event.
    fill: (css, text) =>
      js(
        `const el = document.querySelector(arguments[0]);
         const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
         Object.getOwnPropertyDescriptor(proto, "value").set.call(el, arguments[1]);
         el.dispatchEvent(new Event("input", { bubbles: true })); el.focus();`,
        css,
        text,
      ),
    key: (css, key, mods = {}) =>
      js(
        `document.querySelector(arguments[0]).dispatchEvent(new KeyboardEvent("keydown", Object.assign({ key: arguments[1], bubbles: true }, arguments[2])));`,
        css,
        key,
        mods,
      ),
    enter: (css) => self.key(css, "Enter"),
    async shot(name) {
      writeFileSync(`${out}/${name}.png`, Buffer.from(await wd("GET", `${S}/screenshot`), "base64"));
    },
    refresh: () => wd("POST", `${S}/refresh`, {}),
    close: () => wd("DELETE", S).catch(() => {}),
  };
  return self;
}
