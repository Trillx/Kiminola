import assert from "node:assert/strict";
import { test } from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { createNotepadStore, draftNotepad, meetingNotepad, notepad, type NotepadPort, type RecoveryCheckpoint } from "../src/lib/notepad-store.ts";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { flushPendingWork } from "../src/lib/pending-work.ts";

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** An in-memory database behind the Notepad port; `fail` names documents whose writes reject. */
function database() {
  const rows = new Map<string, { text: string; recovery?: RecoveryCheckpoint }>();
  const writes: string[] = [];
  const fail = new Set<string>();
  const gates = new Map<string, Promise<void>>();
  async function write(key: string, command: string, text: string, recovery?: RecoveryCheckpoint) {
    writes.push(`${command} ${key} ${text}`);
    await gates.get(text);
    if (fail.has(key)) throw new Error(`${key} is read-only`);
    rows.set(key, { text, recovery: recovery ?? rows.get(key)?.recovery });
  }
  const port: NotepadPort = {
    updateNotes: (id, text) => write(`meeting:${id}`, "update_notes", text),
    updateNoteDraft: (id, text) => write(`draft:${id}`, "update_note_draft", text),
    updateNoteDraftRecovery: (id, text, durationSeconds, transcript, location) =>
      write(`draft:${id}`, "update_note_draft_recovery", text, { durationSeconds, transcript, location }),
  };
  return { rows, writes, fail, gates, port };
}

const checkpoint = (seconds: number, text: string): RecoveryCheckpoint => ({
  durationSeconds: seconds,
  transcript: [{ channel: "you", text, start_ms: 0, end_ms: 1_000 }],
  location: { kind: "space", id: 4 },
});

test("a failed note-page save cannot later overwrite a recording checkpoint of the same draft", async () => {
  const db = database();
  const store = createNotepadStore(db.port, { delayMs: 60_000 });
  const draft = draftNotepad(5);

  // The note page's save fails and stays pending...
  db.fail.add(draft);
  store.edit(draft, "typed on the note page");
  await assert.rejects(store.flush(draft), /read-only/);

  // ...then the recording checkpoints the same draft through the same queue.
  db.fail.delete(draft);
  await store.save(draft, "typed while recording", checkpoint(12, "spoken words"));

  // The update boundary flushes everything: nothing stale is left to replay.
  await store.flush();
  assert.equal(db.rows.get(draft)?.text, "typed while recording");
  assert.equal(db.rows.get(draft)?.recovery?.durationSeconds, 12);
  assert.equal(store.pending(draft), undefined);
  assert.equal(db.writes.filter((write) => write.includes("typed on the note page")).length, 1);
});

test("a text-only edit keeps the recovery checkpoint it has not saved yet", async () => {
  const db = database();
  const store = createNotepadStore(db.port, { delayMs: 60_000 });
  const draft = draftNotepad(5);
  db.fail.add(draft);
  await assert.rejects(store.save(draft, "notes", checkpoint(30, "recorded speech")));

  store.edit(draft, "notes edited on the recovery copy");
  db.fail.delete(draft);
  await store.flush(draft);

  assert.deepEqual(db.rows.get(draft), {
    text: "notes edited on the recovery copy",
    recovery: checkpoint(30, "recorded speech"),
  });
  assert.match(db.writes.at(-1)!, /^update_note_draft_recovery /);
});

test("a lazy recovery checkpoint is built once, from the latest state, when its write runs", async () => {
  const db = database();
  const store = createNotepadStore(db.port, { delayMs: 60_000 });
  const draft = draftNotepad(5);
  let built = 0;
  let seconds = 0;
  const live = () => { built++; return checkpoint(seconds, `speech at ${seconds}s`); };

  for (seconds = 1; seconds <= 50; seconds++) store.edit(draft, "notes", live);
  assert.equal(built, 0, "edits only schedule");
  seconds = 99;
  await store.flush(draft);
  assert.equal(built, 1);
  assert.equal(db.rows.get(draft)?.recovery?.durationSeconds, 99);

  // A text-only edit keeps a lazy checkpoint that failed to save.
  db.fail.add(draft);
  store.edit(draft, "notes", live);
  await assert.rejects(store.flush(draft));
  store.edit(draft, "notes edited on the recovery copy");
  db.fail.delete(draft);
  await store.flush(draft);
  assert.deepEqual(db.rows.get(draft), { text: "notes edited on the recovery copy", recovery: checkpoint(99, "speech at 99s") });
});

test("every status listener keeps receiving updates whoever writes, until it unsubscribes", async () => {
  const db = database();
  const store = createNotepadStore(db.port, { delayMs: 60_000 });
  const draft = draftNotepad(7);
  const page: string[] = [];
  const recorder: string[] = [];
  const unsubscribePage = store.subscribe(draft, (status) => page.push(status));
  store.subscribe(draft, (status) => recorder.push(status));

  // A second writer with no listener of its own must not detach the page's.
  await store.save(draft, "started recording");
  assert.deepEqual(page, ["idle", "saving", "saved"]);
  assert.deepEqual(recorder, ["idle", "saving", "saved"]);

  unsubscribePage();
  await store.save(draft, "later");
  assert.deepEqual(page, ["idle", "saving", "saved"]);
  assert.deepEqual(recorder.slice(-2), ["saving", "saved"]);
});

