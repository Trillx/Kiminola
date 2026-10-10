import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { createCaptureSession, type CapturePort } from "../src/lib/capture-session.ts";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { createNotepadStore, draftNotepad, type NotepadPort } from "../src/lib/notepad-store.ts";
import type { LibraryLocation, NoteDraftDetail, RecordingStartStatus, TranscriptEvent } from "../src/lib/tauri";

const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

function deferred<T = void>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

const capturing: RecordingStartStatus = { meeting_audio_available: true, transcription_available: true };
const where = (location: LibraryLocation | null) => (location ? `${location.kind}:${location.id}` : "root");
const speech = (id: number, text: string, startMs = 0): TranscriptEvent => ({
  utterance_id: id, revision: 1, channel: "you", text, is_partial: false, start_ms: startMs, end_ms: startMs + 1_000,
});

/** A fake native side: every call is logged in order, every behaviour can be replaced per test. */
function harness({ last = null as LibraryLocation | null, draft = null as NoteDraftDetail | null } = {}) {
  const calls: string[] = [];
  let clock = 0;
  let ticker: (() => void) | null = null;
  let lastLocation = last;
  const emit = {} as {
    transcript(event: TranscriptEvent): void;
    pressure(mic: number, loopback: number): void;
    shortcut(): void;
    quitBlocked(): void;
  };
  const failDraftWrites = new Set<number>();
  const writes = { gate: Promise.resolve() };
  const notepadPort: NotepadPort = {
    updateNotes: async () => { throw new Error("unexpected meeting write"); },
    updateNoteDraft: async (id, text) => {
      calls.push(`update_note_draft ${id} "${text}"`);
      if (failDraftWrites.has(id)) throw new Error("disk is busy");
    },
    updateNoteDraftRecovery: async (id, text, seconds, transcript, location) => {
      calls.push(`checkpoint ${id} "${text}" ${seconds}s [${transcript.map((line) => line.text).join("|")}] ${where(location)}`);
      await writes.gate;
      if (failDraftWrites.has(id)) throw new Error("disk is busy");
    },
  };
  const notepad = createNotepadStore(notepadPort, { delayMs: 60_000 });
  const native = {
    startRecording: async (): Promise<RecordingStartStatus> => capturing,
    stopRecording: async () => ({ transcript: [] as TranscriptEvent[], finalization_warning: null as string | null }),
    pauseRecording: async () => {},
    resumeRecording: async (): Promise<RecordingStartStatus> => capturing,
    saveMeeting: async (): Promise<number> => 9,
    createNoteDraft: async (): Promise<number> => 5,
    deleteNoteDraft: async () => {},
    navigate: async (_href: string) => {},
  };
  const port: CapturePort = {
    startRecording: () => { calls.push("start_recording"); return native.startRecording(); },
    stopRecording: () => { calls.push("stop_recording"); return native.stopRecording(); },
    pauseRecording: () => { calls.push("pause_recording"); return native.pauseRecording(); },
    resumeRecording: () => { calls.push("resume_recording"); return native.resumeRecording(); },
    saveMeeting: (input) => {
      calls.push(`save_meeting draft=${input.noteDraftId} ${where(input.location)} "${input.notepad}" ${input.durationSeconds}s [${input.segments.map((line) => line.text).join("|")}]`);
      return native.saveMeeting();
    },
    createNoteDraft: (location) => { calls.push(`create_note_draft ${where(location)}`); return native.createNoteDraft(); },
    getNoteDraft: async (id) => { calls.push(`get_note_draft ${id}`); return draft!; },
    deleteNoteDraft: (id) => { calls.push(`delete_note_draft ${id}`); return native.deleteNoteDraft(); },
    onTranscriptEvent: async (handler) => { emit.transcript = handler; return () => {}; },
    onAudioPressure: async (handler) => {
      emit.pressure = (mic, loopback) => handler({ mic_dropped_samples: mic, loopback_dropped_samples: loopback });
      return () => {};
    },
    onShortcutTriggered: async (handler) => { emit.shortcut = handler; return () => {}; },
    onRecordingQuitBlocked: async (handler) => { emit.quitBlocked = handler; return () => {}; },
    navigate: (href) => { calls.push(`navigate ${href}`); return native.navigate(href); },
    now: () => clock,
    every: (_milliseconds, tick) => { ticker = tick; return () => { ticker = null; }; },
  };
  const session = createCaptureSession({ port, notepad, lastLocation: () => lastLocation, log: () => {} });
  return {
    session,
    state: session.state,
    calls,
    native,
    notepad,
    emit,
    failDraftWrites,
    writes,
    open: (search = "") => session.open(new URLSearchParams(search)),
    /** Let time pass; the 1 s ticker may run late, so it fires once. */
    advance(milliseconds: number) { clock += milliseconds; ticker?.(); },
    moveLastLocation(location: LibraryLocation | null) { lastLocation = location; },
    async idle() { await settle(); await session.settled(); },
  };
}

