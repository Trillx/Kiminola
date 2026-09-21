import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
// @ts-expect-error Node imports TypeScript directly.
import * as settingsUi from "../src/lib/settings-ui.ts";

const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
const initialConfig = () => ({
  kind: "open_ai", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini", has_api_key: true,
});
const component = readFileSync(new URL("../src/lib/components/ProviderConfigForm.svelte", import.meta.url), "utf8");

function providerForm(overrides: Record<string, unknown> = {}) {
  const effects: Array<() => void> = [];
  const destroy: Array<() => void> = [];
  const writes: Array<{ config: ReturnType<typeof initialConfig>; apiKey: string | undefined }> = [];
  let persisted = initialConfig();
  const adapters = {
    ...settingsUi,
    $state: (value: unknown) => value,
    $derived: (value: unknown) => value,
    $props: () => ({}),
    $effect: (effect: () => void) => effects.push(effect),
    onDestroy: (fn: () => void) => destroy.push(fn),
    getLlmConfig: async () => ({ ...persisted }),
    setLlmConfig: async (config: ReturnType<typeof initialConfig>, apiKey: string | undefined) => {
      writes.push({ config: { ...config }, apiKey });
      persisted = { ...config, has_api_key: Boolean(apiKey) || config.has_api_key };
    },
    testLlmConfig: async () => {},
    setTimeout: () => 0,
    clearTimeout: () => {},
    console: { error: () => {} },
    ...overrides,
  };
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText.replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "").replace(/^export \{\};?\s*$/gm, "");
  const api = new Function(...Object.keys(adapters), `${source}\nreturn {
    setProviderDefaults, save,
    replaceKey(value) { apiKey = value; },
    editBaseUrl(value) { ${component.includes('bind:value={config.base_url}') ? 'config.base_url = value;' : 'setBaseUrl(value);'} },
    editModel(value) { config.model = value; },
    retry() { return loadProviderConfig(); },
    state() { return { config, savedConfig, apiKey, loaded, saving, testOutput, saveError, loadError: typeof loadError === 'undefined' ? '' : loadError }; }
  };`)(...Object.values(adapters));
  return { ...api, writes, load: () => effects[0](), dispose: () => destroy.forEach((fn) => fn()) };
}

test("switching provider clears both saved-key status and an unsaved replacement", async (t) => {
  const form = providerForm();
  t.after(form.dispose);
  form.load();
  await tick();
  form.replaceKey("synthetic-openai-key");
  form.setProviderDefaults("open_router");
  assert.equal(form.state().config.has_api_key, false);
  assert.equal(form.state().apiKey, "");
  await form.save();
  assert.equal(form.writes[0].apiKey, undefined);
  assert.equal(form.writes[0].config.kind, "open_router");
});

test("changing endpoint clears saved-key status and typed replacement before saving", async (t) => {
  const form = providerForm();
  t.after(form.dispose);
  form.load();
  await tick();
  form.replaceKey("synthetic-old-endpoint-key");
  form.editBaseUrl("https://other.example.invalid/v1");
  assert.equal(form.state().config.has_api_key, false);
  assert.equal(form.state().apiKey, "");
  await form.save();
  assert.equal(form.writes[0].apiKey, undefined);
});

test("returning to the saved identity restores only its known key status", async (t) => {
  const form = providerForm();
  t.after(form.dispose);
  form.load();
  await tick();
  form.editBaseUrl("https://other.example.invalid/v1");
  assert.equal(form.state().config.has_api_key, false);
  form.replaceKey("synthetic-other-endpoint-key");
  form.editBaseUrl(initialConfig().base_url);
  assert.equal(form.state().config.has_api_key, true);
  assert.equal(form.state().apiKey, "");
  form.setProviderDefaults("open_router");
  assert.equal(form.state().config.has_api_key, false);
  form.setProviderDefaults("open_ai");
  assert.equal(form.state().config.has_api_key, true);
});

test("model-only edits keep the key attached to the unchanged identity", async (t) => {
  const form = providerForm();
  t.after(form.dispose);
  form.load();
  await tick();
  form.replaceKey("synthetic-same-endpoint-key");
  form.editModel("another-model");
  assert.equal(form.state().config.has_api_key, true);
  assert.equal(form.state().apiKey, "synthetic-same-endpoint-key");
});

test("a failed provider load has an actionable error and retry recovers the form", async (t) => {
  let attempts = 0;
  const form = providerForm({ getLlmConfig: async () => {
    if (++attempts === 1) throw new Error("synthetic credential service unavailable");
    return initialConfig();
  } });
  t.after(form.dispose);
  form.load();
  await tick();
  assert.equal(form.state().loaded, true);
  assert.equal(form.state().config, null);
  assert.match(form.state().loadError, /could not load provider settings/i);
  assert.match(component, /\{:else if loadError\}[\s\S]*?role="alert"[\s\S]*?onclick=\{[^}]*loadProviderConfig[^}]*\}[^>]*>Retry/);
  await form.retry();
  assert.equal(attempts, 2);
  assert.equal(form.state().loadError, "");
  assert.deepEqual(form.state().config, initialConfig());
});

test("saving refreshes authoritative key presence for the destination", async (t) => {
  let reads = 0;
  const form = providerForm({ getLlmConfig: async () => ++reads === 1 ? initialConfig() : {
    kind: "ollama", base_url: "http://localhost:11434/v1", model: "llama3.1", has_api_key: false,
  } });
  t.after(form.dispose);
  form.load();
  await tick();
  form.setProviderDefaults("ollama");
  await form.save();
  assert.equal(reads, 2, "key presence must come from a backend readback, not a UI guess");
  assert.equal(form.state().savedConfig.has_api_key, false);
});

test("provider and endpoint edits cannot retarget an in-flight credential write", async (t) => {
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  const form = providerForm({ setLlmConfig: () => pending });
  t.after(form.dispose);
  form.load();
  await tick();
  form.replaceKey("synthetic-bound-key");
  const saving = form.save();
  form.setProviderDefaults("open_router");
  form.editBaseUrl("https://other.example.invalid/v1");
  assert.equal(form.state().config.kind, "open_ai");
  assert.equal(form.state().config.base_url, initialConfig().base_url);
  finish();
  await saving;
});

test("a saved configuration with failed readback offers reload without sending a test", async (t) => {
  let reads = 0;
  let tests = 0;
  const form = providerForm({
    getLlmConfig: async () => {
      if (++reads === 2) throw new Error("synthetic readback failure");
      return initialConfig();
    },
    testLlmConfig: async () => { tests++; },
  });
  t.after(form.dispose);
  form.load();
  await tick();
  form.replaceKey("synthetic-replacement-key");
  await form.save(true);
  assert.equal(tests, 0);
  assert.equal(form.state().apiKey, "");
  assert.match(form.state().loadError, /provider saved.*retry/i);
  await form.retry();
  assert.deepEqual(form.state().config, initialConfig());
  assert.equal(form.state().loadError, "");
  assert.equal(tests, 0);
});

test("late configuration loading does not repopulate a disposed form", async () => {
  let finish!: (value: ReturnType<typeof initialConfig>) => void;
  const pending = new Promise<ReturnType<typeof initialConfig>>((resolve) => { finish = resolve; });
  const form = providerForm({ getLlmConfig: () => pending });
  form.load();
  form.dispose();
  finish(initialConfig());
  await tick();
  assert.equal(form.state().config, null);
});
