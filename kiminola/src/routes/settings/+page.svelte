<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import ProviderConfigForm from "$lib/components/ProviderConfigForm.svelte";
  import { themeState, toggleTheme } from "$lib/theme.svelte";
  import {
    getGlobalShortcut,
    setGlobalShortcut,
    listTemplates,
    createTemplate,
    updateTemplate,
    deleteTemplate,
    getMeetingPresenceState,
    onMeetingPresenceState,
    setMeetingPresenceEnabled,
    setMeetingPresencePaused,
    setMeetingPresenceStartWithWindows,
    type MeetingPresenceState,
    type Template,
  } from "$lib/tauri";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Textarea } from "$lib/components/ui/textarea";
  import { Label } from "$lib/components/ui/label";
  import * as Select from "$lib/components/ui/select";

  type Section = "general" | "ai" | "shortcut" | "about" | "templates";

  const SECTIONS: { id: Section; label: string }[] = [
    { id: "general", label: "General" },
    { id: "ai", label: "AI provider" },
    { id: "shortcut", label: "Shortcut" },
    { id: "templates", label: "Templates" },
    { id: "about", label: "About" },
  ];

  let active = $state<Section>("general");
  let shortcut = $state("");
  let savingShortcut = $state(false);
  let shortcutSaved = $state(false);
  let appVersion = $state("");
  let presence = $state<MeetingPresenceState>({
    enabled: false,
    paused: false,
    start_with_windows: false,
    mode: "off",
    hint: null,
    prompt: null,
  });
  let presenceError = $state("");

  // Templates state
  let templates = $state<Template[]>([]);
  let selectedTemplateId = $state<number | undefined>(undefined);
  let editingName = $state("");
  let editingPrompt = $state("");
  let templatesLoading = $state(false);
  let templateStatus = $state<{ message: string; error: boolean } | null>(null);
  let templateStatusTimer: ReturnType<typeof setTimeout> | undefined;
  const selectedTemplate = $derived(templates.find((t) => t.id === selectedTemplateId));

  function flashTemplateStatus(message: string, error = false) {
    templateStatus = { message, error };
    clearTimeout(templateStatusTimer);
    templateStatusTimer = setTimeout(() => {
      templateStatus = null;
    }, 4000);
  }

  function validateTemplatePrompt(prompt: string): string | null {
    if (!prompt.includes("{transcript}")) return "Prompt must contain {transcript}";
    if (!prompt.includes("{notes}")) return "Prompt must contain {notes}";
    return null;
  }

  async function loadTemplates() {
    templatesLoading = true;
    try {
      templates = await listTemplates();
    } catch (err) {
      flashTemplateStatus(String(err), true);
    } finally {
      templatesLoading = false;
    }
  }

  function selectTemplate(t: Template) {
    selectedTemplateId = t.id;
    editingName = t.name;
    editingPrompt = t.prompt;
  }

  function newTemplate() {
    const defaultPrompt = "Summarize the meeting.\n\n## Summary\n## Action items\n\nTranscript:\n{transcript}\n\nRaw notes:\n{notes}";
    selectTemplate({ id: -1, name: "New template", prompt: defaultPrompt, is_builtin: 0 } as Template);
  }

  async function saveTemplate() {
    if (selectedTemplateId === undefined) return;
    const promptErr = validateTemplatePrompt(editingPrompt);
    if (promptErr) {
      flashTemplateStatus(promptErr, true);
      return;
    }
    const name = editingName.trim();
    if (!name) {
      flashTemplateStatus("Template name is required", true);
      return;
    }
    try {
      if (selectedTemplateId === -1) {
        const t = await createTemplate(name, editingPrompt);
        templates = [...templates, t];
        selectTemplate(t);
      } else {
        await updateTemplate(selectedTemplateId, name, editingPrompt);
        templates = templates.map((t) =>
          t.id === selectedTemplateId ? { ...t, name, prompt: editingPrompt } : t,
        );
      }
      flashTemplateStatus("Template saved.");
    } catch (err) {
      flashTemplateStatus(String(err), true);
    }
  }

  async function deleteSelectedTemplate() {
    if (selectedTemplateId === undefined || selectedTemplateId === -1) return;
    if (selectedTemplate?.is_builtin) {
      flashTemplateStatus("Built-in templates cannot be deleted", true);
      return;
    }
    try {
      await deleteTemplate(selectedTemplateId);
      templates = templates.filter((t) => t.id !== selectedTemplateId);
      selectedTemplateId = undefined;
      editingName = "";
      editingPrompt = "";
      flashTemplateStatus("Template deleted.");
    } catch (err) {
      flashTemplateStatus(String(err), true);
    }
  }

  function activateTemplates() {
    active = "templates";
    loadTemplates();
  }

  onMount(() => {
    getGlobalShortcut()
      .then((s) => {
        shortcut = s ?? "";
      })
      .catch((err) => console.error("Failed to load shortcut:", err));

    getVersion()
      .then((v) => (appVersion = v))
      .catch((err) => console.error("Failed to load version:", err));

    let unlisten: (() => void) | undefined;
    onMeetingPresenceState((next) => (presence = next)).then((fn) => (unlisten = fn));
    getMeetingPresenceState()
      .then((next) => (presence = next))
      .catch((err) => console.error("Failed to load meeting presence settings:", err));
    return () => unlisten?.();
  });

  function presenceLabel(): string {
    if (presence.mode === "detecting") return "Detecting locally · not recording";
    if (presence.mode === "paused") return "Paused";
    return "Off";
  }

  async function changePresence(setting: "enabled" | "paused" | "startup", value: boolean) {
    presenceError = "";
    try {
      if (setting === "enabled") await setMeetingPresenceEnabled(value);
      if (setting === "paused") await setMeetingPresencePaused(value);
      if (setting === "startup") await setMeetingPresenceStartWithWindows(value);
      presence = await getMeetingPresenceState();
    } catch (err) {
      presenceError = String(err);
      try {
        presence = await getMeetingPresenceState();
      } catch {
        // Keep the last visible state if the refresh also fails.
      }
    }
  }

  async function saveShortcut() {
    savingShortcut = true;
    shortcutSaved = false;
    try {
      await setGlobalShortcut(shortcut.trim() || null);
      shortcutSaved = true;
      setTimeout(() => (shortcutSaved = false), 3000);
    } catch (err) {
      console.error("Failed to save shortcut:", err);
    } finally {
      savingShortcut = false;
    }
  }
