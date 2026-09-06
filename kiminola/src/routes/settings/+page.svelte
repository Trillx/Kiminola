<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
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
    checkModelPack,
    downloadModelPack,
    openModelFolder,
    type DownloadEvent,
    type MeetingPresenceState,
    type Template,
  } from "$lib/tauri";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Textarea } from "$lib/components/ui/textarea";
  import { Label } from "$lib/components/ui/label";
  import { Progress } from "$lib/components/ui/progress";
  import * as Select from "$lib/components/ui/select";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Switch } from "$lib/components/ui/switch";
  import { compactReleaseNotes, isRecordingPath } from "$lib/update-policy";
  import { checkForUpdates, installUpdate, updateState } from "$lib/update.svelte";
  import {
    nextSettingsSection,
    resolveSettingsSection,
    SETTINGS_SECTIONS,
    settingsSectionHref,
    templateNeedsDeleteConfirmation,
    type SettingsSection,
  } from "$lib/settings-ui";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Check from "@lucide/svelte/icons/check";
  import Moon from "@lucide/svelte/icons/moon";
  import Plus from "@lucide/svelte/icons/plus";
  import Sun from "@lucide/svelte/icons/sun";

  let active = $state<SettingsSection>(resolveSettingsSection(page.url.searchParams.get("section")));
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
  let modelState = $state<"idle" | "checking" | "ready" | "missing" | "downloading" | "error">(
    "idle",
  );
  let modelProgress = $state(0);
  let modelDownloadedMB = $state(0);
  let modelTotalMB = $state(663);
  let modelError = $state("");

  // Templates state
  let templates = $state<Template[]>([]);
  let selectedTemplateId = $state<number | undefined>(undefined);
  let editingName = $state("");
  let editingPrompt = $state("");
  let templatesLoading = $state(false);
  let templateStatus = $state<{ message: string; error: boolean } | null>(null);
  let deleteConfirmOpen = $state(false);
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
      deleteConfirmOpen = false;
      flashTemplateStatus("Template deleted.");
    } catch (err) {
      flashTemplateStatus(String(err), true);
    }
  }

  function requestTemplateDelete() {
    if (!selectedTemplate || !templateNeedsDeleteConfirmation(selectedTemplate)) return;
    deleteConfirmOpen = true;
  }

  function activateTemplates() {
    active = "templates";
    loadTemplates();
  }

  async function refreshModelHealth() {
    modelState = "checking";
    modelError = "";
    try {
      modelState = (await checkModelPack()) ? "ready" : "missing";
    } catch (err) {
      modelError = String(err);
      modelState = "error";
    }
  }

  async function repairModel() {
    modelState = "downloading";
    modelProgress = 0;
    modelDownloadedMB = 0;
    modelTotalMB = 663;
    modelError = "";
    try {
      await downloadModelPack((event: DownloadEvent) => {
        const total = Math.max(1, event.overall_total);
        modelProgress = Math.min(100, (event.overall_downloaded / total) * 100);
        modelDownloadedMB = Math.floor(event.overall_downloaded / 1_048_576);
        modelTotalMB = Math.max(1, Math.ceil(event.overall_total / 1_048_576));
      });
      if (!(await checkModelPack())) {
        throw new Error("The downloaded model pack did not pass verification.");
      }
      modelProgress = 100;
      modelState = "ready";
    } catch (err) {
      modelError = String(err);
      modelState = "error";
    }
  }

  async function showModelFolder() {
    try {
      await openModelFolder();
    } catch (err) {
      modelError = String(err);
      modelState = "error";
    }
  }

  function activateSection(section: SettingsSection) {
    if (section === "templates") {
      activateTemplates();
    } else {
      active = section;
    }
    void goto(settingsSectionHref(section), { replaceState: true, noScroll: true, keepFocus: true });
    if (section === "models" && modelState !== "downloading") void refreshModelHealth();
  }

  function onSectionKeydown(event: KeyboardEvent, section: SettingsSection) {
    const next = nextSettingsSection(section, event.key);
    if (!next) return;
    event.preventDefault();
    activateSection(next);
    requestAnimationFrame(() => document.getElementById(`settings-tab-${next}`)?.focus());
  }

  $effect(() => {
    const requested = resolveSettingsSection(page.url.searchParams.get("section"));
    if (requested !== active) {
      active = requested;
      if (requested === "templates") void loadTemplates();
      if (requested === "models" && modelState !== "downloading") void refreshModelHealth();
    }
  });

  onMount(() => {
    if (active === "models") void refreshModelHealth();
    if (active === "templates") void loadTemplates();
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

  async function installAppUpdate() {
    await installUpdate(() => !isRecordingPath(page.url.pathname));
  }
</script>

<svelte:head>
  <title>Settings — Kimi Nola</title>
</svelte:head>

<div class="main-content settings-page">
  <div class="post-shell settings-layout">
    <aside class="settings-rail" aria-label="Settings sections">
      <a class="settings-back" href="/"><ArrowLeft size={15} aria-hidden="true" /> Meetings</a>
      <h1 class="display settings-title">Settings</h1>
      <div class="settings-nav" role="tablist" aria-label="Settings sections">
        {#each SETTINGS_SECTIONS as section}
          <button
            id={`settings-tab-${section.id}`}
            class="settings-nav-item"
            class:active={active === section.id}
            role="tab"
            aria-selected={active === section.id}
            aria-controls="settings-panel"
            tabindex={active === section.id ? 0 : -1}
            onclick={() => activateSection(section.id)}
            onkeydown={(event) => onSectionKeydown(event, section.id)}
          >
            {section.label}
          </button>
        {/each}
      </div>
    </aside>

    <div
      id="settings-panel"
      class="settings-pane"
      role="tabpanel"
      aria-labelledby={`settings-tab-${active}`}
      tabindex="0"
    >
      {#if active === "general"}
        <section class="settings-card general-settings-card">
          <header class="settings-card-header">
            <h2>General</h2>
            <p>Appearance and meeting prompt preferences.</p>
          </header>

          <div class="settings-group">
            <div class="settings-row">
              <div class="settings-row-copy">
                <strong>Theme</strong>
                <span>Choose the appearance used throughout Kimi Nola.</span>
              </div>
              <div class="theme-options" role="group" aria-label="Theme">
                <button
                  class:active={themeState.theme === "light"}
                  aria-pressed={themeState.theme === "light"}
                  onclick={() => themeState.theme !== "light" && toggleTheme()}
                ><Sun size={15} aria-hidden="true" /> Light</button>
                <button
                  class:active={themeState.theme === "dark"}
                  aria-pressed={themeState.theme === "dark"}
                  onclick={() => themeState.theme !== "dark" && toggleTheme()}
                ><Moon size={15} aria-hidden="true" /> Dark</button>
              </div>
            </div>
          </div>

          <div class="settings-section-divider"></div>

          <div class="settings-subheader">
            <h2>Meeting prompts</h2>
            <p>
              Look for supported meeting apps locally and ask before doing anything. Recording never
              starts automatically.
            </p>
          </div>

          <div class="settings-group">
            <div class="settings-row">
              <div class="settings-row-copy">
                <strong>Meeting detection</strong>
                <span>{presenceLabel()}</span>
              </div>
              <Switch
                checked={presence.enabled}
                onCheckedChange={(checked) => void changePresence("enabled", checked)}
                aria-label="Meeting detection"
              />
            </div>
            <div class="settings-row">
              <div class="settings-row-copy">
                <strong>Start with Windows</strong>
                <span>Keep meeting prompts available after signing in.</span>
              </div>
              <Switch
                checked={presence.start_with_windows}
                onCheckedChange={(checked) => void changePresence("startup", checked)}
                aria-label="Start with Windows"
              />
            </div>
          </div>

          {#if presence.enabled}
            <div class="settings-inline-actions">
              <Button variant="outline" onclick={() => changePresence("paused", !presence.paused)}>
                {presence.paused ? "Resume detection" : "Pause detection"}
              </Button>
            </div>
          {/if}
          {#if presenceError}<div class="test-output error" role="alert">{presenceError}</div>{/if}
        </section>
      {:else if active === "models"}
        <div class="settings-card provider-config">
          <div class="enhance-title">On-device speech model</div>
          <div class="enhance-copy">
            Kimi Nola uses this local model for live transcription. Model files stay on this
            machine, and meeting audio is never uploaded.
          </div>

          {#if modelState === "idle" || modelState === "checking"}
            <div class="model-status" aria-live="polite">Checking the model packâ€¦</div>
          {:else if modelState === "ready"}
            <div class="model-status ready" role="status">
              <strong>Model ready.</strong>
              <span>Live transcription is available for the next meeting.</span>
            </div>
          {:else if modelState === "missing"}
            <div class="model-status" role="alert">
              <strong>Model repair needed.</strong>
              <span>One or more required model files are missing or incomplete.</span>
            </div>
          {:else if modelState === "downloading"}
            <div class="model-progress" aria-live="polite">
              <div class="model-progress-row">
                <span>Downloading and verifyingâ€¦</span>
                <span class="mono">{modelDownloadedMB} / {modelTotalMB} MB</span>
              </div>
              <Progress value={modelProgress} max={100} class="h-2" />
            </div>
          {:else if modelState === "error"}
            <div class="model-status" role="alert">
              <strong>Model repair failed.</strong>
              <span>{modelError}</span>
            </div>
          {/if}

          <div class="config-actions">
            {#if modelState === "missing" || modelState === "error"}
              <Button onclick={repairModel}>Download or repair model</Button>
            {:else if modelState === "ready"}
              <Button variant="outline" onclick={refreshModelHealth}>Verify again</Button>
            {/if}
            <Button
              variant="outline"
              onclick={() => void showModelFolder()}
              disabled={modelState === "downloading"}>Open model folder</Button
            >
          </div>
        </div>
      {:else if active === "ai"}
        <div class="settings-card">
          <ProviderConfigForm />
        </div>
      {:else if active === "shortcut"}
        <div class="settings-card provider-config">
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
        <div class="settings-card provider-config">
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
        <div class="settings-card provider-config update-settings-card">
          <div class="enhance-title">Updates</div>
          <div class="enhance-copy">
            Kimi Nola checks the published stable GitHub release after launch. It never installs an
            update while a meeting is active, and installation always requires your confirmation.
          </div>

          {#if updateState.status === "checking"}
            <div class="model-status" aria-live="polite">Checking for a stable update…</div>
          {:else if updateState.status === "available"}
            <div class="model-status ready" role="status">
              <strong>Version {updateState.version} is available.</strong>
              {#if compactReleaseNotes(updateState.notes)}
                <span>{compactReleaseNotes(updateState.notes)}</span>
              {/if}
            </div>
          {:else if updateState.status === "downloading"}
            <div class="model-progress" aria-live="polite">
              <div class="model-progress-row">
                <span>Downloading signed update…</span>
                <span class="mono">{updateState.progress}%</span>
              </div>
              <Progress value={updateState.progress} max={100} class="h-2" />
            </div>
          {:else if updateState.status === "ready"}
            <div class="model-status ready" role="status">
              <strong>Update ready to install.</strong>
              <span>Kimi Nola will close and restart after installation.</span>
            </div>
          {:else if updateState.status === "preparing"}
            <div class="model-status" aria-live="polite">Saving your changes before updating…</div>
          {:else if updateState.status === "installing"}
            <div class="model-status" aria-live="polite">
              Installing the update. Kimi Nola will restart automatically.
            </div>
          {:else if updateState.status === "up_to_date"}
            <div class="model-status ready" role="status">
              <strong>You’re up to date.</strong>
              <span>Only published stable releases are offered here.</span>
            </div>
          {:else if updateState.status === "error"}
            <div class="model-status" role="alert">
              <strong>Update check failed.</strong>
              <span>{updateState.error}</span>
            </div>
          {:else}
            <div class="model-status">
              <span>Stable update checks run once after each app launch.</span>
            </div>
          {/if}

          {#if updateState.error && updateState.status !== "error"}
            <p class="model-status error" role="alert">{updateState.error}</p>
          {/if}
          <div class="config-actions">
            {#if updateState.status === "available"}
              <Button onclick={() => void installAppUpdate()}>Install update</Button>
              <Button variant="outline" onclick={() => void checkForUpdates()}>Check again</Button>
            {:else if updateState.status === "ready"}
              <Button onclick={() => void installAppUpdate()}>Restart and update</Button>
            {:else if updateState.status !== "downloading" && updateState.status !== "preparing" && updateState.status !== "installing"}
              <Button variant="outline" onclick={() => void checkForUpdates()}>
                {updateState.status === "error" ? "Try again" : "Check for updates"}
              </Button>
            {/if}
          </div>
        </div>
      {:else if active === "templates"}
        <section class="settings-card template-settings-card">
          <header class="settings-card-header settings-card-header-row">
            <div>
              <h2>Summary templates</h2>
              <p>Choose a built-in template or create a custom prompt.</p>
            </div>
            <Button variant="outline" onclick={newTemplate}><Plus size={15} aria-hidden="true" /> New template</Button>
          </header>

          <div class="template-requirements">
            <span>Required variables</span>
            <code>{`{transcript}`}</code>
            <code>{`{notes}`}</code>
          </div>

          {#if templatesLoading}
            <div class="empty-state" aria-live="polite">Loading templates…</div>
          {:else}
            <div class="template-picker-row">
              <div class="template-select-wrap">
                <Label for="template-select">Template</Label>
                <Select.Root
                  type="single"
                  value={selectedTemplateId !== undefined && selectedTemplateId !== -1
                    ? String(selectedTemplateId)
                    : ""}
                  onValueChange={(value) => {
                    const id = Number(value);
                    const t = templates.find((x) => x.id === id);
                    if (t) selectTemplate(t);
                  }}
                >
                  <Select.Trigger id="template-select" class="w-full">
                    {selectedTemplateId === -1 ? "New custom template" : (selectedTemplate?.name ?? "Choose a template")}
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
            </div>

            {#if selectedTemplateId !== undefined}
              {#if selectedTemplate?.is_builtin}
                <div class="template-readonly-heading">
                  <div>
                    <span class="template-readonly-badge"><Check size={13} aria-hidden="true" /> Built-in · read-only</span>
                    <h3>{editingName}</h3>
                  </div>
                </div>
                <pre class="template-preview" role="region" aria-label="Built-in template prompt">{editingPrompt}</pre>
              {:else}
                <div class="template-editor">
                  <div class="field">
                    <Label for="template-name">Name</Label>
                    <Input id="template-name" bind:value={editingName} placeholder="Template name" />
                  </div>
                  <div class="template-editor-scroll">
                    <div class="field">
                      <Label for="template-prompt">Prompt</Label>
                      <Textarea
                        id="template-prompt"
                        class="template-prompt-editor"
                        bind:value={editingPrompt}
                        rows={12}
                      />
                    </div>
                    <div class="template-actions">
                      <Button onclick={saveTemplate}>Save template</Button>
                      {#if selectedTemplateId !== -1}
                        <Button variant="destructive" onclick={requestTemplateDelete}>Delete template</Button>
                      {/if}
                    </div>
                  </div>
                </div>
              {/if}
            {/if}
            {#if templateStatus}
              <div class="test-output" class:error={templateStatus.error} role={templateStatus.error ? "alert" : "status"}>
                {templateStatus.message}
              </div>
            {/if}
          {/if}
        </section>
      {/if}
    </div>
  </div>
</div>

<Dialog.Root bind:open={deleteConfirmOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Delete “{selectedTemplate?.name ?? "template"}”?</Dialog.Title>
      <Dialog.Description>
        This permanently removes the custom template. Meetings that already used it are unchanged.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteConfirmOpen = false)}>Cancel</Button>
      <Button variant="destructive" onclick={() => void deleteSelectedTemplate()}>Delete template</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<style>
  .model-status {
    display: grid;
    gap: 3px;
    padding: 12px 14px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--ink);
    font-size: 13px;
  }

  .model-status span {
    color: var(--text-muted);
  }

  .model-status.ready {
    border-color: color-mix(in srgb, var(--ink) 24%, var(--hairline));
  }

  .model-progress {
    display: grid;
    gap: 10px;
    padding: 12px 14px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--ink);
    font-size: 13px;
  }

  .model-progress-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }

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

  .update-settings-card {
    margin-top: 16px;
  }
</style>