test("unsaved text outlives its editor, is preferred over the database, and is reported until saved", async () => {
  const db = database();
  const store = createNotepadStore(db.port, { delayMs: 60_000 });
  const a = meetingNotepad(1);
  db.rows.set(a, { text: "A database value" });
  db.fail.add(a);
  store.edit(a, "A unsaved");

  // Navigating to another meeting still loads it; the write failure is reported separately.
  const errors: unknown[] = [];
  const destination = await store.read(async () => "meeting 2", (error) => errors.push(error));
  assert.equal(destination, "meeting 2");
  assert.equal(errors.length, 1);

  // A returning page sees the failure immediately and the unsaved text over the database row.
  const statuses: string[] = [];
  store.subscribe("all", (status) => statuses.push(status));
  assert.equal(statuses.at(-1), "error");
  assert.equal(store.pending(a)?.text ?? db.rows.get(a)?.text, "A unsaved");

  // Saving another meeting must not hide A's failure.
  await store.save(meetingNotepad(2), "B edited");
  assert.equal(statuses.at(-1), "error");

  db.fail.delete(a);
  await store.flush();
  assert.equal(db.rows.get(a)?.text, "A unsaved");
  assert.equal(store.pending(a), undefined);
  assert.equal(statuses.at(-1), "saved");
});

test("documents are independent: a scoped flush neither retries nor cancels another document", async () => {
  const db = database();
  const store = createNotepadStore(db.port, { delayMs: 10 });
  db.fail.add(meetingNotepad(1));
  store.edit(meetingNotepad(1), "A failing");
  await assert.rejects(store.flush(meetingNotepad(1)));
  const failedWrites = db.writes.length;

  store.edit(meetingNotepad(2), "B");
  store.edit(meetingNotepad(3), "C");
  await store.flush(meetingNotepad(2));
  assert.equal(db.writes.length, failedWrites + 1, "flushing B must not retry A");
  await wait(30);
  assert.equal(db.rows.get(meetingNotepad(3))?.text, "C", "C's own debounce still fires");
  assert.equal(store.pending(meetingNotepad(1))?.text, "A failing");
});

test("rapid edits coalesce and a slower older write cannot win", async () => {
  const db = database();
  const store = createNotepadStore(db.port, { delayMs: 5 });
  const a = meetingNotepad(1);
  store.edit(a, "first");
  store.edit(a, "second");
  await wait(20);
  assert.deepEqual(db.writes, ["update_notes meeting:1 second"]);

  let release!: () => void;
  db.gates.set("slow", new Promise<void>((resolve) => { release = resolve; }));
  const slow = store.save(a, "slow");
  store.edit(a, "newest");
  const reading = store.read(async () => db.rows.get(a)?.text);
  await wait(20);
  assert.equal(db.rows.get(a)?.text, "second", "nothing lands while the older write is in flight");
  release();
  await slow;
  assert.equal(await reading, "newest", "reads wait for in-flight and pending writes");
});

test("discarding a document waits for its in-flight write, then never writes it again", async () => {
  const db = database();
  const store = createNotepadStore(db.port, { delayMs: 60_000 });
  const draft = draftNotepad(9);
  let release!: () => void;
  db.gates.set("in flight", new Promise<void>((resolve) => { release = resolve; }));
  const statuses: string[] = [];
  store.subscribe(draft, (status) => statuses.push(status));
  const inFlight = store.save(draft, "in flight");
  store.edit(draft, "never written");

  let discarded = false;
  const discarding = store.discard(draft).then(() => { discarded = true; });
  await wait(5);
  assert.equal(discarded, false);
  release();
  await Promise.all([inFlight, discarding]);
  await store.flush();

  assert.deepEqual(db.writes, ["update_note_draft draft:9 in flight"]);
  assert.equal(store.pending(draft), undefined);
  assert.equal(statuses.at(-1), "idle");
});

test("the app's store is registered with the update boundary", async () => {
  Object.defineProperty(globalThis, "window", { value: { crypto: globalThis.crypto }, configurable: true });
  const commands: Array<[string, unknown]> = [];
  mockIPC((command, payload) => {
    commands.push([command, payload]);
  });
  try {
    notepad.edit(meetingNotepad(42), "typed just before updating");
    await flushPendingWork();
    assert.deepEqual(commands, [["update_notes", { meetingId: 42, rawMarkdown: "typed just before updating" }]]);
  } finally {
    clearMocks();
    Reflect.deleteProperty(globalThis, "window");
  }
});