</script>

<svelte:head>
  <title>Settings — Kimi Nola</title>
</svelte:head>

<div class="main-content">
  <div class="post-shell settings-layout">
    <aside class="settings-rail">
      <div class="display" style="margin-bottom: 16px; font-size: 24px;">Settings</div>
      <div class="settings-nav" role="tablist">
        {#each SECTIONS as section}
          <button
            class="settings-nav-item"
            class:active={active === section.id}
            role="tab"
            aria-selected={active === section.id}
            onclick={() => section.id === "templates" ? activateTemplates() : (active = section.id)}
          >
            {section.label}
          </button>
        {/each}
      </div>
      <a class="btn btn-ghost btn-sm" href="/" style="margin-top: auto;">← Back to meetings</a>
    </aside>

    <div class="settings-pane">
      {#if active === "general"}
        <div class="my-notes-card provider-config">
          <div class="enhance-title">General</div>
          <div class="enhance-copy">Choose how Kimi Nola looks on your machine.</div>
          <div class="field inline">
            <span>Theme</span>
            <button class="btn btn-ghost" onclick={toggleTheme}>
              {themeState.theme === "dark" ? "☀️ Switch to light" : "🌙 Switch to dark"}
            </button>
          </div>
        </div>
        <div class="my-notes-card provider-config presence-settings">
          <div class="enhance-title">Meeting prompts</div>
          <div class="enhance-copy">
            Kimi Nola can look for meeting apps locally and ask before it does anything. It never
            starts recording automatically.
          </div>
          <div class="field inline">
            <span>Meeting detection</span>
            <button class="btn btn-ghost" onclick={() => changePresence("enabled", !presence.enabled)}>
              {presence.enabled ? "On" : "Off"}
            </button>
          </div>
          <div class="field inline">
            <span>Start with Windows</span>
            <button
              class="btn btn-ghost"
              onclick={() => changePresence("startup", !presence.start_with_windows)}
            >
              {presence.start_with_windows ? "On" : "Off"}
            </button>
          </div>
          <div class="field inline">
            <span>Status</span>
            <span class="enhance-copy">{presenceLabel()}</span>
          </div>
          {#if presence.enabled}
            <div class="config-actions">
              <Button variant="outline" onclick={() => changePresence("paused", !presence.paused)}>
                {presence.paused ? "Resume detection" : "Pause detection"}
              </Button>
            </div>
          {/if}
          {#if presenceError}<div class="test-output error">{presenceError}</div>{/if}
        </div>
      {:else if active === "ai"}
        <div class="my-notes-card">
          <ProviderConfigForm />
        </div>
      {:else if active === "shortcut"}
        <div class="my-notes-card provider-config">
          <div class="enhance-title">Global shortcut</div>
          <div class="enhance-copy">
            Press this key combination anywhere in the app to start or stop a recording. Examples:
            <code>Ctrl+Shift+R</code>, <code>Alt+F10</code>,
            <code>CommandOrControl+Shift+N</code>.
          </div>
          <label class="field">
            <span>Accelerator</span>
            <input type="text" bind:value={shortcut} placeholder="Ctrl+Shift+R" />
          </label>
          <div class="config-actions">
            <button class="btn btn-primary" onclick={saveShortcut} disabled={savingShortcut}>
              {savingShortcut ? "Saving…" : "Save shortcut"}
            </button>
          </div>
          {#if shortcutSaved}
            <div class="test-output">Shortcut saved.</div>
          {/if}
        </div>
      {:else if active === "about"}
        <div class="my-notes-card provider-config">
          <div class="enhance-title">About Kimi Nola</div>
          <div class="enhance-copy">
            Local-first meeting transcription and AI-enhanced notes for Windows.
          </div>
          <div class="field inline">
            <span>Version</span>
            <span class="enhance-copy">{appVersion || "…"}</span>
          </div>
          <div class="field inline">
            <span>License</span>
            <span class="enhance-copy">MIT</span>
          </div>
        </div>
      {:else if active === "templates"}
        <div class="my-notes-card provider-config">
          <div class="enhance-title">Summary templates</div>
          <div class="enhance-copy">
            Built-in templates cannot be edited or deleted. Custom templates must include
            <code>{`{transcript}`}</code> and <code>{`{notes}`}</code>.
          </div>

          {#if templatesLoading}
            <div class="empty-state" style="margin-top: 16px;">Loading templates…</div>
          {:else}
            <div class="template-picker-row">
              <div class="template-select-wrap">
                <Label for="template-select">Template</Label>
                <Select.Root
                  type="single"
                  value={selectedTemplateId !== undefined ? String(selectedTemplateId) : ""}
                  onValueChange={(value) => {
                    const id = Number(value);
                    const t = templates.find((x) => x.id === id);
                    if (t) selectTemplate(t);
                  }}
                >
                  <Select.Trigger id="template-select" class="w-full">
                    {selectedTemplate?.name ??
                      (selectedTemplateId === -1 ? editingName : "Choose a template")}
                  </Select.Trigger>
                  <Select.Content>
                    {#each templates as t (t.id)}
                      <Select.Item value={String(t.id)} label={t.name}>
                        {t.name}{#if t.is_builtin} · built-in{/if}
                      </Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
              </div>
              <Button variant="outline" size="sm" onclick={newTemplate}>New template</Button>
            </div>

            {#if selectedTemplateId !== undefined}
              <div class="field">
                <Label for="template-name">Name</Label>
                <Input
                  id="template-name"
                  bind:value={editingName}
                  disabled={selectedTemplate?.is_builtin === 1}
                />
              </div>
              <div class="field">
                <Label for="template-prompt">Prompt</Label>
                <Textarea
                  id="template-prompt"
                  bind:value={editingPrompt}
                  rows={12}
                  disabled={selectedTemplate?.is_builtin === 1}
                />
              </div>
              {#if !selectedTemplate?.is_builtin}
                <div class="config-actions">
                  <Button onclick={saveTemplate}>Save template</Button>
                  <Button variant="outline" onclick={deleteSelectedTemplate}>Delete</Button>
                </div>
              {/if}
            {/if}
            {#if templateStatus}
              <div class="test-output" class:error={templateStatus.error}>
                {templateStatus.message}
              </div>
            {/if}
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .template-picker-row {
    display: flex;
    align-items: flex-end;
    gap: 10px;
    margin-top: 8px;
  }
  .template-select-wrap {
    flex: 1;
    min-width: 0;
  }
</style>
