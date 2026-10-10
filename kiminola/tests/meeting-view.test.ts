import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { createMeetingView, type MeetingViewPort } from "../src/lib/meeting-view.ts";
// @ts-expect-error Node strip-types imports the production TypeScript module.
import { createNotepadStore, meetingNotepad } from "../src/lib/notepad-store.ts";
import type { LlmStreamEvent, MeetingDetail, ProviderConfig } from "../src/lib/tauri";

const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

function deferred<T = void>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

const configuredProvider: ProviderConfig = {
  kind: "open_router",
  base_url: "https://openrouter.ai/api/v1",
  model: "openai/gpt-4o-mini",
  has_api_key: true,
};

/** A fake database and native side. `observed` records what each read or stream saw in the database. */
function harness(overrides: Partial<MeetingViewPort> = {}) {
  const notes = new Map([[1, "notes 1"], [2, "notes 2"]]);
  const enhanced = new Map<number, string>();
  const observed: string[] = [];
  const streams: Array<(event: LlmStreamEvent) => void> = [];
  const timers = new Set<() => void>();
  const renders: string[] = [];
  const writes = { fail: false, gate: Promise.resolve() };
  const notepad = createNotepadStore({
    updateNotes: async (id, text) => {
      await writes.gate;
      if (writes.fail) throw new Error("disk full");
      notes.set(id, text);
    },
    updateNoteDraft: async () => {},
    updateNoteDraftRecovery: async () => {},
  }, { delayMs: 60_000 });
  const meeting = (id: number): MeetingDetail => ({
    id,
    title: `Meeting ${id}`,
    created_at: "2026-10-01T09:00:00Z",
    duration_seconds: 60,
    space_name: null,
    location_path: null,
    parent_meeting_id: null,
    notepad: notes.get(id) ?? "",
    enhanced_markdown: enhanced.get(id) ?? null,
    transcript: [
      { id: id * 10, channel: "you", text: `Original ${id}` },
      { id: id * 10 + 1, channel: "others", text: `Other ${id}` },
    ],
  });
  const port: MeetingViewPort = {
    getMeeting: async (id) => meeting(id),
    getLlmConfig: async () => configuredProvider,
    listTemplates: async () => [{ id: 1, name: "General", prompt: "", is_builtin: 1 }],
    enhanceMeeting: async (id, _templateId, onEvent) => {
      observed.push(`enhance ${notes.get(id)}`);
      streams.push(onEvent);
    },
    exportNotesMarkdown: async (id) => { observed.push(`export ${notes.get(id)}`); return notes.get(id)!; },
    exportTranscriptText: async () => "You: hello",
    saveNotesExport: async (id) => { observed.push(`export ${notes.get(id)}`); return "notes.md"; },
    saveTranscriptExport: async () => "transcript.txt",
    updateSegmentText: async (id, text) => { observed.push(`segment ${id} "${text}"`); },
    deleteSegment: async (id) => { observed.push(`delete segment ${id}`); },
    updateEnhancedActionItems: async ({ meetingId, edits }) => {
      observed.push(`action items ${meetingId} ${edits.map((edit) => `${edit.sourceIndex}:${edit.title}`).join("|")}`);
    },
    writeClipboard: async () => {},
    renderMarkdown: (markdown) => { renders.push(markdown); return `<p>${markdown}</p>`; },
    delay: (_milliseconds, run) => { timers.add(run); return () => timers.delete(run); },
    ...overrides,
  };
  const view = createMeetingView({ port, notepad, log: () => {} });
  return {
    view,
    state: view.state,
    notepad,
    observed,
    streams,
    renders,
    writes,
    enhanced,
    open: (id: number, search = "") => view.open(id, new URLSearchParams(search)),
    edit(text: string) { view.state.notes = text; view.notesChanged(); },
    runTimers() { const due = [...timers]; timers.clear(); due.forEach((run) => run()); },
  };
}

for (const action of ["copy-notes", "save-notes", "enhance"] as const) {
  test(`${action} includes notes typed immediately before it`, async () => {
    const h = harness();
    h.open(1);
    await settle();
    h.edit("latest edit");
    if (action === "enhance") await h.view.enhance();
    else await h.view.export(action);
    assert.deepEqual(h.observed, [`${action === "enhance" ? "enhance" : "export"} latest edit`]);
  });
}

