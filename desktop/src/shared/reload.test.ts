import { test } from "node:test";
import assert from "node:assert/strict";
import { reloader } from "./reload.ts";

const tick = () => new Promise((r) => setTimeout(r, 0));

test("changes while a reload runs make one more reload, not one each", async () => {
  let calls = 0;
  let settled = 0;
  let release: () => void = () => {};
  const r = reloader(
    () => {
      calls++;
      return new Promise<void>((done) => (release = done));
    },
    () => settled++,
  );
  const first = r.run();
  await tick();
  r.run();
  r.run();
  r.run();
  assert.equal(calls, 1);
  release();
  await tick();
  assert.equal(calls, 2);
  release();
  await first;
  assert.equal(calls, 2);
  assert.equal(settled, 1);
});

test("a failing reload still settles, and stopping ends it quietly", async () => {
  let settled = 0;
  const r = reloader(() => Promise.reject(new Error("no repository")), () => settled++);
  await r.run();
  assert.equal(settled, 1);
  r.stop();
  await r.run();
  assert.equal(settled, 1);
});
