import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { isProviderConfigDirty, nextSettingsSection, resolveSettingsSection, settingsSectionHref, shouldUseFocusedSettingsShell, templateNeedsDeleteConfirmation } from "../src/lib/settings-ui.ts";

test("every settings section can be opened directly", () => {
  for (const section of ["general", "models", "ai", "shortcut", "templates", "about"] as const) {
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
  assert.equal(isProviderConfigDirty(saved, { ...saved }, false), false);
  assert.equal(isProviderConfigDirty(saved, { ...saved, model: "gpt-4.1-mini" }, false), true);
  assert.equal(isProviderConfigDirty(saved, { ...saved }, true), true);
  assert.equal(isProviderConfigDirty(null, { ...saved }, false), true);
});

test("only persisted custom templates require destructive confirmation", () => {
  assert.equal(templateNeedsDeleteConfirmation({ id: 10, is_builtin: 0 }), true);
  assert.equal(templateNeedsDeleteConfirmation({ id: -1, is_builtin: 0 }), false);
  assert.equal(templateNeedsDeleteConfirmation({ id: 11, is_builtin: 1 }), false);
});
