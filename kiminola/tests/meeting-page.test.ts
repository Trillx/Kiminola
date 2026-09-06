import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
// @ts-expect-error Node imports TypeScript directly.
import { createMeetingNotesAutosave, loadMeetingAfterAutosave } from "../src/lib/meeting-notes.ts";

const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
const meetingData = (id: number) => ({ id, notepad: `notes ${id}`, enhanced_markdown: null });

function pageController(overrides: Record<string, unknown> = {}) {
  const effects: Array<() => void | (() => void)> = [];
  const timers = new Set<ReturnType<typeof setTimeout>>();
  const savers: Array<ReturnType<typeof createMeetingNotesAutosave>> = [];
  const observed: string[] = [];
  let stored = "old notes";
  const route = { params: { id: "1" }, url: new URL("http://localhost/meeting/1") };
  const adapters = {
    $state: (value: unknown) => value,
    $derived: (value: unknown) => value,
    $effect: (effect: () => void | (() => void)) => effects.push(effect),
    page: route,
    getMeeting: async (id: number) => meetingData(id),
    getLlmConfig: async () => ({ model: "test", base_url: "https://example.invalid" }),
    listTemplates: async () => [{ id: 1, name: "General" }],
    renderMarkdown: (value: string) => value,
    onDestroy: () => {},
    registerPendingSave: () => () => {},
    registerUpdateGuard: () => () => {},
    loadMeetingAfterAutosave,
    createMeetingNotesAutosave: (save: (id: number, text: string) => Promise<void>, status: (value: string) => void) => {
      const saver = createMeetingNotesAutosave(save, status, 60_000);
      savers.push(saver);
      return saver;
    },
    updateNotes: async (_id: number, text: string) => { stored = text; },
    enhanceMeeting: async () => { observed.push(stored); },
    onLlmChunk: async () => () => {},
    onLlmDone: async () => () => {},
    onLlmError: async () => () => {},
    exportNotesMarkdown: async () => { observed.push(stored); return stored; },
    saveNotesExport: async () => { observed.push(stored); return "notes.md"; },
    navigator: { clipboard: { writeText: async () => {} } },
    setTimeout: (fn: () => void, ms: number) => {
      const timer = setTimeout(fn, ms); timers.add(timer); return timer;
    },
    clearTimeout,
    ...overrides,
  };
  // Run the actual page script and handlers with controlled IPC and rune adapters.
  // Scheduling effects explicitly lets the tests reproduce route and load ordering.
  const component = readFileSync(new URL("../src/routes/meeting/[id]/+page.svelte", import.meta.url), "utf8");
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText.replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "").replace(/^export \{\};?\s*$/gm, "");
  const api = new Function(...Object.keys(adapters), `${source}\nreturn {
    edit(text) { notes = text; onNotesInput(); },
    runExport, runEnhancement,
    state() { return { meeting, notes, notFound, enhancing, enhanceError, exportStatus }; }
  };`)(...Object.values(adapters));
  return {
    ...api, route, observed,
    load: () => effects[0](),
    dispose: () => { savers.forEach((saver) => { void saver.close().catch(() => {}); }); timers.forEach(clearTimeout); },
  };
}

for (const action of ["copy-notes", "save-notes", "enhance"]) {
  test(`${action} includes notes typed immediately before the action`, async (t) => {
    const controller = pageController();
    t.after(controller.dispose);
    controller.load();
    await tick();
    controller.edit("latest edit");
    if (action === "enhance") await controller.runEnhancement();
    else await controller.runExport(action);
    assert.deepEqual(controller.observed, ["latest edit"]);
  });
}

test("failed note persistence prevents export and reports the error", async (t) => {
  const controller = pageController({ updateNotes: async () => { throw new Error("disk full"); } });
  t.after(controller.dispose);
  controller.load();
  await tick();
  controller.edit("unsaved edit");
  await controller.runExport("save-notes");
  assert.deepEqual(controller.observed, []);
  assert.match(controller.state().exportStatus, /disk full/);
});

test("failed note persistence prevents enhancement and allows retry", async (t) => {
  let fail = true;
  const controller = pageController({ updateNotes: async () => {
    if (fail) throw new Error("disk full");
  } });
  t.after(controller.dispose);
  controller.load();
  await tick();
  controller.edit("unsaved edit");
  await controller.runEnhancement();
  assert.deepEqual(controller.observed, []);
  assert.equal(controller.state().enhancing, false);
  assert.match(controller.state().enhanceError, /disk full/);
  fail = false;
  await controller.runEnhancement();
  assert.equal(controller.observed.length, 1);
  assert.equal(controller.state().enhanceError, null);
});

test("export waits for an in-flight note write to finish", async (t) => {
  const saved = deferred<void>();
  const controller = pageController({ updateNotes: () => saved.promise });
  t.after(controller.dispose);
  controller.load();
  await tick();
  controller.edit("pending write");
  const exporting = controller.runExport("save-notes");
  await tick();
  assert.equal(controller.observed.length, 0);
  saved.resolve();
  await exporting;
  assert.equal(controller.observed.length, 1);
});

test("automatic enhancement waits for provider configuration", async (t) => {
  const config = deferred<unknown>();
  const controller = pageController({ getLlmConfig: () => config.promise });
  t.after(controller.dispose);
  controller.route.url.searchParams.set("mode", "enhance");
  controller.load();
  await tick();
  assert.equal(controller.observed.length, 0);
  config.resolve({ model: "test", base_url: "https://example.invalid" });
  await tick();
  assert.equal(controller.observed.length, 1);
});

test("a late response for the previous meeting cannot replace the current editor", async (t) => {
  const first = deferred<unknown>();
  const controller = pageController({ getMeeting: (id: number) => id === 1 ? first.promise : Promise.resolve(meetingData(id)) });
  t.after(controller.dispose);
  const cleanup = controller.load();
  cleanup?.();
  controller.route.params.id = "2";
  controller.load();
  await tick();
  assert.equal(controller.state().meeting.id, 2);
  first.resolve(meetingData(1));
  await tick();
  assert.equal(controller.state().meeting.id, 2);
  assert.equal(controller.state().notes, "notes 2");
});

test("late configuration cannot trigger enhancement for an abandoned route", async (t) => {
  const config = deferred<unknown>();
  const controller = pageController({ getLlmConfig: () => config.promise });
  t.after(controller.dispose);
  controller.route.url.searchParams.set("mode", "enhance");
  const cleanup = controller.load();
  await tick();
  cleanup?.();
  controller.route.params.id = "2";
  controller.route.url.searchParams.delete("mode");
  controller.load();
  config.resolve({ model: "test", base_url: "https://example.invalid" });
  await tick();
  assert.equal(controller.state().meeting.id, 2);
  assert.equal(controller.observed.length, 0);
});
