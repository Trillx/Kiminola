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
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
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
    enhanced_markdown: null,
    transcript: [],
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