test("a session records into the destination resolved at its start and checkpoints before stopping", async () => {
  const h = harness({ last: { kind: "meeting", id: 2 } });
  await h.open("?spaceId=4");
  assert.equal(h.state.phase, "recording");
  assert.equal(h.session.active, true);
  assert.equal(h.state.recoveryStatus, "Recovery copy ready");

  // Sidebar navigation can change the last-used destination mid-recording.
  h.moveLastLocation({ kind: "space", id: 99 });
  h.state.notes = "agenda";
  h.session.notesChanged();
  await h.session.finish("save");

  assert.deepEqual(h.calls, [
    "create_note_draft space:4",
    "start_recording",
    'checkpoint 5 "agenda" 0s [] space:4',
    "stop_recording",
    'checkpoint 5 "agenda" 0s [] space:4',
    'save_meeting draft=5 space:4 "agenda" 0s []',
    "navigate /meeting/9",
  ]);
  assert.equal(h.session.active, false);
});

test("an exit waits for a pause in flight, then stops before saving the final transcript", async () => {
  const h = harness();
  await h.open();
  const pause = deferred();
  h.native.pauseRecording = () => pause.promise;
  h.native.stopRecording = async () => ({ transcript: [speech(1, "final words")], finalization_warning: "decoder timed out" });
  h.advance(3_000);

  const pausing = h.session.pause();
  const finishing = h.session.finish("generate");
  await settle();
  assert.equal(h.state.controlBusy, true);
  assert.equal(h.calls.includes("stop_recording"), false, "capture must not stop under a pause request");

  pause.resolve();
  await Promise.all([pausing, finishing]);
  assert.deepEqual(h.calls.slice(2), [
    "pause_recording",
    'checkpoint 5 "" 3s [] root',
    'checkpoint 5 "" 3s [] root',
    "stop_recording",
    'checkpoint 5 "" 3s [final words] root',
    'save_meeting draft=5 root "" 3s [final words]',
    "navigate /meeting/9?mode=generate&warning=transcript-finalization",
  ]);
});

test("a failed save keeps the session and retries with the same Note draft without stopping twice", async () => {
  const h = harness();
  await h.open();
  // The recovery copy cannot be written either; the Meeting save carries the notes itself.
  h.failDraftWrites.add(5);
  h.native.saveMeeting = async () => { throw new Error("database is locked"); };
  await h.session.finish("enhance");

  assert.equal(h.state.phase, "finish_failed");
  assert.equal(h.state.finishError, "database is locked");
  assert.equal(h.session.guardsNavigation, true);
  assert.equal(h.session.active, true, "an unsaved meeting still blocks updates");

  h.native.saveMeeting = async () => 9;
  h.emit.shortcut();
  await h.idle();

  assert.equal(h.calls.filter((call) => call === "stop_recording").length, 1);
  assert.deepEqual(h.calls.filter((call) => call.startsWith("save_meeting")), [
    'save_meeting draft=5 root "" 0s []',
    'save_meeting draft=5 root "" 0s []',
  ]);
  assert.equal(h.calls.at(-1), "navigate /meeting/9?mode=enhance");
  // The draft is attached now: its failed checkpoint must not be retried (and block updates) forever.
  assert.equal(h.notepad.pending(draftNotepad(5)), undefined);
  await h.notepad.flush();
});

