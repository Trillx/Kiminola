import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { isProviderConfigDirty, nextSettingsSection, openRouterModelOptionLabel, providerIdentityChanged, providerIsConfigured, resolveSettingsSection, SETTINGS_SECTIONS, settingsSectionHref, shouldUseFocusedSettingsShell, templateNeedsDeleteConfirmation, uniqueOpenRouterModels } from "../src/lib/settings-ui.ts";

type OpenRouterModel = {
  id: string;
  name: string;
  context_length: number | null;
};

function providerConfigController(
  listOpenRouterModels: () => Promise<OpenRouterModel[]>,
) {
  const effects: Array<() => void> = [];
  const destroyers: Array<() => void> = [];
  const config = {
    kind: "open_router",
    base_url: "https://openrouter.ai/api/v1",
    model: "manual/model-id",
    has_api_key: true,
  };
  const adapters = {
    $props: () => ({}),
    $state: <T>(value: T) => value,
    $derived: <T>(value: T) => value,
    $effect: (effect: () => void) => effects.push(effect),
    getLlmConfig: async () => config,
    listOpenRouterModels,
    setLlmConfig: async () => {},
    testLlmConfig: async () => {},
    onDestroy: (destroy: () => void) => destroyers.push(destroy),
    tick: async () => {},
    isProviderConfigDirty,
    providerIdentityChanged,
    providerIsConfigured,
    uniqueOpenRouterModels,
  };
  const component = readFileSync(
    new URL("../src/lib/components/ProviderConfigForm.svelte", import.meta.url),
    "utf8",
  );
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText
    .replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "")
    .replace(/^export \{\};?\s*$/gm, "");
  const api = new Function(...Object.keys(adapters), `${source}\nreturn {
    refreshOpenRouterModels,
    selectOpenRouterModel,
    setProviderDefaults,
    state() { return { config, openRouterModels, modelListState, modelListError, modelPickerOpen }; }
  };`)(...Object.values(adapters));

  return {
    ...api,
    load: effects[0],
    dispose: () => destroyers.forEach((destroy) => destroy()),
  };
}

const flushPromises = () => new Promise<void>((resolve) => setImmediate(resolve));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

test("every settings section can be opened directly", () => {
  assert.deepEqual(SETTINGS_SECTIONS.map((section) => section.label), ["General", "Speech model", "AI provider", "Dictation", "Shortcut", "Templates", "About"]);
  for (const section of ["general", "models", "ai", "dictation", "shortcut", "templates", "about"] as const) {
    assert.equal(resolveSettingsSection(section), section);
  }
  assert.equal(resolveSettingsSection(null), "general");
  assert.equal(resolveSettingsSection("unknown"), "general");
});

test("settings section links preserve the requested section", () => {
  assert.equal(settingsSectionHref("general"), "/settings");
  assert.equal(settingsSectionHref("models"), "/settings?section=models");
  assert.equal(settingsSectionHref("templates"), "/settings?section=templates");
});

test("settings tabs support arrow, home, and end navigation", () => {
  assert.equal(nextSettingsSection("general", "ArrowRight"), "models");
  assert.equal(nextSettingsSection("about", "ArrowRight"), "general");
  assert.equal(nextSettingsSection("general", "ArrowLeft"), "about");
  assert.equal(nextSettingsSection("templates", "Home"), "general");
  assert.equal(nextSettingsSection("general", "End"), "about");
  assert.equal(nextSettingsSection("ai", "Enter"), null);
});

test("only the settings route uses the focused settings shell", () => {
  assert.equal(shouldUseFocusedSettingsShell("/settings"), true);
  assert.equal(shouldUseFocusedSettingsShell("/settings/"), true);
  assert.equal(shouldUseFocusedSettingsShell("/meeting/12"), false);
});

test("provider save state tracks config and API key edits", () => {
  const saved = {
    kind: "open_ai" as const,
    base_url: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
  };
  assert.equal(isProviderConfigDirty(saved, { ...saved }, ""), false);
  assert.equal(isProviderConfigDirty(saved, { ...saved, model: "gpt-4.1-mini" }, ""), true);
  assert.equal(isProviderConfigDirty(saved, { ...saved }, "new-secret"), true);
  assert.equal(isProviderConfigDirty(saved, { ...saved }, "   "), false);
  assert.equal(isProviderConfigDirty(null, { ...saved }, ""), true);
});