test("a failed notes save blocks a notes export instead of exporting stale notes", async () => {
  const h = harness();
  h.open(1);
  await settle();
  h.writes.fail = true;
  h.edit("unsaved edit");
  await assert.rejects(h.view.export("save-notes"), /disk full/);
  assert.deepEqual(h.observed, []);
  assert.equal(h.state.notesSaveError, true);
  // Transcript exports need no notes write.
  assert.equal((await h.view.export("save-transcript")).path, "transcript.txt");
});

test("a failed notes save blocks enhancement, keeps the previous result, and can be retried", async () => {
  const h = harness();
  h.open(1);
  await settle();
  h.state.enhancedMd = "previous enhancement";
  h.writes.fail = true;
  h.edit("unsaved edit");
  await h.view.enhance();
  assert.deepEqual(h.observed, []);
  assert.equal(h.state.enhancing, false);
  assert.match(h.state.enhanceError ?? "", /disk full/);
  assert.equal(h.state.enhancedMd, "previous enhancement");

  h.writes.fail = false;
  await h.view.retrySave();
  assert.equal(h.state.notesSaveError, false);
  await h.view.enhance();
  assert.deepEqual(h.observed, ["enhance unsaved edit"]);
  assert.equal(h.state.enhanceError, null);
});

test("an export waits for an in-flight notes write", async () => {
  const h = harness();
  h.open(1);
  await settle();
  const write = deferred();
  h.writes.gate = write.promise;
  h.edit("pending write");
  const exporting = h.view.export("save-notes");
  await settle();
  assert.deepEqual(h.observed, []);
  write.resolve();
  await exporting;
  assert.deepEqual(h.observed, ["export pending write"]);
});

test("automatic enhancement waits for the provider configuration", async () => {
  const config = deferred<ProviderConfig>();
  const h = harness({ getLlmConfig: () => config.promise });
  h.open(1, "mode=enhance");
  await settle();
  assert.equal(h.state.tab, "enhance");
  assert.deepEqual(h.observed, []);
  config.resolve(configuredProvider);
  await settle();
  assert.deepEqual(h.observed, ["enhance notes 1"]);
});

test("automatic enhancement uses the shared provider rule", async () => {
  const cloudWithoutKey = harness({ getLlmConfig: async () => ({ ...configuredProvider, has_api_key: false }) });
  cloudWithoutKey.open(1, "mode=generate");
  await settle();
  assert.equal(cloudWithoutKey.view.configured, false);
  assert.deepEqual(cloudWithoutKey.observed, []);

  const blankModel = harness({ getLlmConfig: async () => ({ ...configuredProvider, model: "   " }) });
  blankModel.open(1, "mode=enhance");
  await settle();
  assert.equal(blankModel.view.configured, false);
  assert.deepEqual(blankModel.observed, []);

  const local = harness({
    getLlmConfig: async () => ({ kind: "ollama", base_url: "http://localhost:11434/v1", model: "llama3.1", has_api_key: false }),
  });
  local.open(1, "mode=enhance");
  await settle();
  assert.equal(local.view.configured, true);
  assert.deepEqual(local.observed, ["enhance notes 1"]);
});

test("a late response for the previous meeting cannot replace the current one", async () => {
  const first = deferred<MeetingDetail>();
  const late = harness({
    getMeeting: (id) => (id === 1 ? first.promise : Promise.resolve(meetingFixture(id))),
  });
  const close = late.open(1);
  close();
  late.open(2);
  await settle();
  assert.equal(late.state.meeting?.id, 2);
  first.resolve(meetingFixture(1));
  await settle();
  assert.equal(late.state.meeting?.id, 2);
  assert.equal(late.state.notes, "notes 2");
});

function meetingFixture(id: number): MeetingDetail {
  return {
    id, title: `Meeting ${id}`, created_at: "2026-10-01T09:00:00Z", duration_seconds: 60, space_name: null,
    location_path: null, parent_meeting_id: null, notepad: `notes ${id}`, enhanced_markdown: null, transcript: [],
  };
}

