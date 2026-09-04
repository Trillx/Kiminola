import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { createMeetingNotesAutosave } from "../src/lib/meeting-notes.ts";

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
