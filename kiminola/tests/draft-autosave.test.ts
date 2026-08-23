import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { createDraftAutosave } from "../src/lib/draft-autosave.ts";

const wait = (milliseconds: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

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