test("late configuration cannot start enhancement for an abandoned route", async () => {
  const firstConfig = deferred<ProviderConfig>();
  const configs = [firstConfig.promise];
  const h = harness({ getLlmConfig: () => configs.shift() ?? Promise.resolve(configuredProvider) });
  const close = h.open(1, "mode=enhance");
  await settle();
  close();
  h.open(2);
  await settle();
  assert.equal(h.view.configured, true, "the new route already knows its provider");
  firstConfig.resolve(configuredProvider);
  await settle();
  assert.equal(h.state.meeting?.id, 2);
  assert.deepEqual(h.observed, []);
});

test("a stream from a previous route or run never writes into the current one", async () => {
  const h = harness();
  h.open(1);
  await settle();
  await h.view.enhance();
  const previous = h.streams[0];
  h.open(2);
  await settle();
  previous({ event: "chunk", data: "meeting 1 text" });
  previous({ event: "done" });
  assert.equal(h.state.enhancedMd, "");
  assert.equal(h.state.hasEverEnhanced, false);

  await h.view.enhance();
  h.streams[1]({ event: "error", data: "rate limited" });
  assert.equal(h.state.enhancing, false);
  assert.equal(h.state.enhanceError, "rate limited");
});

test("streamed enhancement renders on a throttle and once more when it completes", async () => {
  const h = harness();
  h.open(1);
  await settle();
  await h.view.enhance();
  const stream = h.streams[0];
  stream({ event: "chunk", data: "# Summary" });
  stream({ event: "chunk", data: "\n- one" });
  assert.deepEqual(h.renders, [], "chunks wait for the throttle");
  h.runTimers();
  assert.deepEqual(h.renders, ["# Summary\n- one"]);
  stream({ event: "chunk", data: "\n- two" });
  stream({ event: "done" });
  assert.equal(h.state.enhancedHtml, "<p># Summary\n- one\n- two</p>");
  assert.equal(h.state.enhancedGenerated, true);
  h.runTimers();
  assert.equal(h.renders.length, 2, "completion cancels the pending throttled render");
});

test("returning to a meeting shows its unsaved notes, and leaving saves edits", async () => {
  const h = harness();
  h.open(1);
  await settle();
  h.writes.fail = true;
  h.edit("A unsaved");
  h.open(2);
  await settle();
  assert.equal(h.state.meeting?.id, 2, "a failed save does not turn the destination into a missing meeting");
  assert.equal(h.state.notesSaveError, true);
  h.open(1);
  await settle();
  assert.equal(h.state.notes, "A unsaved");

  h.writes.fail = false;
  h.view.dispose();
  await settle();
  assert.equal(h.notepad.pending(meetingNotepad(1)), undefined);
});

/* ---------- transcript corrections ---------- */

async function editing(overrides: Partial<MeetingViewPort> = {}) {
  const h = harness(overrides);
  h.open(1);
  await settle();
  const [first, second] = h.state.meeting!.transcript;
  return { h, first, second };
}

test("a correction saves trimmed text and closes the editor; Cancel never writes", async () => {
  const { h, first } = await editing();
  h.view.startSegmentEdit(first);
  h.state.segmentText = "Unwanted correction";
  h.view.cancelSegmentEdit();
  await h.view.saveSegmentEdit();
  assert.deepEqual(h.observed, []);
  assert.equal(h.state.editingSegmentId, undefined);

  h.view.startSegmentEdit(first);
  h.state.segmentText = "  Corrected sentence  ";
  await h.view.saveSegmentEdit();
  assert.deepEqual(h.observed, ['segment 10 "Corrected sentence"']);
  assert.equal(h.state.meeting!.transcript[0].text, "Corrected sentence");
  assert.equal(h.state.editingSegmentId, undefined);
});