test("leaving during startup waits for startup to finish, then keeps the recovery copy", async () => {
  const h = harness();
  const started = deferred<RecordingStartStatus>();
  h.native.startRecording = () => started.promise;
  const opening = h.open();
  await settle();
  assert.equal(h.state.phase, "starting");
  assert.equal(h.session.guardsNavigation, true);
  assert.equal(h.session.canFinish, false, "finishing is not offered before capture starts");

  await h.session.leave("/settings");
  assert.equal(h.state.exitPending, true);
  assert.equal(h.state.recoveryStatus, "Preparing recovery copy before leaving...");
  assert.equal(h.calls.includes("stop_recording"), false);

  started.resolve(capturing);
  await opening;
  assert.deepEqual(h.calls, [
    "create_note_draft root",
    "start_recording",
    'checkpoint 5 "" 0s [] root',
    "stop_recording",
    'checkpoint 5 "" 0s [] root',
    "navigate /settings",
  ]);
  assert.equal(h.state.phase, "idle");
  assert.equal(h.state.exitPending, false);
});

test("a startup failure is surfaced and Try again reuses the prepared Note draft", async () => {
  const h = harness();
  h.native.startRecording = async () => { throw new Error("Microphone access is blocked"); };
  await h.open();
  assert.equal(h.state.phase, "failed");
  assert.equal(h.state.startError, "Microphone access is blocked");
  assert.equal(h.session.canStop, false);
  assert.equal(h.session.guardsNavigation, true, "its notepad may hold edits not checkpointed yet");
  assert.equal(h.session.active, true, "the failed view still owns a recovery copy");

  h.native.startRecording = async () => capturing;
  await h.session.retryStart();
  assert.equal(h.state.phase, "recording");
  assert.deepEqual(h.calls, ["create_note_draft root", "start_recording", "start_recording"]);
  await h.session.cancel();
});

test("the stop shortcut saves only from a state where stopping is safe", async () => {
  const h = harness();
  const started = deferred<RecordingStartStatus>();
  h.native.startRecording = () => started.promise;
  const opening = h.open();
  await settle();
  h.emit.shortcut();
  started.resolve(capturing);
  await opening;
  await h.idle();
  assert.equal(h.state.phase, "recording", "ignored while starting");

  const pause = deferred();
  h.native.pauseRecording = () => pause.promise;
  const pausing = h.session.pause();
  h.emit.shortcut();
  pause.resolve();
  await pausing;
  await h.idle();
  assert.equal(h.state.phase, "paused", "ignored while a pause is in flight");

  h.emit.shortcut();
  await h.idle();
  assert.equal(h.calls.at(-1), "navigate /meeting/9");
});

test("cancel deletes an auto-created recovery copy without checkpointing it", async () => {
  const h = harness();
  await h.open();
  h.emit.transcript(speech(1, "said before cancelling"));
  h.state.notes = "throwaway";
  h.session.notesChanged();
  await h.session.cancel();

  assert.deepEqual(h.calls, [
    "create_note_draft root",
    "start_recording",
    "stop_recording",
    "delete_note_draft 5",
    "navigate /",
  ]);
  await h.notepad.flush();
  assert.equal(h.calls.length, 5, "no write may reach the deleted draft");
});

