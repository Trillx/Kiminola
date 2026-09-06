import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node imports TypeScript directly.
import { installWhenSaved } from "../src/lib/update-safety.ts";

test("update waits for notes to become durable before preparing and installing", async () => {
  const events: string[] = [];
  let save!: () => void;
  const pending = new Promise<void>((resolve) => { save = resolve; });
  const update = installWhenSaved({
    flush: async () => { await pending; events.push("saved"); },
    prepare: async () => { events.push("prepared"); },
    install: async () => { events.push("installed"); },
    cancel: async () => { events.push("cancelled"); },
  });
  await Promise.resolve();
  assert.deepEqual(events, []);
  save();
  await update;
  assert.deepEqual(events, ["saved", "prepared", "installed"]);
});

test("save failure prevents shutdown", async () => {
  let installed = false;
  await assert.rejects(installWhenSaved({
    flush: async () => { throw new Error("disk full"); },
    prepare: async () => {},
    install: async () => { installed = true; },
    cancel: async () => {},
  }), /disk full/);
  assert.equal(installed, false);
});

test("failed installation releases the native update barrier", async () => {
  let cancelled = false;
  await assert.rejects(installWhenSaved({
    flush: async () => {},
    prepare: async () => {},
    install: async () => { throw new Error("installer failed"); },
    cancel: async () => { cancelled = true; },
  }), /installer failed/);
  assert.equal(cancelled, true);
});