test("a failed correction keeps the edit and its error until a retry succeeds", async () => {
  let fail = true;
  const writes: string[] = [];
  const { h, first } = await editing({
    updateSegmentText: async (id, text) => {
      writes.push(`${id} ${text}`);
      if (fail) throw new Error("disk full");
    },
  });
  h.view.startSegmentEdit(first);
  h.state.segmentText = "  Corrected sentence  ";
  await h.view.saveSegmentEdit();
  assert.equal(h.state.editingSegmentId, 10);
  assert.equal(h.state.segmentText, "  Corrected sentence  ");
  assert.equal(h.state.meeting!.transcript[0].text, "Original 1");
  assert.equal(h.state.segmentError?.action, "save");
  assert.match(h.state.segmentError!.message, /retry saving/);
  assert.equal(h.state.segmentOperation, null);

  fail = false;
  await h.view.saveSegmentEdit();
  assert.deepEqual(writes, ["10 Corrected sentence", "10 Corrected sentence"]);
  assert.equal(h.state.meeting!.transcript[0].text, "Corrected sentence");
  assert.equal(h.state.editingSegmentId, undefined);
  assert.equal(h.state.segmentError, null);
});

test("a failed delete keeps the correction and offers a delete retry", async () => {
  let fail = true;
  const deletions: number[] = [];
  const { h, first } = await editing({
    deleteSegment: async (id) => {
      deletions.push(id);
      if (fail) throw new Error("disk unavailable");
    },
  });
  h.view.startSegmentEdit(first);
  h.state.segmentText = "Correction to keep after failed deletion";
  await h.view.deleteSegment(10);
  assert.equal(h.state.editingSegmentId, 10);
  assert.equal(h.state.segmentText, "Correction to keep after failed deletion");
  assert.equal(h.state.meeting!.transcript.length, 2);
  assert.equal(h.state.segmentError?.action, "delete");
  assert.match(h.state.segmentError!.message, /could not delete/i);

  fail = false;
  await h.view.deleteSegment(10);
  assert.deepEqual(deletions, [10, 10]);
  assert.equal(h.state.editingSegmentId, undefined);
  assert.deepEqual(h.state.meeting!.transcript.map((line) => line.id), [11]);
  assert.equal(h.state.segmentError, null);
});

for (const operation of ["save", "delete"] as const) {
  test(`an in-flight ${operation} locks the editor against saves, deletes, Cancel, and switching rows`, async () => {
    const written = deferred();
    const writes: string[] = [];
    const deletions: number[] = [];
    const { h, first, second } = await editing({
      updateSegmentText: (id, text) => { writes.push(`${id} ${text}`); return written.promise; },
      deleteSegment: (id) => { deletions.push(id); return written.promise; },
    });
    h.view.startSegmentEdit(first);
    h.state.segmentText = "In flight";
    const running = operation === "save" ? h.view.saveSegmentEdit() : h.view.deleteSegment(10);
    assert.equal(h.state.segmentOperation, operation);

    void h.view.saveSegmentEdit();
    void h.view.deleteSegment(10);
    h.view.cancelSegmentEdit();
    h.view.startSegmentEdit(second);
    assert.deepEqual(writes, operation === "save" ? ["10 In flight"] : []);
    assert.deepEqual(deletions, operation === "delete" ? [10] : []);
    assert.equal(h.state.editingSegmentId, 10);
    assert.equal(h.state.segmentText, "In flight");
    assert.equal(h.state.meeting!.transcript.length, 2, "the row changes only after success");

    written.resolve();
    await running;
    assert.equal(h.state.segmentOperation, null);
    assert.equal(h.state.editingSegmentId, undefined);
    if (operation === "save") assert.equal(h.state.meeting!.transcript[0].text, "In flight");
    else assert.deepEqual(h.state.meeting!.transcript.map((line) => line.id), [11]);
  });
}

for (const operation of ["save", "delete"] as const) {
  for (const outcome of ["resolve", "reject"] as const) {
    for (const destination of [1, 2]) {
      test(`a late ${operation} ${outcome} cannot touch the editor after ${destination === 1 ? "reloading the meeting" : "opening another meeting"}`, async () => {
        const late = deferred();
        const next = deferred();
        let calls = 0;
        const { h } = await editing({
          updateSegmentText: () => (++calls === 1 && operation === "save" ? late.promise : next.promise),
          deleteSegment: () => late.promise,
        });
        h.view.startSegmentEdit(h.state.meeting!.transcript[0]);
        h.state.segmentText = "Old route correction";
        const first = operation === "save" ? h.view.saveSegmentEdit() : h.view.deleteSegment(10);

        h.open(destination);
        await settle();
        assert.equal(h.state.editingSegmentId, undefined, "a route load resets the old editor");
        const line = h.state.meeting!.transcript[0];
        h.view.startSegmentEdit(line);
        h.state.segmentText = "New route correction";
        const saving = h.view.saveSegmentEdit();

        if (outcome === "resolve") late.resolve();
        else late.reject(new Error("old route write failed"));
        await first;
        assert.equal(h.state.meeting!.id, destination);
        assert.deepEqual(h.state.meeting!.transcript.map((item) => item.text), [`Original ${destination}`, `Other ${destination}`]);
        assert.equal(h.state.editingSegmentId, destination * 10);
        assert.equal(h.state.segmentError, null);
        assert.equal(h.state.segmentOperation, "save", "the old write must not unlock the newer one");

        next.resolve();
        await saving;
        assert.equal(h.state.meeting!.transcript[0].text, "New route correction");
      });
    }
  }
}

