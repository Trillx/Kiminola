import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
// @ts-expect-error Node imports TypeScript directly.
import { createDraftAutosave } from "../src/lib/draft-autosave.ts";
// @ts-expect-error Node imports TypeScript directly.
import * as transcript from "../src/lib/transcript-state.ts";
// @ts-expect-error Node imports TypeScript directly.
import * as recording from "../src/lib/recording-ui-state.ts";

const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function recordingPage(overrides: Record<string, unknown> = {}) {
  let guard: (navigation: { cancel: () => void; to: { url: URL } }) => void = () => {};
  const navigations: string[] = [];
  const writes: Array<{ notes: string; transcript: unknown[] }> = [];
  const meetings: Array<{ notepad: string; segments: unknown[] }> = [];
  const deleted: number[] = [];
  const page = { url: new URL("http://localhost/record") };
  const adapters = {
    ...transcript, ...recording,
    $state: (value: unknown) => value,
    $derived: (value: unknown) => value,
    onMount: () => {},
    beforeNavigate: (callback: typeof guard) => { guard = callback; },
    page,
    libraryDestinationState: { last: null },
    recordingLocationFromSearchParams: () => ({ space_id: 1 }),
    createDraftAutosave,
    createNoteDraft: async () => 100,
    getNoteDraft: async () => ({ id: 100, raw_markdown: "recovered notes", recovery_duration_seconds: 10, recovery_transcript: [] }),
    updateNoteDraftRecovery: async (_id: number, notes: string, _duration: number, lines: unknown[]) => { writes.push({ notes, transcript: lines }); },
    deleteNoteDraft: async (id: number) => { deleted.push(id); },
    startRecording: async () => ({ meeting_audio_available: true, transcription_available: true }),
    stopRecording: async () => ({ transcript: [] }),
    saveMeeting: async (meeting: { notepad: string; segments: unknown[] }) => { meetings.push(meeting); return 7; },
    goto: async (destination: string) => {
      let cancelled = false;
      guard({ cancel: () => { cancelled = true; }, to: { url: new URL(destination, page.url) } });
      if (!cancelled) navigations.push(destination);
    },
    console: { error: () => {} },
    ...overrides,
  };
  const component = readFileSync(new URL("../src/routes/record/+page.svelte", import.meta.url), "utf8");
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText.replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "").replace(/^export \{\};?\s*$/gm, "");
  const api = new Function(...Object.keys(adapters), `${source}\nreturn {
    start(draft) { requestedDraftId = draft ?? NaN; return prepareAndStart(); },
    edit(text) { notepad = text; checkpointRecovery(); },
    keepRecoveryAndLeave, openRecoveryDraft, cancel, finishMeeting, retryFinish,
    discard() { return discardAndLeave("/"); },
    state() { return { phase, notepad, finishError, noteSaveStatus, nativeSessionActive }; },
    dispose() { closeNoteAutosave(); }
  };`)(...Object.values(adapters));
  return { ...api, navigations, writes, meetings, deleted,
    navigate: (destination = "/") => adapters.goto(destination),
  };
}

test("normal meeting save remains a durable fallback when checkpoints fail", async (t) => {
  const controller = recordingPage({ updateNoteDraftRecovery: async () => { throw new Error("disk full"); } });
  t.after(controller.dispose);
  await controller.start();
  controller.edit("save these notes");
  await controller.finishMeeting("save");
  assert.equal(controller.meetings[0].notepad, "save these notes");
  assert.deepEqual(controller.navigations, ["/meeting/7"]);
});

for (const exit of ["openRecoveryDraft", "cancel"]) {
  test(`${exit} from a recovered draft retains failed writes and final transcript`, async (t) => {
    let fail = true;
    const stored: unknown[] = [];
    const final = { utterance_id: 1, revision: 2, channel: "you", text: "final words", is_partial: false, start_ms: 0, end_ms: 500 };
    const controller = recordingPage({
      stopRecording: async () => ({ transcript: [final] }),
      updateNoteDraftRecovery: async (_id: number, _notes: string, _duration: number, lines: unknown[]) => {
        if (fail) throw new Error("disk full");
        stored.push(lines);
      },
    });
    t.after(controller.dispose);
    await controller.start(100);
    controller.edit("changed recovered notes");
    await controller[exit]();
    assert.deepEqual(controller.navigations, []);
    assert.equal(controller.state().phase, "finish_failed");
    fail = false;
    await controller[exit]();
    assert.deepEqual(controller.navigations, [exit === "cancel" ? "/" : "/note/100"]);
    assert.deepEqual(stored, [[{ channel: "you", text: "final words", start_ms: 10_000, end_ms: 10_500 }]]);
    assert.deepEqual(controller.deleted, []);
  });
}

for (const waitingOn of ["draft", "capture"]) {
  test(`startup recovery exit waits for ${waitingOn} and stays after a failed checkpoint`, async (t) => {
    const ready = deferred<any>();
    const controller = recordingPage({
      ...(waitingOn === "draft" ? { createNoteDraft: () => ready.promise } : { startRecording: () => ready.promise }),
      updateNoteDraftRecovery: async () => { throw new Error("disk full"); },
    });
    t.after(controller.dispose);
    const starting = controller.start();
    await tick();
    await controller.navigate("/settings");
    await controller.keepRecoveryAndLeave();
    ready.resolve(waitingOn === "draft" ? 100 : { meeting_audio_available: true, transcription_available: true });
    await starting;
    assert.deepEqual(controller.navigations, []);
    assert.equal(controller.state().phase, "finish_failed");
    assert.match(controller.state().finishError, /disk full/);
  });
}

