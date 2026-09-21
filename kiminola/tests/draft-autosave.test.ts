import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { createDraftAutosave } from "../src/lib/draft-autosave.ts";

test("continuous edits checkpoint the latest revision within the maximum wait", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const saved: string[] = [];
  const autosave = createDraftAutosave(async (value) => { saved.push(value); }, () => {}, 500);
  t.after(() => autosave.cancel());

  for (let revision = 0; revision < 200; revision++) {
    autosave.schedule(`revision ${revision}`);
    t.mock.timers.tick(100);
    // Drain the serialized promise queue between clock ticks.
    for (let microtask = 0; microtask < 8; microtask++) await Promise.resolve();
    if (revision === 49) assert.deepEqual(saved, ["revision 49"]);
  }
  assert.deepEqual(saved, ["revision 49", "revision 99", "revision 149", "revision 199"]);
});

const wait = (milliseconds: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

test("lazy checkpoints construct only the latest snapshot when a write runs", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let constructed = 0;
  let current = "initial";
  let fail = false;
  const saved: string[] = [];
  const autosave = createDraftAutosave<string>(async (value) => {
    if (fail) throw new Error("disk full");
    saved.push(value);
  }, () => {});
  t.after(() => autosave.cancel());
  const snapshot = () => { constructed++; return current; };

  for (let revision = 0; revision < 50; revision++) {
    current = `revision ${revision}`;
    autosave.schedule(snapshot);
  }
  assert.equal(constructed, 0);
  t.mock.timers.tick(500);
  for (let microtask = 0; microtask < 8; microtask++) await Promise.resolve();
  assert.equal(constructed, 1);
  assert.deepEqual(saved, ["revision 49"]);

  current = "retry latest";
  fail = true;
  await assert.rejects(autosave.flush(snapshot), /disk full/);
  fail = false;
  current = "edited after failure";
  await autosave.flushPending();
  assert.deepEqual(saved, ["revision 49", "edited after failure"]);
});

test("coalesces rapid edits into the latest draft", async () => {
  const saved: string[] = [];
  const statuses: string[] = [];
  const autosave = createDraftAutosave(
    async (value) => {
      saved.push(value);
    },
    (status) => statuses.push(status),
    10,
  );

  autosave.schedule("first");
  autosave.schedule("latest");
  await wait(30);

  assert.deepEqual(saved, ["latest"]);
  assert.deepEqual(statuses, ["saving", "saved"]);
});

test("serializes writes so a slower old save cannot win", async () => {
  const saved: string[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstBlocked = new Promise<void>((resolve) => {
    releaseFirst = resolve;
  });
  const autosave = createDraftAutosave(
    async (value) => {
      saved.push(value);
      if (value === "first") await firstBlocked;
    },
    () => undefined,
    0,
  );

  autosave.schedule("first");
  await wait(5);
  autosave.schedule("second");
  await wait(5);
  assert.deepEqual(saved, ["first"]);

  releaseFirst?.();
  await wait(10);
  assert.deepEqual(saved, ["first", "second"]);
});

test("flush saves immediately and cancel drops a pending timer", async () => {
  const saved: string[] = [];
  const autosave = createDraftAutosave(
    async (value) => {
      saved.push(value);
    },
    () => undefined,
    20,
  );

  autosave.schedule("pending");
  await autosave.flush("final");
  autosave.schedule("discarded");
  autosave.cancel();
  await wait(30);

  assert.deepEqual(saved, ["final"]);
});

test("update flush retries failed notes and serializes with an older write", async () => {
  const saved: string[] = [];
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  let fail = false;
  const autosave = createDraftAutosave(async (value: string) => {
    if (value === "old") await blocked;
    if (fail) throw new Error("storage unavailable");
    saved.push(value);
  }, () => {}, 60_000);
  const old = autosave.flush("old");
  autosave.schedule("latest");
  const update = autosave.flushPending();
  release();
  await Promise.all([old, update]);
  assert.deepEqual(saved, ["old", "latest"]);
  fail = true;
  autosave.schedule("retry me");
  await assert.rejects(autosave.flushPending(), /storage unavailable/);
  fail = false;
  await autosave.flushPending();
  assert.deepEqual(saved, ["old", "latest", "retry me"]);
  await autosave.flushPending();
  assert.equal(saved.length, 3);
});
