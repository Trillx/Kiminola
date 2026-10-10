import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node strip-types imports TypeScript directly.
import { createDictationController } from "../src/lib/dictation-controller.ts";
// @ts-expect-error Node strip-types imports TypeScript directly.
import { sameDictationSettings, type DictationSettings } from "../src/lib/dictation-types.ts";

const settings: DictationSettings = { enabled: false, activation: "hold", shortcut: "Ctrl+Shift+Space", side: "right", cleanup: "raw", clipboard_consent: false, history_enabled: false, microphone_id: null };
const snapshot = (patch = {}) => ({ settings, provider_authorized: false, phase: "disabled", session_id: null, text: "", raw_text: "", level: 0, elapsed_seconds: 0, error: null, exit_intent: null, delivery: "none", ...patch });
const deferred = () => { let resolve!: (value?: any) => void; let reject!: (error: Error) => void; const promise = new Promise<any>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const tick = () => new Promise(resolve => setTimeout(resolve, 0));

test("settings equality is semantic rather than dependent on IPC key order", () => {
  const { enabled, ...rest } = settings;
  const reordered = { ...rest, enabled };
  assert.equal(sameDictationSettings(settings, reordered), true);
  assert.equal(sameDictationSettings(settings, { ...settings, history_enabled: true }), false);
  assert.equal(sameDictationSettings(null, settings), false);
});

function fixture() {
  let authoritative = snapshot();
  let callback: (value: any) => void = () => {};
  let reads = 0;
  let stopped = false;
  const subscribe = deferred();
  const pending: ReturnType<typeof deferred>[] = [];
  const changes: any[] = [];
  const controller = createDictationController({
    async listen(fn: (value: any) => void) { callback = fn; await subscribe.promise; return () => { stopped = true; }; },
    async get() { reads++; const captured = structuredClone(authoritative); const gate = pending.shift(); if (gate) return gate.promise; return captured; },
  }, (value: any) => changes.push(value));
  return { controller, changes, subscribe, pending, get reads() { return reads; }, get stopped() { return stopped; }, state(value: any) { authoritative = value; }, emit(value: any) { callback(value); } };
}

test("dictation subscribes before its initial read and never starts capture", async () => {
  const f = fixture();
  const ready = f.controller.connect();
  await tick();
  assert.equal(f.reads, 0);
  f.subscribe.resolve();
  await ready;
  assert.equal(f.reads, 1);
  assert.equal(f.changes.at(-1).snapshot.phase, "disabled");
  f.controller.dispose();
  assert.equal(f.stopped, true);
});

test("late initial reads and stale event payloads cannot erase recovery", async () => {
  const f = fixture();
  const initial = deferred();
  f.pending.push(initial);
  const ready = f.controller.connect();
  f.subscribe.resolve();
  await tick();
  const review = snapshot({ phase: "review", session_id: 7, text: "Retain this text." });
  f.state(review);
  f.emit(review);
  await tick();
  assert.equal(f.changes.at(-1).snapshot.text, "Retain this text.");
  initial.resolve(snapshot());
  await ready;
  assert.equal(f.changes.at(-1).snapshot.text, "Retain this text.");
  f.emit(snapshot({ phase: "listening", session_id: 6 }));
  await tick();
  assert.equal(f.changes.at(-1).snapshot.text, "Retain this text.");
  f.controller.dispose();
});

test("failed refresh keeps recovery and a late old failure cannot annotate newer state", async () => {
  const f = fixture();
  f.state(snapshot({ phase: "review", session_id: 10, text: "Keep recovery." }));
  f.subscribe.resolve();
  await f.controller.connect();
  const failure = deferred();
  f.pending.push(failure);
  const refresh = f.controller.refresh();
  failure.reject(new Error("State unavailable"));
  await refresh;
  assert.equal(f.changes.at(-1).snapshot.text, "Keep recovery.");
  assert.match(f.changes.at(-1).error, /State unavailable/);
  const old = deferred();
  f.pending.push(old);
  const oldRefresh = f.controller.refresh();
  f.state(snapshot({ phase: "review", session_id: 11, text: "New recovery." }));
  f.emit(snapshot());
  await tick();
  old.reject(new Error("Obsolete failure"));
  await oldRefresh;
  assert.equal(f.changes.at(-1).snapshot.text, "New recovery.");
  assert.equal(f.changes.at(-1).error, "");
  f.controller.dispose();
});

test("teardown before subscription completion immediately removes the late listener", async () => {
  const f = fixture();
  const ready = f.controller.connect();
  f.controller.dispose();
  f.subscribe.resolve();
  await ready;
  assert.equal(f.stopped, true);
  assert.equal(f.reads, 0);
  assert.deepEqual(f.changes, []);
});