test("navigation cannot bypass a recovery write that is still pending", async (t) => {
  const stored = deferred<void>();
  const controller = recordingPage({ updateNoteDraftRecovery: () => stored.promise });
  t.after(controller.dispose);
  await controller.start();
  await controller.navigate("/settings");
  const leaving = controller.keepRecoveryAndLeave();
  await tick();
  await controller.navigate("/");
  assert.deepEqual(controller.navigations, []);
  stored.resolve();
  await leaving;
  assert.deepEqual(controller.navigations, ["/settings"]);
});

test("Back to meetings after startup failure keeps unsaved notes on write failure", async (t) => {
  const controller = recordingPage({
    startRecording: async () => { throw new Error("microphone unavailable"); },
    updateNoteDraftRecovery: async () => { throw new Error("disk full"); },
  });
  t.after(controller.dispose);
  await controller.start();
  controller.edit("notes without capture");
  await controller.navigate("/settings");
  assert.deepEqual(controller.navigations, []);
  await controller.cancel();
  assert.deepEqual(controller.navigations, []);
  assert.equal(controller.state().notepad, "notes without capture");
});

for (const draft of [undefined, 100]) {
  test(`explicit discard after failed recovery leaves without saving ${draft ? "an existing" : "an automatic"} draft`, async (t) => {
    const controller = recordingPage({ updateNoteDraftRecovery: async () => { throw new Error("disk full"); } });
    t.after(controller.dispose);
    await controller.start(draft);
    controller.edit("discard me");
    await controller.navigate();
    await controller.keepRecoveryAndLeave();
    await controller.discard();
    assert.deepEqual(controller.navigations, ["/"]);
    assert.deepEqual(controller.deleted, draft ? [] : [100]);
    assert.equal(controller.meetings.length, 0);
  });
}

test("Keep recovery copy after stop failure must not discard the automatic draft", async (t) => {
  let stopFails = true;
  const controller = recordingPage({ stopRecording: async () => {
    if (stopFails) throw new Error("capture could not stop");
    return { transcript: [] };
  } });
  t.after(controller.dispose);
  await controller.start();
  controller.edit("keep me after failed stop");
  await controller.finishMeeting("save");
  assert.equal(controller.state().phase, "finish_failed");
  assert.equal(controller.state().nativeSessionActive, true);
  stopFails = false;
  await controller.cancel();
  assert.deepEqual(controller.deleted, []);
  assert.equal(controller.writes[0].notes, "keep me after failed stop");
  assert.deepEqual(controller.navigations, ["/"]);
});

test("recovery creation failure blocks exit and retry can create the missing draft", async (t) => {
  let fails = true;
  const controller = recordingPage({ createNoteDraft: async () => {
    if (fails) throw new Error("database unavailable");
    return 100;
  } });
  t.after(controller.dispose);
  await controller.start();
  controller.edit("notes without an initial draft");
  await controller.navigate();
  await controller.keepRecoveryAndLeave();
  assert.deepEqual(controller.navigations, []);
  assert.equal(controller.state().phase, "finish_failed");
  fails = false;
  await controller.navigate();
  await controller.keepRecoveryAndLeave();
  assert.equal(controller.writes[0].notes, "notes without an initial draft");
  assert.deepEqual(controller.navigations, ["/"]);
});

test("failed recovery-only exit keeps the editor and allows retry with latest notes", async (t) => {
  let fail = true;
  const stored: string[] = [];
  const controller = recordingPage({ updateNoteDraftRecovery: async (_id: number, notes: string) => {
    if (fail) throw new Error("disk full");
    stored.push(notes);
  } });
  t.after(controller.dispose);
  await controller.start();
  controller.edit("unsaved notes");
  await controller.navigate("/settings");
  await controller.keepRecoveryAndLeave();
  assert.deepEqual(controller.navigations, []);
  assert.equal(controller.state().notepad, "unsaved notes");
  assert.equal(controller.state().phase, "finish_failed");
  assert.match(controller.state().finishError, /disk full/);
  assert.equal(controller.meetings.length, 0);
  fail = false;
  controller.edit("latest edit after failure");
  await controller.navigate("/settings");
  await controller.keepRecoveryAndLeave();
  assert.deepEqual(controller.navigations, ["/settings"]);
  assert.deepEqual(stored, ["latest edit after failure"]);
});

test("choosing Save as meeting after recovery failure keeps that intent on retry", async (t) => {
  let fails = true;
  const controller = recordingPage({
    updateNoteDraftRecovery: async () => { throw new Error("checkpoint unavailable"); },
    saveMeeting: async () => { if (fails) throw new Error("meeting write failed"); return 7; },
  });
  t.after(controller.dispose);
  await controller.start();
  await controller.navigate();
  await controller.keepRecoveryAndLeave();
  await controller.finishMeeting("save");
  fails = false;
  await controller.retryFinish();
  await tick();
  assert.deepEqual(controller.navigations, ["/meeting/7"]);
});

test("Retry saving resumes the original recovery-only destination without creating a meeting", async (t) => {
  let fails = true;
  const saved: string[] = [];
  const controller = recordingPage({ updateNoteDraftRecovery: async (_id: number, notes: string) => {
    if (fails) throw new Error("disk full");
    saved.push(notes);
  } });
  t.after(controller.dispose);
  await controller.start();
  controller.edit("keep these notes");
  await controller.navigate("/");
  await controller.keepRecoveryAndLeave();
  fails = false;
  controller.edit("latest after recovery failure");
  await controller.retryFinish();
  await tick();
  assert.deepEqual(saved, ["latest after recovery failure"]);
  assert.deepEqual(controller.navigations, ["/"]);
  assert.equal(controller.meetings.length, 0);
});