test("resuming a Note draft prefers unsaved text, offsets new speech, and keeps the draft on cancel", async () => {
  const h = harness({
    last: { kind: "space", id: 99 },
    draft: {
      id: 5,
      title: "Draft",
      created_at: "2026-10-01T09:00:00Z",
      updated_at: "2026-10-01T09:00:00Z",
      raw_markdown: "database notes",
      meeting_id: null,
      recovery_duration_seconds: 37,
      recovery_transcript: [{ channel: "you", text: "before", start_ms: 0, end_ms: 1_000 }],
      recovery_location: { kind: "space", id: 3 },
    },
  });
  // The note page's last save failed; its text is still queued for this draft.
  h.failDraftWrites.add(5);
  h.notepad.edit(draftNotepad(5), "typed on the note page");

  await h.open("?draft=5");
  h.failDraftWrites.delete(5);
  assert.equal(h.state.notes, "typed on the note page");
  assert.equal(h.state.elapsedSeconds, 37);
  h.emit.transcript(speech(1, "after", 250));
  assert.deepEqual(h.state.lines.map((line) => [line.text, line.start_ms]), [["before", 0], ["after", 37_250]]);

  h.advance(5_000);
  await h.session.cancel();
  assert.deepEqual(h.calls, [
    'update_note_draft 5 "typed on the note page"',
    "get_note_draft 5",
    "start_recording",
    'checkpoint 5 "typed on the note page" 42s [before|after] space:3',
    "stop_recording",
    'checkpoint 5 "typed on the note page" 42s [before|after] space:3',
    "navigate /",
  ]);
  // The recording's checkpoint replaced the failed note-page save: an update has nothing stale to replay.
  await h.notepad.flush();
  assert.equal(h.calls.length, 7);
});

test("elapsed time catches up after late ticks, excludes pauses, and checkpoints every five seconds", async () => {
  const h = harness();
  await h.open();
  const recovery = () => h.notepad.pending(draftNotepad(5))?.recovery?.durationSeconds;

  h.advance(3_900);
  assert.equal(h.state.elapsedSeconds, 3);
  assert.equal(recovery(), undefined);
  h.advance(1_600);
  assert.equal(h.state.elapsedSeconds, 5);
  assert.equal(recovery(), 5);

  await h.session.pause();
  h.advance(50_000);
  assert.equal(h.state.elapsedSeconds, 5, "paused time does not count");
  await h.session.resume();
  h.advance(10_000);
  assert.equal(h.state.elapsedSeconds, 15, "one late tick catches up");
  assert.equal(recovery(), 15);
  await h.session.cancel();
});

test("capture problems are surfaced, dismissable, and re-raised when capture resumes", async () => {
  const h = harness();
  h.native.startRecording = async () => ({ meeting_audio_available: false, transcription_available: false });
  await h.open();
  assert.equal(h.state.meetingAudioMissing, true);
  assert.equal(h.state.transcriptionUnavailable, true);

  h.emit.pressure(480, 0);
  h.emit.quitBlocked();
  assert.deepEqual(h.state.audioPressure, { mic_dropped_samples: 480, loopback_dropped_samples: 0 });
  assert.equal(h.state.quitBlocked, true);

  for (const warning of ["meetingAudio", "transcription", "audioPressure", "quit"] as const) h.session.dismiss(warning);
  assert.equal(h.state.meetingAudioMissing || h.state.transcriptionUnavailable || h.state.quitBlocked, false);
  assert.equal(h.state.audioPressure, null);

  h.native.resumeRecording = async () => ({ meeting_audio_available: false, transcription_available: true });
  await h.session.pause();
  await h.session.resume();
  assert.equal(h.state.meetingAudioMissing, true);
  assert.equal(h.state.transcriptionUnavailable, false);
  await h.session.cancel();
});

test("pause and resume failures leave the phase unchanged and say why", async () => {
  const h = harness();
  await h.open();
  h.native.pauseRecording = async () => { throw new Error("device busy"); };
  await h.session.pause();
  assert.equal(h.state.phase, "recording");
  assert.deepEqual(h.state.controlError, { action: "pause", message: "device busy" });

  h.native.pauseRecording = async () => {};
  h.native.resumeRecording = async () => { throw new Error("microphone unplugged"); };
  await h.session.pause();
  assert.equal(h.state.controlError, null);
  await h.session.resume();
  assert.equal(h.state.phase, "paused");
  assert.deepEqual(h.state.controlError, { action: "resume", message: "microphone unplugged" });
  await h.session.cancel();
});

