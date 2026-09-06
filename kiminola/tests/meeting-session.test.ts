import assert from "node:assert/strict";
import { test } from "node:test";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
// @ts-expect-error Node strip-types imports the production session.
import { meetingNotesAutosave } from "../src/lib/meeting-notes-session.ts";
// @ts-expect-error Node strip-types imports the production export handler.
import { exportMeeting } from "../src/lib/meeting-export.ts";

// @ts-expect-error Node strip-types imports the production update boundary.
import { flushPendingWork } from "../src/lib/pending-work.ts";

test("failed edits survive page detachment, exports, and returning to the meeting", async () => {
  Object.defineProperty(globalThis, "window", { value: { crypto: globalThis.crypto }, configurable: true });
  const previousNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const clipboard: string[] = [];
  Object.defineProperty(globalThis, "navigator", {
    value: { clipboard: { writeText: async (text: string) => { clipboard.push(text); } } },
    configurable: true,
  });
  const notes = new Map([[1, "A saved"], [2, "B saved"]]);
  const calls: string[] = [];
  let failA = true;
  mockIPC((command, payload) => {
    calls.push(command);
    assert.ok(payload && "meetingId" in payload);
    const id = Number(payload.meetingId);
    if (command === "update_notes") {
      if (id === 1 && failA) throw new Error("database is read-only");
      assert.ok("rawMarkdown" in payload);
      notes.set(id, String(payload.rawMarkdown));
    } else if (command === "export_transcript_text") {
      return "Saved transcript";
    } else if (command === "save_transcript_export") {
      return "transcript.txt";
    } else if (command === "export_notes_markdown") {
      return notes.get(id);
    } else if (command === "save_notes_export") {
      return "notes.md";
    } else {
      assert.fail(`Unexpected command: ${command}`);
    }
  });
  let detach = () => {};
  try {
    const oldPageStatuses: string[] = [];
    detach = meetingNotesAutosave.subscribe((status) => oldPageStatuses.push(status));
    meetingNotesAutosave.schedule(1, "A edited");
    // The page unsubscribes and flushes on destruction; the session survives.
    detach();
    const detachedStatusCount = oldPageStatuses.length;
    await assert.rejects(meetingNotesAutosave.flush(), /read-only/);
    assert.equal(oldPageStatuses.length, detachedStatusCount);

    const returningPageStatuses: string[] = [];
    detach = meetingNotesAutosave.subscribe((status) => returningPageStatuses.push(status));
    assert.equal(returningPageStatuses.at(-1), "error");
    assert.equal(meetingNotesAutosave.pendingNotes(1) ?? notes.get(1), "A edited");

    calls.length = 0;
    await exportMeeting("copy-transcript", 1);
    const transcriptFile = await exportMeeting("save-transcript", 1);
    assert.deepEqual(calls, ["export_transcript_text", "save_transcript_export"]);
    assert.deepEqual(clipboard, ["Saved transcript"]);
    assert.equal(transcriptFile.path, "transcript.txt");

    meetingNotesAutosave.schedule(2, "B edited");
    await exportMeeting("copy-notes", 2);
    assert.equal(clipboard.at(-1), "B edited");
    assert.equal(meetingNotesAutosave.pendingNotes(1), "A edited");
    await assert.rejects(exportMeeting("copy-notes", 1), /read-only/);
    assert.equal(clipboard.at(-1), "B edited", "do not copy stale notes after a failed save");

    failA = false;
    await meetingNotesAutosave.flush();
    assert.equal(notes.get(1), "A edited");
    assert.equal(meetingNotesAutosave.pendingNotes(1), undefined);
    assert.equal(returningPageStatuses.at(-1), "saved");
    meetingNotesAutosave.schedule(1, "A final");
    const notesFile = await exportMeeting("save-notes", 1);
    assert.equal(notes.get(1), "A final");
    assert.equal(notesFile.path, "notes.md");
    detach();
    meetingNotesAutosave.schedule(1, "Saved before update");
    await flushPendingWork();
    assert.equal(notes.get(1), "Saved before update");
  } finally {
    detach();
    failA = false;
    await meetingNotesAutosave.flush();
    clearMocks();
    Reflect.deleteProperty(globalThis, "window");
    if (previousNavigator) Object.defineProperty(globalThis, "navigator", previousNavigator);
    else Reflect.deleteProperty(globalThis, "navigator");
  }
});
