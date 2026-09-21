import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { parse, type AST } from "svelte/compiler";
// @ts-expect-error Node imports TypeScript directly.
import { createMeetingNotesAutosave, loadMeetingAfterAutosave } from "../src/lib/meeting-notes.ts";
// @ts-expect-error Node imports TypeScript directly.
import { resolveSettingsSection, settingsSectionHref } from "../src/lib/settings-ui.ts";

const component = readFileSync(new URL("../src/routes/meeting/[id]/+page.svelte", import.meta.url), "utf8");
function elementsIn(value: unknown): Array<AST.RegularElement | AST.Component> {
  if (!value || typeof value !== "object") return [];
  const { type } = value as { type?: string };
  const elements = type === "RegularElement" || type === "Component"
    ? [value as AST.RegularElement | AST.Component] : [];
  return [...elements, ...Object.values(value).flatMap(elementsIn)];
}
const elements = elementsIn(parse(component, { modern: true }).fragment);

function expressionSource(expression: object): string {
  // Svelte supplies offsets, but ESTree's public Expression type omits them.
  const span = expression as { start?: unknown; end?: unknown };
  assert.ok(typeof span.start === "number" && typeof span.end === "number", "Expected Svelte expression source offsets");
  return component.slice(span.start, span.end);
}

// Read the real component bindings, not a test-only copy of its event wiring.
function markupElement(className: string) {
  const element = elements.find((node) => node.attributes.some((attribute) =>
    attribute.type === "Attribute" && attribute.name === "class" &&
    Array.isArray(attribute.value) && attribute.value.some((part) => part.type === "Text" && part.data === className)));
  assert.ok(element, `Missing component element: ${className}`);
  return element;
}

function markupAttribute(className: string, name: string, evaluate: (expression: string) => unknown): unknown {
  const element = markupElement(className);
  const attribute = element.attributes.find((attribute) => attribute.type === "Attribute" && attribute.name === name);
  if (!attribute || attribute.type !== "Attribute") return undefined;
  if (attribute.value === true) return true;
  const parts = Array.isArray(attribute.value) ? attribute.value : [attribute.value];
  const values = parts.map((part) => part.type === "Text" ? part.data
    : evaluate(expressionSource(part.expression)));
  return values.length === 1 ? values[0] : values.join("");
}

function markupText(className: string, evaluate: (expression: string) => unknown): string {
  return markupElement(className).fragment.nodes.map((node) => {
    if (node.type === "Text") return node.data;
    if (node.type === "ExpressionTag") return evaluate(expressionSource(node.expression));
    return "";
  }).join("").trim();
}
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const meetingData = (id: number) => ({
  id, title: `Meeting ${id}`, notepad: `notes ${id}`, enhanced_markdown: null,
  transcript: [
    { id: id * 10, channel: "you", text: `Original ${id}` },
    { id: id * 10 + 1, channel: "others", text: `Other ${id}` },
  ],
});