test("a late segment write does not mutate a disposed view", async () => {
  const written = deferred();
  const { h, first } = await editing({ updateSegmentText: () => written.promise });
  h.view.startSegmentEdit(first);
  h.state.segmentText = "Pending edit";
  const saving = h.view.saveSegmentEdit();
  h.view.dispose();
  written.reject(new Error("destroyed page write failed"));
  await saving;
  assert.equal(h.state.segmentError, null);
  assert.equal(h.state.segmentOperation, "save", "nothing is written back after disposal");
  assert.equal(h.state.meeting!.transcript[0].text, "Original 1");
});

/* ---------- enhanced action items ---------- */

test("renamed action items are saved with their source positions and block regeneration meanwhile", async () => {
  const saved = deferred();
  const inputs: Array<{ originalMarkdown: string; enhancedMarkdown: string }> = [];
  const h = harness({
    updateEnhancedActionItems: (input) => { inputs.push(input); return saved.promise; },
  });
  h.enhanced.set(1, "## Action Items\n- Send recap\n- Book room\n");
  h.open(1);
  await settle();
  assert.deepEqual(h.view.actionItems.map((item: { title: string }) => item.title), ["Send recap", "Book room"]);

  h.view.startActionItemEdit();
  assert.deepEqual(h.state.actionItemDrafts, ["Send recap", "Book room"]);
  h.state.actionItemDrafts[1] = "  ";
  await h.view.saveActionItemEdits();
  assert.equal(h.state.actionItemError, "Action items cannot be empty.");
  assert.equal(inputs.length, 0);

  h.state.actionItemDrafts[1] = "Book the large room";
  const saving = h.view.saveActionItemEdits();
  assert.equal(h.state.savingActionItems, true);
  await h.view.enhance();
  assert.equal(h.observed.some((entry) => entry.startsWith("enhance")), false, "no regeneration while editing");
  h.view.cancelActionItemEdit();
  assert.equal(h.state.editingActionItems, true, "Cancel cannot abort a save in flight");

  saved.resolve();
  await saving;
  assert.equal(inputs[0].enhancedMarkdown, "## Action Items\n- Send recap\n- Book the large room\n");
  assert.equal(h.state.enhancedMd, inputs[0].enhancedMarkdown);
  assert.equal(h.state.editingActionItems, false);
  assert.match(h.state.actionItemStatus!, /Linked board cards/);
});

test("a failed action-item save keeps the drafts; unchanged drafts just close the editor", async () => {
  let fail = true;
  const h = harness({
    updateEnhancedActionItems: async () => { if (fail) throw new Error("board locked"); },
  });
  h.enhanced.set(1, "## Action Items\n- Send recap\n");
  h.open(1);
  await settle();
  h.view.startActionItemEdit();
  await h.view.saveActionItemEdits();
  assert.equal(h.state.editingActionItems, false, "nothing changed, nothing saved");

  h.view.startActionItemEdit();
  h.state.actionItemDrafts[0] = "Send the recap";
  await h.view.saveActionItemEdits();
  assert.match(h.state.actionItemError!, /board locked/);
  assert.equal(h.state.editingActionItems, true);
  assert.deepEqual(h.state.actionItemDrafts, ["Send the recap"]);
  assert.equal(h.state.enhancedMd, "## Action Items\n- Send recap\n");

  fail = false;
  await h.view.saveActionItemEdits();
  assert.equal(h.state.enhancedMd, "## Action Items\n- Send the recap\n");
});