test("unmounting keeps the recovery copy and stops capture; the update boundary waits for it", async () => {
  const h = harness();
  await h.open();
  h.state.notes = "kept";
  const stopped = deferred<{ transcript: TranscriptEvent[]; finalization_warning: string | null }>();
  h.native.stopRecording = () => stopped.promise;

  h.session.release();
  await settle();
  let settled = false;
  const waiting = h.session.settled().then(() => { settled = true; });
  await settle();
  assert.equal(h.session.active, true);
  assert.equal(settled, false);

  stopped.resolve({ transcript: [speech(1, "last words")], finalization_warning: null });
  await waiting;
  assert.equal(h.session.active, false);
  assert.deepEqual(h.calls.slice(2), [
    'checkpoint 5 "kept" 0s [] root',
    "stop_recording",
    'checkpoint 5 "kept" 0s [last words] root',
  ]);
});

const recoveredDraft = (): NoteDraftDetail => ({
  id: 5,
  title: "Draft",
  created_at: "2026-10-01T09:00:00Z",
  updated_at: "2026-10-01T09:00:00Z",
  raw_markdown: "recovered notes",
  meeting_id: null,
  recovery_duration_seconds: 10,
  recovery_transcript: [],
  recovery_location: null,
});

test("a finished Meeting does not depend on its recovery checkpoints", async () => {
  const h = harness();
  await h.open();
  h.failDraftWrites.add(5);
  h.state.notes = "save these notes";
  h.session.notesChanged();
  await h.session.finish("save");
  assert.ok(h.calls.includes('save_meeting draft=5 root "save these notes" 0s []'));
  assert.equal(h.calls.at(-1), "navigate /meeting/9");
});

for (const exit of ["recovery-copy", "cancel"] as const) {
  test(`${exit === "cancel" ? "cancel" : "opening the recovery copy"} from a resumed draft leaves only once the notes and final transcript are saved`, async () => {
    const h = harness({ draft: recoveredDraft() });
    h.native.stopRecording = async () => ({ transcript: [speech(1, "final words")], finalization_warning: null });
    const leave = () => (exit === "cancel" ? h.session.cancel() : h.session.leave("recovery-copy"));
    await h.open("?draft=5");
    h.failDraftWrites.add(5);
    h.state.notes = "changed recovered notes";
    h.session.notesChanged();

    await leave();
    assert.equal(h.state.phase, "finish_failed");
    assert.equal(h.state.finishError, "disk is busy");
    assert.equal(h.state.notes, "changed recovered notes");
    assert.equal(h.session.guardsNavigation, true);
    assert.equal(h.calls.some((call) => call.startsWith("navigate")), false);

    h.failDraftWrites.delete(5);
    await leave();
    assert.equal(h.calls.at(-2), 'checkpoint 5 "changed recovered notes" 10s [final words] root');
    assert.equal(h.calls.at(-1), exit === "cancel" ? "navigate /" : "navigate /note/5");
    assert.deepEqual(h.calls.filter((call) => call.startsWith("delete_note_draft")), []);
    assert.deepEqual(h.state.lines.map((line) => line.start_ms), [10_000], "the final transcript is offset");
  });
}

for (const waitingOn of ["draft", "capture"] as const) {
  test(`a leave queued behind startup waits for the ${waitingOn}, then stays after a failed checkpoint`, async () => {
    const h = harness();
    const ready = deferred<unknown>();
    if (waitingOn === "draft") h.native.createNoteDraft = () => ready.promise as Promise<number>;
    else h.native.startRecording = () => ready.promise as Promise<RecordingStartStatus>;
    h.failDraftWrites.add(5);
    const opening = h.open();
    await settle();
    await h.session.leave("/settings");
    assert.equal(h.session.blocksNavigation, true, "other navigation waits for the queued leave");

    ready.resolve(waitingOn === "draft" ? 5 : capturing);
    await opening;
    assert.equal(h.state.phase, "finish_failed");
    assert.equal(h.state.finishError, "disk is busy");
    assert.equal(h.state.exitPending, false);
    assert.equal(h.session.blocksNavigation, false, "the failure is shown, not a silent lock");
    assert.equal(h.calls.some((call) => call.startsWith("navigate")), false);
  });
}