function pageController(overrides: Record<string, unknown> = {}) {
  const effects: Array<() => void | (() => void)> = [];
  const destroyers: Array<() => void> = [];
  const timers = new Set<ReturnType<typeof setTimeout>>();
  const savers: Array<ReturnType<typeof createMeetingNotesAutosave>> = [];
  const observed: string[] = [];
  const errors: unknown[][] = [];
  let stored = "old notes";
  const route = { params: { id: "1" }, url: new URL("http://localhost/meeting/1") };
  const adapters = {
    $state: (value: unknown) => value,
    $derived: (value: unknown) => value,
    $effect: (effect: () => void | (() => void)) => effects.push(effect),
    page: route,
    console: { ...console, error: (...args: unknown[]) => { errors.push(args); } },
    getMeeting: async (id: number) => meetingData(id),
    getLlmConfig: async () => ({ model: "test", base_url: "https://example.invalid" }),
    listTemplates: async () => [{ id: 1, name: "General" }],
    renderMarkdown: (value: string) => value,
    onDestroy: (destroy: () => void) => { destroyers.push(destroy); },
    registerPendingSave: () => () => {},
    registerUpdateGuard: () => () => {},
    loadMeetingAfterAutosave,
    settingsSectionHref,
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
  const notesAutosave = createMeetingNotesAutosave(adapters.updateNotes, () => {}, 60_000);
  savers.push(notesAutosave);
  const exportSource = ts.transpileModule(readFileSync(new URL("../src/lib/meeting-export.ts", import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText.replace(/^import[\s\S]*?from\s+["\x27][^"\x27]+["\x27];\s*/gm, "").replace(/^export /gm, "");
  const exportAdapters = { ...adapters, meetingNotesAutosave: notesAutosave };
  const exportMeeting = new Function(...Object.keys(exportAdapters), exportSource + "\nreturn exportMeeting;")(...Object.values(exportAdapters));
  const pageAdapters = { ...adapters, notesAutosave, exportMeeting };
  // Run the actual page script and handlers with controlled IPC and rune adapters.
  // Scheduling effects explicitly lets the tests reproduce route and load ordering.
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText.replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "").replace(/^export \{\};?\s*$/gm, "");
  const api = new Function(...Object.keys(pageAdapters), `${source}\nreturn {
    edit(text) { notes = text; onNotesInput(); },
    runExport, runEnhancement,
    startSegmentEdit, cancelSegmentEdit, saveSegmentEdit, removeSegment, onSegmentKeydown,
    correctSegment(text) { editSegmentText = text; },
    evaluate(expression) {
      const line = meeting?.transcript.find((item) => item.id === editingSegmentId);
      return eval(expression);
    },
    state() { return { meeting, notes, notFound, enhancing, enhanceError, exportStatus, editingSegmentId, editSegmentText }; }
  };`)(...Object.values(pageAdapters));
  return {
    ...api, route, observed, errors,
    attribute: (className: string, name: string) => markupAttribute(className, name, api.evaluate),
    text: (className: string) => markupText(className, api.evaluate),
    dispatch: (className: string, event: string, argument?: unknown) => {
      if (markupAttribute(className, "disabled", api.evaluate)) return;
      const handler = markupAttribute(className, event, api.evaluate) as ((argument?: unknown) => unknown) | undefined;
      return handler?.(argument);
    },
    load: () => effects[0](),
    destroy: () => { destroyers.splice(0).forEach((destroy) => destroy()); },
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

test("transcript Cancel after textarea blur never persists the correction", async (t) => {
  const writes: Array<[number, string]> = [];
  const controller = pageController({ updateSegmentText: async (id: number, text: string) => { writes.push([id, text]); } });
  t.after(controller.dispose);
  controller.load();
  await tick();
  controller.startSegmentEdit(controller.state().meeting.transcript[0]);
  controller.correctSegment("Unwanted correction");
  // A pointer click transfers focus (blur) before firing the Cancel click.
  const blur = controller.dispatch("segment-edit-textarea", "onblur");
  controller.dispatch("segment-action", "onclick");
  await blur;
  assert.deepEqual(writes, []);
  assert.equal(controller.state().meeting.transcript[0].text, "Original 1");
  assert.equal(controller.state().editingSegmentId, undefined);
});

test("failed transcript save retains the correction and visible error until retry succeeds", async (t) => {
  let fail = true;
  const writes: Array<[number, string]> = [];
  const controller = pageController({ updateSegmentText: async (id: number, text: string) => {
    writes.push([id, text]);
    if (fail) throw new Error("disk full");
  } });
  t.after(controller.dispose);
  controller.load();
  await tick();
  controller.startSegmentEdit(controller.state().meeting.transcript[0]);
  controller.correctSegment("  Corrected sentence  ");
  controller.dispatch("segment-edit-textarea", "onkeydown", {
    key: "Enter", ctrlKey: true, preventDefault() {},
  });
  await tick();
  assert.equal(controller.state().editingSegmentId, 10);
  assert.equal(controller.state().editSegmentText, "  Corrected sentence  ");
  assert.equal(controller.state().meeting.transcript[0].text, "Original 1");
  assert.equal(controller.attribute("segment-edit-error", "role"), "alert");
  assert.match(controller.text("segment-edit-error"), /save|retry/i);
  assert.equal(controller.text("segment-action save"), "Retry save");
  assert.equal(controller.errors.length, 1);
  // Neither losing focus nor waiting should clear the correction or its error.
  await controller.dispatch("segment-edit-textarea", "onblur");
  assert.match(controller.text("segment-edit-error"), /save|retry/i);
  fail = false;
  await controller.dispatch("segment-action save", "onclick");
  assert.deepEqual(writes, [[10, "Corrected sentence"], [10, "Corrected sentence"]]);
  assert.equal(controller.state().meeting.transcript[0].text, "Corrected sentence");
  assert.equal(controller.state().editingSegmentId, undefined);
  assert.equal(controller.evaluate("segmentEditError"), null);
});

test("an in-flight transcript save blocks duplicate saves, deletion, Cancel, Escape, and editor switching", async (t) => {
  const saved = deferred<void>();
  const writes: Array<[number, string]> = [];
  const deletions: number[] = [];
  const controller = pageController({
    updateSegmentText: (id: number, text: string) => { writes.push([id, text]); return saved.promise; },
    deleteSegment: async (id: number) => { deletions.push(id); },
  });
  t.after(controller.dispose);
  controller.load();
  await tick();
  const [first, second] = controller.state().meeting.transcript;
  controller.startSegmentEdit(first);
  controller.correctSegment("Saved once");
  const saving = controller.dispatch("segment-action save", "onclick");
  // Exercise handler guards as well as the disabled UI; keyboard and stale events
  // must not bypass the same in-flight operation lock.
  void controller.saveSegmentEdit();
  await controller.removeSegment(first.id);
  controller.cancelSegmentEdit();
  controller.onSegmentKeydown({ key: "Escape", preventDefault() {} });
  controller.startSegmentEdit(second);
  assert.deepEqual(writes, [[10, "Saved once"]]);
  assert.deepEqual(deletions, []);
  assert.equal(controller.state().editingSegmentId, first.id);
  assert.equal(controller.state().editSegmentText, "Saved once");
  for (const className of ["segment-edit-textarea", "segment-action save", "segment-action", "segment-action delete", "raw-line"]) {
    assert.equal(controller.attribute(className, "disabled"), true, `${className} is disabled while saving`);
  }
  saved.resolve();
  await saving;
  assert.equal(controller.state().meeting.transcript[0].text, "Saved once");
  assert.equal(controller.state().meeting.transcript[1].text, "Other 1");
  assert.equal(controller.state().editingSegmentId, undefined);
  assert.equal(controller.attribute("raw-line", "disabled"), false);
});

test("an in-flight transcript delete blocks saves, duplicate deletion, cancellation, and editor switching", async (t) => {
  const deleted = deferred<void>();
  const writes: Array<[number, string]> = [];
  const deletions: number[] = [];
  const controller = pageController({
    updateSegmentText: async (id: number, text: string) => { writes.push([id, text]); },
    deleteSegment: (id: number) => { deletions.push(id); return deleted.promise; },
  });
  t.after(controller.dispose);
  controller.load();
  await tick();
  const [first, second] = controller.state().meeting.transcript;
  controller.startSegmentEdit(first);
  controller.correctSegment("Do not save this before deleting");
  controller.dispatch("segment-edit-textarea", "onblur");
  const deleting = controller.dispatch("segment-action delete", "onclick");
  void controller.removeSegment(first.id);
  void controller.saveSegmentEdit();
  controller.cancelSegmentEdit();
  controller.startSegmentEdit(second);
  assert.deepEqual(writes, []);
  assert.deepEqual(deletions, [10]);
  assert.equal(controller.state().editingSegmentId, first.id);
  assert.equal(controller.state().meeting.transcript.length, 2, "delete only changes the row after success");
  for (const className of ["segment-edit-textarea", "segment-action save", "segment-action", "segment-action delete", "raw-line"]) {
    assert.equal(controller.attribute(className, "disabled"), true, `${className} is disabled while deleting`);
  }
  deleted.resolve();
  await deleting;
  assert.deepEqual(controller.state().meeting.transcript, [second]);
  assert.equal(controller.state().editingSegmentId, undefined);
  assert.equal(controller.attribute("raw-line", "disabled"), false);
});

test("Cancel invalidates queued transcript Save and Delete actions", async (t) => {
  const writes: number[] = [];
  const deletions: number[] = [];
  const controller = pageController({
    updateSegmentText: async (id: number) => { writes.push(id); },
    deleteSegment: async (id: number) => { deletions.push(id); },
  });
  t.after(controller.dispose);
  controller.load();
  await tick();
  controller.startSegmentEdit(controller.state().meeting.transcript[0]);
  controller.correctSegment("Discard this correction");
  const queuedDelete = controller.attribute("segment-action delete", "onclick") as () => Promise<void>;
  controller.dispatch("segment-edit-textarea", "onkeydown", { key: "Escape", preventDefault() {} });
  await controller.saveSegmentEdit();
  await queuedDelete();
  assert.deepEqual(writes, []);
  assert.deepEqual(deletions, []);
  assert.equal(controller.state().meeting.transcript.length, 2);
  assert.equal(controller.state().editingSegmentId, undefined);
});

test("failed transcript deletion keeps the correction and offers a delete retry", async (t) => {
  let fail = true;
  const deletions: number[] = [];
  const controller = pageController({ deleteSegment: async (id: number) => {
    deletions.push(id);
    if (fail) throw new Error("disk unavailable");
  } });
  t.after(controller.dispose);
  controller.load();
  await tick();
  controller.startSegmentEdit(controller.state().meeting.transcript[0]);
  controller.correctSegment("Correction to keep after failed deletion");
  await controller.dispatch("segment-action delete", "onclick");
  assert.equal(controller.state().editingSegmentId, 10);
  assert.equal(controller.state().editSegmentText, "Correction to keep after failed deletion");
  assert.equal(controller.state().meeting.transcript.length, 2);
  assert.match(controller.text("segment-edit-error"), /could not delete/i);
  assert.equal(controller.errors.length, 1);
  assert.equal(controller.text("segment-action delete"), "Retry delete");
  assert.equal(controller.text("segment-action save"), "Save");
  fail = false;
  await controller.dispatch("segment-action delete", "onclick");
  assert.deepEqual(deletions, [10, 10]);
  assert.equal(controller.state().editingSegmentId, undefined);
  assert.equal(controller.state().meeting.transcript.length, 1);
  assert.equal(controller.evaluate("segmentEditError"), null);
});

for (const destination of [1, 2]) {
  for (const outcome of ["resolve", "reject"] as const) {
    test(`late transcript save ${outcome} cannot change a new editor or unlock its save on meeting ${destination}`, async (t) => {
      const firstSave = deferred<void>();
      const nextSave = deferred<void>();
      const writes: Array<[number, string]> = [];
      const controller = pageController({ updateSegmentText: (id: number, text: string) => {
        writes.push([id, text]);
        return writes.length === 1 ? firstSave.promise : nextSave.promise;
      } });
      t.after(controller.dispose);
      const cleanup = controller.load();
      await tick();
      controller.startSegmentEdit(controller.state().meeting.transcript[0]);
      controller.correctSegment("Old route correction");
      const first = controller.saveSegmentEdit();
      cleanup?.();
      controller.route.params.id = String(destination);
      controller.load();
      await tick();
      assert.equal(controller.state().editingSegmentId, undefined, "route load resets the old editor");
      controller.startSegmentEdit(controller.state().meeting.transcript[0]);
      controller.correctSegment("New route correction");
      const next = controller.saveSegmentEdit();
      assert.deepEqual(writes, [[10, "Old route correction"], [destination * 10, "New route correction"]]);
      if (outcome === "resolve") firstSave.resolve();
      else firstSave.reject(new Error("old route save failed"));
      await first;
      assert.equal(controller.state().meeting.id, destination);
      assert.equal(controller.state().meeting.transcript[0].text, `Original ${destination}`);
      assert.equal(controller.state().editingSegmentId, destination * 10);
      assert.equal(controller.state().editSegmentText, "New route correction");
      assert.equal(controller.evaluate("segmentEditError"), null);
      assert.equal(controller.attribute("segment-action", "disabled"), true, "old finally must not unlock a newer operation");
      nextSave.resolve();
      await next;
      assert.equal(controller.state().meeting.transcript[0].text, "New route correction");
      assert.equal(controller.state().editingSegmentId, undefined);
    });
  }
}

for (const destination of [1, 2]) {
  for (const outcome of ["resolve", "reject"] as const) {
    test(`late transcript delete ${outcome} cannot change a new editor or unlock its save on meeting ${destination}`, async (t) => {
      const deleted = deferred<void>();
      const saved = deferred<void>();
      const deletions: number[] = [];
      const controller = pageController({
        deleteSegment: (id: number) => { deletions.push(id); return deleted.promise; },
        updateSegmentText: () => saved.promise,
      });
      t.after(controller.dispose);
      const cleanup = controller.load();
      await tick();
      controller.startSegmentEdit(controller.state().meeting.transcript[0]);
      const deleting = controller.removeSegment(10);
      cleanup?.();
      controller.route.params.id = String(destination);
      controller.load();
      await tick();
      controller.startSegmentEdit(controller.state().meeting.transcript[0]);
      controller.correctSegment("New correction");
      const saving = controller.saveSegmentEdit();
      if (outcome === "resolve") deleted.resolve();
      else deleted.reject(new Error("old delete failed"));
      await deleting;
      assert.deepEqual(deletions, [10]);
      assert.equal(controller.state().meeting.transcript.length, 2);
      assert.equal(controller.state().editingSegmentId, destination * 10);
      assert.equal(controller.state().editSegmentText, "New correction");
      assert.equal(controller.evaluate("segmentEditError"), null);
      assert.equal(controller.attribute("segment-action", "disabled"), true);
      saved.resolve();
      await saving;
      assert.equal(controller.state().meeting.transcript[0].text, "New correction");
    });
  }
}

for (const action of ["save", "delete"] as const) {
  for (const outcome of ["resolve", "reject"] as const) {
    test(`late transcript ${action} ${outcome} does not mutate a destroyed page`, async (t) => {
      const persisted = deferred<void>();
      const controller = pageController({
        updateSegmentText: () => persisted.promise,
        deleteSegment: () => persisted.promise,
      });
      t.after(controller.dispose);
      controller.load();
      await tick();
      controller.startSegmentEdit(controller.state().meeting.transcript[0]);
      controller.correctSegment("Pending edit");
      const saving = action === "save" ? controller.saveSegmentEdit() : controller.removeSegment(10);
      controller.destroy();
      const before = structuredClone(controller.state());
      if (outcome === "resolve") persisted.resolve();
      else persisted.reject(new Error("destroyed page write failed"));
      await saving;
      assert.deepEqual(controller.state(), before);
      assert.equal(controller.evaluate("segmentEditError"), null);
    });
  }
}

test("Manage templates navigates directly to the Templates settings section", (t) => {
  const controller = pageController();
  t.after(controller.dispose);
  const href = controller.attribute("manage-templates", "href");
  assert.equal(href, "/settings?section=templates");
  const url = new URL(String(href), "http://localhost");
  assert.equal(resolveSettingsSection(url.searchParams.get("section")), "templates");
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
