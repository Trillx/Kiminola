import assert from "node:assert/strict";
import { test } from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
// @ts-expect-error Node imports TypeScript directly.
import { installWhenSaved } from "../src/lib/update-safety.ts";
// @ts-expect-error Node imports TypeScript directly.
import { flushPendingWork } from "../src/lib/pending-work.ts";
// @ts-expect-error Node imports the production IPC wrappers directly.
import { downloadModelPack, enhanceMeeting, renameMeeting, testLlmConfig } from "../src/lib/tauri.ts";

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

test("an update waits for in-flight data writes, never for long-running streams", async () => {
  Object.defineProperty(globalThis, "window", { value: { crypto: globalThis.crypto }, configurable: true });
  let finishRename!: () => void;
  const rename = new Promise<void>((resolve) => { finishRename = resolve; });
  const never = new Promise<void>(() => {});
  mockIPC((command) => (command === "rename_meeting" ? rename : never));
  try {
    const renaming = renameMeeting(1, "Quarterly review");
    void downloadModelPack(() => {});
    void enhanceMeeting(1, undefined);
    void testLlmConfig();
    let flushed = false;
    const flushing = flushPendingWork().then(() => { flushed = true; });
    await new Promise<void>((resolve) => setImmediate(resolve));
    assert.equal(flushed, false, "the rename is user data and must land first");
    finishRename();
    await Promise.all([renaming, flushing]);
    assert.equal(flushed, true, "model downloads and AI streams lose no user data when interrupted");
  } finally {
    clearMocks();
    Reflect.deleteProperty(globalThis, "window");
  }
});