test("no other navigation can bypass a recovery write still in flight", async () => {
  const h = harness();
  await h.open();
  const stored = deferred();
  h.writes.gate = stored.promise;
  const blockedWhileNavigating: boolean[] = [];
  h.native.navigate = async () => { blockedWhileNavigating.push(h.session.blocksNavigation); };

  const leaving = h.session.leave("/settings");
  await settle();
  assert.equal(h.state.phase, "stopping");
  assert.equal(h.session.blocksNavigation, true);
  stored.resolve();
  await leaving;
  assert.equal(h.calls.at(-1), "navigate /settings");
  assert.deepEqual(blockedWhileNavigating, [false], "the teardown's own navigation is allowed");
  assert.equal(h.session.blocksNavigation, false);
});

test("Back to meetings after a startup failure keeps unsaved notes when the write fails", async () => {
  const h = harness();
  h.native.startRecording = async () => { throw new Error("microphone unavailable"); };
  await h.open();
  h.failDraftWrites.add(5);
  h.state.notes = "notes without capture";
  h.session.notesChanged();
  assert.equal(h.session.guardsNavigation, true);

  await h.session.cancel();
  assert.equal(h.state.phase, "finish_failed");
  assert.equal(h.state.notes, "notes without capture");
  assert.equal(h.calls.some((call) => call.startsWith("navigate") || call.startsWith("delete_note_draft")), false);

  h.failDraftWrites.delete(5);
  await h.session.cancel();
  assert.deepEqual(h.calls.slice(-2), ['checkpoint 5 "notes without capture" 0s [] root', "navigate /"]);
});

for (const resumed of [false, true]) {
  test(`discarding after a failed leave drops unsaved edits${resumed ? " but keeps an existing draft" : " and the automatic draft"}`, async () => {
    const h = harness({ draft: recoveredDraft() });
    await h.open(resumed ? "?draft=5" : "");
    h.failDraftWrites.add(5);
    h.state.notes = "discard me";
    h.session.notesChanged();
    await h.session.leave("/settings");
    assert.equal(h.state.failedLeave, "/settings");

    await h.session.discard();
    assert.equal(h.calls.at(-1), "navigate /settings");
    assert.deepEqual(h.calls.filter((call) => call.startsWith("delete_note_draft")), resumed ? [] : ["delete_note_draft 5"]);
    assert.equal(h.calls.some((call) => call.startsWith("save_meeting")), false);
    const written = h.calls.length;
    await h.notepad.flush();
    assert.equal(h.calls.length, written, "discarded edits are never written");
  });
}

test("Keep recovery copy after a failed stop keeps the automatic draft", async () => {
  const h = harness();
  await h.open();
  h.native.stopRecording = async () => { throw new Error("capture could not stop"); };
  h.state.notes = "keep me after failed stop";
  h.session.notesChanged();
  await h.session.finish("save");
  assert.equal(h.state.phase, "finish_failed");
  assert.equal(h.state.finishError, "capture could not stop");

  h.native.stopRecording = async () => ({ transcript: [], finalization_warning: null });
  await h.session.cancel();
  assert.deepEqual(h.calls.filter((call) => call.startsWith("delete_note_draft")), []);
  assert.deepEqual(h.calls.slice(-2), ['checkpoint 5 "keep me after failed stop" 0s [] root', "navigate /"]);
});