test("cloud providers require a stored key while local providers do not", () => {
  const base = { base_url: "https://api.openai.com/v1", model: "gpt-4o-mini" };
  assert.equal(providerIsConfigured({ ...base, kind: "open_ai", has_api_key: false }), false);
  assert.equal(providerIsConfigured({ ...base, kind: "open_ai", has_api_key: true }), true);
  assert.equal(providerIsConfigured({ ...base, kind: "ollama", has_api_key: false }), true);
  assert.equal(providerIsConfigured({ ...base, kind: "lm_studio" }), true);
});

test("OpenRouter model options include a compact context size", () => {
  assert.equal(
    openRouterModelOptionLabel({
      id: "anthropic/claude-3.5-sonnet",
      name: "Anthropic: Claude 3.5 Sonnet",
      context_length: 200_000,
    }),
    "Anthropic: Claude 3.5 Sonnet · 200K context",
  );
  assert.equal(
    openRouterModelOptionLabel({
      id: "openrouter/auto",
      name: "Auto Router",
      context_length: null,
    }),
    "Auto Router",
  );
});

test("OpenRouter model options discard duplicate IDs", () => {
  const first = {
    id: "openai/gpt-4o",
    name: "OpenAI: GPT-4o",
    context_length: 128_000,
  };
  const second = {
    id: "anthropic/claude-sonnet-4",
    name: "Anthropic: Claude Sonnet 4",
    context_length: 200_000,
  };

  assert.deepEqual(
    uniqueOpenRouterModels([
      first,
      second,
      { ...first, name: "Duplicate GPT-4o" },
    ]),
    [first, second],
  );
});

test("OpenRouter model refresh exposes a successful catalog response", async (t) => {
  const response = deferred<OpenRouterModel[]>();
  const controller = providerConfigController(() => response.promise);
  t.after(controller.dispose);
  controller.load();
  await flushPromises();

  const refreshing = controller.refreshOpenRouterModels();
  assert.equal(controller.state().modelListState, "loading");
  response.resolve([
    { id: "openai/gpt-4o", name: "OpenAI: GPT-4o", context_length: 128_000 },
    { id: "openai/gpt-4o", name: "Duplicate", context_length: 128_000 },
  ]);
  await refreshing;

  assert.deepEqual(controller.state().openRouterModels.map((model: OpenRouterModel) => model.id), ["openai/gpt-4o"]);
  assert.equal(controller.state().modelListState, "ready");
  assert.equal(controller.state().modelPickerOpen, true);
});

test("OpenRouter model refresh preserves manual entry after a catalog error", async (t) => {
  const controller = providerConfigController(async () => {
    throw new Error("catalog unavailable");
  });
  t.after(controller.dispose);
  controller.load();
  await flushPromises();

  await controller.refreshOpenRouterModels();

  assert.equal(controller.state().config.model, "manual/model-id");
  assert.equal(controller.state().modelListState, "error");
  assert.match(controller.state().modelListError, /catalog unavailable/);
  assert.equal(controller.state().modelPickerOpen, false);
});

test("selecting an OpenRouter catalog model updates the editable model ID", async (t) => {
  const controller = providerConfigController(async () => []);
  t.after(controller.dispose);
  controller.load();
  await flushPromises();

  controller.selectOpenRouterModel({
    id: "anthropic/claude-sonnet-4",
    name: "Anthropic: Claude Sonnet 4",
    context_length: 200_000,
  });

  assert.equal(controller.state().config.model, "anthropic/claude-sonnet-4");
  assert.equal(controller.state().modelPickerOpen, false);
});

test("a stale OpenRouter catalog response cannot repopulate another provider", async (t) => {
  const response = deferred<OpenRouterModel[]>();
  const controller = providerConfigController(() => response.promise);
  t.after(controller.dispose);
  controller.load();
  await flushPromises();

  const refreshing = controller.refreshOpenRouterModels();
  controller.setProviderDefaults("open_ai");
  response.resolve([
    { id: "openai/gpt-4o", name: "OpenAI: GPT-4o", context_length: 128_000 },
  ]);
  await refreshing;

  assert.equal(controller.state().config.kind, "open_ai");
  assert.deepEqual(controller.state().openRouterModels, []);
  assert.equal(controller.state().modelListState, "idle");
});

test("only persisted custom templates require destructive confirmation", () => {
  assert.equal(templateNeedsDeleteConfirmation({ id: 10, is_builtin: 0 }), true);
  assert.equal(templateNeedsDeleteConfirmation({ id: -1, is_builtin: 0 }), false);
  assert.equal(templateNeedsDeleteConfirmation({ id: 11, is_builtin: 1 }), false);
});
