import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { isProviderConfigDirty, nextSettingsSection, openRouterModelOptionLabel, providerIsConfigured, resolveSettingsSection, SETTINGS_SECTIONS, settingsSectionHref, shouldUseFocusedSettingsShell, templateNeedsDeleteConfirmation, uniqueOpenRouterModels } from "../src/lib/settings-ui.ts";

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

test("only persisted custom templates require destructive confirmation", () => {
  assert.equal(templateNeedsDeleteConfirmation({ id: 10, is_builtin: 0 }), true);
  assert.equal(templateNeedsDeleteConfirmation({ id: -1, is_builtin: 0 }), false);
  assert.equal(templateNeedsDeleteConfirmation({ id: 11, is_builtin: 1 }), false);
});
