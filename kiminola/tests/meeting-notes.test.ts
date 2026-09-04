import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { createMeetingNotesAutosave, loadMeetingAfterAutosave } from "../src/lib/meeting-notes.ts";

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

test("navigation cannot save the next meeting's text into the previous meeting", async () => {
  const writes: Array<[number, string]> = [];
  const autosave = createMeetingNotesAutosave(async (id, notes) => { writes.push([id, notes]); }, () => {}, 10);
  let meeting = { id: 1, notes: "A edited" };
  autosave.schedule(meeting.id, meeting.notes);
  meeting = { id: 2, notes: "B existing" };
  await autosave.flush();
  assert.deepEqual(writes, [[1, "A edited"]]);
  assert.equal(meeting.notes, "B existing");
  await autosave.close();
});

test("rapid edits in two meetings retain both destinations and their latest text", async () => {
  const writes: Array<[number, string]> = [];
  const autosave = createMeetingNotesAutosave(async (id, notes) => { writes.push([id, notes]); }, () => {}, 10);
  autosave.schedule(1, "A first");
  autosave.schedule(1, "A latest");
  autosave.schedule(2, "B latest");
  await wait(30);
  assert.deepEqual(writes, [[1, "A latest"], [2, "B latest"]]);
  await autosave.close();
});

test("returning to a meeting serializes a newer edit behind an older in-flight write", async () => {
  const writes: Array<[number, string]> = [];
  const gate = Promise.withResolvers<void>();
  const started = Promise.withResolvers<void>();
  const autosave = createMeetingNotesAutosave(async (id, notes) => {
    if (notes === "A old") { started.resolve(); await gate.promise; }
    writes.push([id, notes]);
  }, () => {}, 0);
  autosave.schedule(1, "A old");
  await started.promise;
  autosave.schedule(2, "B");
  autosave.schedule(1, "A new");
  const closed = autosave.close();
  gate.resolve();
  await closed;
  assert.equal(writes.filter(([id]) => id === 1).at(-1)?.[1], "A new");
  assert.ok(writes.some(([id, notes]) => id === 2 && notes === "B"));
});

test("a failed flush retains the text for an explicit retry", async () => {
  let fail = true;
  const saved: string[] = [];
  const autosave = createMeetingNotesAutosave(async (_id, notes) => {
    if (fail) throw new Error("disk full");
    saved.push(notes);
  }, () => {}, 1000);
  autosave.schedule(1, "unsaved edit");
  await assert.rejects(autosave.flush(), /disk full/);
  fail = false;
  await autosave.flush();
  assert.deepEqual(saved, ["unsaved edit"]);
  await autosave.close();
});


test("failed autosave still loads the destination and retains each meeting's edit for retry", async () => {
  let failA = true;
  let status = "";
  const saved: Array<[number, string]> = [];
  const errors: unknown[] = [];
  const autosave = createMeetingNotesAutosave(async (id, notes) => {
    if (id === 1 && failA) throw new Error("disk full");
    saved.push([id, notes]);
  }, (next) => { status = next; }, 1000);
  autosave.schedule(1, "A unsaved");
  const destination = await loadMeetingAfterAutosave(autosave, async () => ({ id: 2, notepad: "B existing" }), (error) => errors.push(error));
  assert.equal(destination.id, 2);
  assert.equal(errors.length, 1);
  assert.equal(autosave.pendingNotes(1), "A unsaved");

  autosave.schedule(2, "B edited");
  await assert.rejects(autosave.flush(), /disk full/);
  assert.equal(autosave.pendingNotes(1), "A unsaved");
  assert.equal(autosave.pendingNotes(2), undefined);
  assert.equal(status, "error", "saving B must not hide the failed edit in A");
  assert.ok(saved.some(([id, notes]) => id === 2 && notes === "B edited"));

  const returned = await loadMeetingAfterAutosave(autosave, async () => ({ id: 1, notepad: "A old database value" }), (error) => errors.push(error));
  assert.equal(autosave.pendingNotes(returned.id) ?? returned.notepad, "A unsaved");
  failA = false;
  await autosave.flush();
  assert.equal(autosave.pendingNotes(1), undefined);
  assert.equal(status, "saved");
  assert.ok(saved.some(([id, notes]) => id === 1 && notes === "A unsaved"));
  await autosave.close();
});

test("destination lookup errors remain distinct from autosave errors", async () => {
  const saveError = new Error("write failed");
  const loadError = new Error("meeting missing");
  const errors: unknown[] = [];
  await assert.rejects(loadMeetingAfterAutosave(
    { flush: async () => { throw saveError; } },
    async () => { throw loadError; },
    (error) => errors.push(error),
  ), (error) => error === loadError);
  assert.deepEqual(errors, [saveError]);
});

test("destination reads wait for an in-flight save", async () => {
  const gate = Promise.withResolvers<void>();
  let loaded = false;
  const pending = loadMeetingAfterAutosave({ flush: () => gate.promise }, async () => { loaded = true; return "loaded"; }, () => {});
  await Promise.resolve();
  assert.equal(loaded, false);
  gate.resolve();
  assert.equal(await pending, "loaded");
});
