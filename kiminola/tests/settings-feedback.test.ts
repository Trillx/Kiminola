import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
// @ts-expect-error Node's strip-types runner imports the TypeScript source directly.
import { resolveSettingsSection } from "../src/lib/settings-ui.ts";

function controller(route: string, adapters: Record<string, unknown>, expose: string) {
  const component = readFileSync(new URL(`../src/routes/${route}/+page.svelte`, import.meta.url), "utf8");
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText
    .replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "").replace(/^export \{\};?\s*$/gm, "");
  const scope = {
    $state: (value: unknown) => value,
    $derived: (value: unknown) => value,
    $effect: () => {},
    onMount: () => {},
    onDestroy: () => {},
    beforeNavigate: () => {},
    page: { url: new URL("http://localhost/settings?section=shortcut") },
    resolveSettingsSection,
    console: { error: () => {} },
    setTimeout: () => 0,
    clearTimeout: () => {},
    ...adapters,
  };
  return new Function(...Object.keys(scope), source + "\nreturn " + expose)(...Object.values(scope));
}

test("shortcut save rejection stays visible until a successful retry", async () => {
  let fail = true;
  const ui = controller("settings", { setGlobalShortcut: async () => {
    if (fail) throw new Error("Invalid accelerator");
  } }, `{ saveShortcut, state() { return { savingShortcut, shortcutSaved, error: typeof shortcutError === "undefined" ? "" : shortcutError }; } }`);
  await ui.saveShortcut();
  assert.match(ui.state().error, /Invalid accelerator/);
  assert.equal(ui.state().savingShortcut, false);
  assert.equal(ui.state().shortcutSaved, false);
  fail = false;
  await ui.saveShortcut();
  assert.equal(ui.state().error, "");
  assert.equal(ui.state().shortcutSaved, true);
});

test("dismissing the discard dialog clears abandoned template and section actions", () => {
  const ui = controller("settings", {}, `{
    onDiscardDialogOpenChange,
    setPending(template, section) {
      pendingTemplate = template;
      pendingSection = section;
      discardConfirmOpen = true;
    },
    state() { return { discardConfirmOpen, pendingTemplate, pendingSection }; }
  }`);
  ui.setPending({ id: 9, name: "Abandoned", prompt: "", is_builtin: 0 }, "general");

  ui.onDiscardDialogOpenChange(false);

  assert.deepEqual(ui.state(), {
    discardConfirmOpen: false,
    pendingTemplate: null,
    pendingSection: null,
  });
});

test("template placeholder controls copy the exact token and confirm it", async () => {
  const copied: string[] = [];
  const ui = controller("settings", {
    navigator: { clipboard: { writeText: async (text: string) => { copied.push(text); } } },
  }, `{
    copyPlaceholder,
    state() { return templateStatus; }
  }`);

  await ui.copyPlaceholder("{transcript}");
  assert.deepEqual(ui.state(), { message: "{transcript} copied to clipboard.", error: false });

  await ui.copyPlaceholder("{notes}");

  assert.deepEqual(copied, ["{transcript}", "{notes}"]);
  assert.deepEqual(ui.state(), { message: "{notes} copied to clipboard.", error: false });
});

test("microphone permission success does not invent an audio-level test", async () => {
  let intervals = 0;
  const ui = controller("onboarding", {
    checkMicrophonePermission: async () => "Granted",
    setInterval: () => { intervals++; return 1; },
    clearInterval: () => {},
  }, `{ requestMic, state() { return micState; } }`);
  await ui.requestMic();
  assert.equal(ui.state(), "granted");
  assert.equal(intervals, 0);
});

test("onboarding clears a typed credential when changing providers", () => {
  const ui = controller("onboarding", {}, `{ applyProviderPreset, setKey(value) { apiKey = value; }, key() { return apiKey; } }`);
  ui.setKey("synthetic-test-input");
  ui.applyProviderPreset("ollama");
  assert.equal(ui.key(), "");
});

test("speech-model progress uses a correctly encoded ellipsis", () => {
  const component = readFileSync(new URL("../src/routes/settings/+page.svelte", import.meta.url), "utf8");
  assert.ok(component.includes("Checking the model pack…"));
  assert.ok(component.includes("Downloading and verifying…"));
});