test("a cancel whose stop fails keeps the draft and its unsaved edits until a retry stops capture", async () => {
  const h = harness();
  await h.open();
  h.state.notes = "typed while recording";
  h.session.notesChanged();
  h.native.stopRecording = async () => { throw new Error("capture could not stop"); };

  await h.session.cancel();
  assert.equal(h.state.phase, "finish_failed");
  assert.equal(h.state.finishError, "capture could not stop");
  assert.equal(h.state.failedLeave, "/");
  assert.deepEqual(h.calls.filter((call) => call.startsWith("delete_note_draft") || call.startsWith("navigate")), []);
  assert.equal(h.notepad.pending(draftNotepad(5))?.text, "typed while recording", "pending recovery writes survive");

  // Retrying the discard deletes the draft only once capture has stopped.
  h.native.stopRecording = async () => ({ transcript: [], finalization_warning: null });
  await h.session.discard();
  assert.deepEqual(h.calls.slice(-3), ["stop_recording", "delete_note_draft 5", "navigate /"]);
  assert.equal(h.session.active, false);
});

test("a recovery copy that could not be created at startup is created when leaving", async () => {
  const h = harness();
  h.native.createNoteDraft = async () => { throw new Error("database unavailable"); };
  await h.open();
  assert.equal(h.state.phase, "recording");
  assert.equal(h.state.recoveryStatus, "Recovery copy unavailable");
  h.state.notes = "notes without an initial draft";
  h.session.notesChanged();

  await h.session.leave("/");
  assert.equal(h.state.phase, "finish_failed");
  assert.equal(h.state.finishError, "database unavailable");

  h.native.createNoteDraft = async () => 5;
  await h.session.retryFinish();
  assert.deepEqual(h.calls.slice(-3), [
    "create_note_draft root",
    'checkpoint 5 "notes without an initial draft" 0s [] root',
    "navigate /",
  ]);
});

test("Save as meeting after a failed leave keeps that intent on retry", async () => {
  const h = harness();
  await h.open();
  h.failDraftWrites.add(5);
  await h.session.leave("/");
  assert.equal(h.state.failedLeave, "/");

  h.native.saveMeeting = async () => { throw new Error("meeting write failed"); };
  await h.session.finish("save");
  assert.equal(h.state.phase, "finish_failed");
  assert.equal(h.state.failedLeave, null);

  h.native.saveMeeting = async () => 9;
  await h.session.retryFinish();
  assert.equal(h.calls.at(-1), "navigate /meeting/9");
});

test("Retry saving resumes a failed recovery-only leave without creating a Meeting", async () => {
  const h = harness();
  await h.open();
  h.failDraftWrites.add(5);
  h.state.notes = "keep these notes";
  await h.session.leave("/");

  h.failDraftWrites.delete(5);
  h.state.notes = "latest after recovery failure";
  h.session.notesChanged();
  await h.session.retryFinish();
  assert.deepEqual(h.calls.slice(-2), ['checkpoint 5 "latest after recovery failure" 0s [] root', "navigate /"]);
  assert.equal(h.calls.some((call) => call.startsWith("save_meeting")), false);
});

test("a lazy checkpoint written after its session closed still holds that session's transcript", async () => {
  const h = harness();
  h.native.startRecording = async () => { throw new Error("microphone unavailable"); };
  await h.open();
  // Startup failed, so closing stops nothing and saves nothing: the debounced
  // checkpoint is still pending, built from session state when it is written.
  h.emit.transcript(speech(1, "first session"));
  h.state.notes = "first notes";
  h.session.notesChanged();
  h.session.release();
  await h.idle();
  assert.equal(h.session.active, false);
  assert.equal(h.calls.some((call) => call.startsWith("checkpoint")), false);

  h.native.startRecording = async () => capturing;
  h.native.createNoteDraft = async () => 6;
  await h.open();
  h.emit.transcript(speech(1, "second session"));
  await h.notepad.flush(draftNotepad(5));
  assert.equal(h.calls.at(-1), 'checkpoint 5 "first notes" 0s [first session] root');
  await h.session.cancel();
});
