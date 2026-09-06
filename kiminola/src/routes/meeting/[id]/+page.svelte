<script lang="ts">
  import { onDestroy } from "svelte";
  import { loadMeetingAfterAutosave } from "$lib/meeting-notes";
  import { meetingNotesAutosave as notesAutosave } from "$lib/meeting-notes-session";
  import { exportMeeting, type MeetingExportAction } from "$lib/meeting-export";
  import { page } from "$app/state";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import {
    getMeeting,
    renameMeeting,
    getLlmConfig,
    listTemplates,
    enhanceMeeting,
    updateSegmentText,
    deleteSegment,
    type MeetingDetail,
    type ProviderConfig,
    type Template,
    type TranscriptLine,
  } from "$lib/tauri";
  import { renderMarkdown } from "$lib/markdown";
  import ProviderConfigForm from "$lib/components/ProviderConfigForm.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Textarea } from "$lib/components/ui/textarea";
  import * as Tabs from "$lib/components/ui/tabs";
  import * as Select from "$lib/components/ui/select";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";

  type Tab = "mynotes" | "enhance" | "transcript";

  let meeting = $state<MeetingDetail | null>(null);
  let notFound = $state(false);
  let tab = $state<Tab>("mynotes");
  let notes = $state("");
  let showTranscriptFinalizationWarning = $state(false);
  let notesSaveError = $state(false);
  const unsubscribeNotes = notesAutosave.subscribe((status) => {
    notesSaveError = status === "error";
  });
  let enhancementVersion = 0;
  onDestroy(() => {
    enhancementVersion++;
    clearTimeout(renderTimer);
    unsubscribeNotes();
    void notesAutosave.flush().catch((error) => console.error("Failed to save notes:", error));
  });

  // Inline title editing
  let editingTitle = $state(false);
  let editTitle = $state("");
  let titleInputRef = $state<HTMLInputElement | null>(null);

  // Enhancement state
  let templates = $state<Template[]>([]);
  let selectedTemplateId = $state<number | undefined>(undefined);
  let config = $state<ProviderConfig | null>(null);
  let configLoaded = $state(false);
  let enhancing = $state(false);
  let enhanceError = $state<string | null>(null);
  let enhancedMd = $state("");
  let enhancedGenerated = $state(false);
  let hasEverEnhanced = $state(false);
  // Rendered HTML is updated on a throttle while streaming: re-parsing and
  // sanitizing the whole document on every token chunk stalls the UI.
  let enhancedHtml = $state("");
  let renderTimer: ReturnType<typeof setTimeout> | undefined;

  function renderEnhancedNow() {
    clearTimeout(renderTimer);
    renderTimer = undefined;
    enhancedHtml = renderMarkdown(enhancedMd);
  }

  function scheduleEnhancedRender() {
    if (renderTimer !== undefined) return;
    renderTimer = setTimeout(() => {
      renderTimer = undefined;
      enhancedHtml = renderMarkdown(enhancedMd);
    }, 100);
  }

  function isEnhanceMode(value: string | null): value is "generate" | "enhance" {
    return value === "generate" || value === "enhance";
  }

  async function loadConfig() {
    try {
      config = await getLlmConfig();
      templates = await listTemplates();
      selectedTemplateId = templates[0]?.id;
    } catch (err) {
      console.error("Failed to load LLM config/templates:", err);
      config = null;
    } finally {
      configLoaded = true;
    }
  }

  async function refreshConfig() {
    try {
      config = await getLlmConfig();
    } catch (err) {
      console.error("Failed to refresh LLM config:", err);
    }
  }

  async function runEnhancement() {
    if (!meeting || enhancing) return;
    const id = meeting.id;
    const version = ++enhancementVersion;
    enhancing = true;
    enhanceError = null;
    enhancedMd = "";
    enhancedHtml = "";
    enhancedGenerated = false;

    try {
      await notesAutosave.flush(id);
      if (version !== enhancementVersion) return;
      await enhanceMeeting(id, selectedTemplateId, (event) => {
        if (version !== enhancementVersion) return;
        if (event.event === "chunk") {
          enhancedMd += event.data;
          scheduleEnhancedRender();
        } else {
          enhancing = false;
          if (event.event === "done") {
            enhancedGenerated = true;
            hasEverEnhanced = true;
          } else {
            enhanceError = event.data;
          }
          renderEnhancedNow();
        }
      });
    } catch (err) {
      if (version !== enhancementVersion) return;
      enhanceError = String(err);
      enhancing = false;
    }
  }

  // (Re)load whenever the route param changes.
  $effect(() => {
    const id = Number(page.params.id);
    const mode = page.url.searchParams.get("mode");
    const warning = page.url.searchParams.get("warning");
    let active = true;
    enhancementVersion++;
    enhancing = false;
    enhanceError = null;
    clearTimeout(renderTimer);
    renderTimer = undefined;
    meeting = null;
    notFound = false;
    tab = "mynotes";
    notes = "";
    showTranscriptFinalizationWarning = warning === "transcript-finalization";
    enhancedMd = "";
    enhancedHtml = "";
    enhancedGenerated = false;
    hasEverEnhanced = false;
    configLoaded = false;
    if (!Number.isInteger(id)) {
      notFound = true;
      return;
    }
    loadConfig();
    loadMeetingAfterAutosave(
      notesAutosave,
      () => getMeeting(id),
      (error) => console.error("Failed to save notes:", error),
    )
      .then((m) => {
        if (!active) return;
        meeting = m;
        notes = notesAutosave.pendingNotes(id) ?? m.notepad;
        if (m.enhanced_markdown) {
          enhancedMd = m.enhanced_markdown;
          renderEnhancedNow();
          enhancedGenerated = true;
          hasEverEnhanced = true;
        }
        if (isEnhanceMode(mode)) {
          tab = "enhance";
          if (config?.model) {
            runEnhancement();
          }
        }
      })
      .catch(() => {
        if (active) notFound = true;
      });
    return () => { active = false; };
  });

  $effect(() => {
    if (editingTitle && titleInputRef) {
      titleInputRef.focus();
    }
  });

  function startTitleEdit() {
    if (!meeting) return;
    editTitle = meeting.title;
    editingTitle = true;
  }

  async function saveTitle() {
    if (!meeting) return;
    const trimmed = editTitle.trim();
    if (trimmed && trimmed !== meeting.title) {
      try {
        await renameMeeting(meeting.id, trimmed);
        meeting.title = trimmed;
      } catch (err) {
        console.error("Failed to rename meeting:", err);
      }
    }
    editingTitle = false;
  }

  function cancelTitleEdit() {
    editingTitle = false;
  }

  function onTitleKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      saveTitle();
    } else if (event.key === "Escape") {
      cancelTitleEdit();
    }
  }

  // Export (SPEC §8): clipboard copy + .md notes / .txt transcript.
  let exportStatus = $state<string | null>(null);
  let exportSavedPath = $state<string | null>(null);
  let exportTimer: ReturnType<typeof setTimeout> | undefined;

  function flashExportStatus(message: string, path: string | null = null) {
    exportStatus = message;
    exportSavedPath = path;
    clearTimeout(exportTimer);
    exportTimer = setTimeout(() => {
      exportStatus = null;
      exportSavedPath = null;
    }, 5000);
  }

  async function runExport(action: MeetingExportAction) {
    if (!meeting) return;
    const id = meeting.id;
    try {
      const result = await exportMeeting(action, id);
      flashExportStatus(result.message, result.path ?? null);
    } catch (err) {
      flashExportStatus(`Export failed: ${String(err)}`);
    }
  }

  async function revealExport() {
    if (exportSavedPath) await revealItemInDir(exportSavedPath);
  }

  // Inline transcript editing.
  let editingSegmentId = $state<number | undefined>(undefined);
  let editSegmentText = $state("");

  function startSegmentEdit(line: TranscriptLine) {
    if (line.id === undefined) return;
    editingSegmentId = line.id;
    editSegmentText = line.text;
  }

  function cancelSegmentEdit() {
    editingSegmentId = undefined;
    editSegmentText = "";
  }

  async function saveSegmentEdit() {
    if (editingSegmentId === undefined || !meeting) return;
    const text = editSegmentText.trim();
    if (!text) {
      cancelSegmentEdit();
      return;
    }
    try {
      await updateSegmentText(editingSegmentId, text);
      const line = meeting.transcript.find((l) => l.id === editingSegmentId);
      if (line) line.text = text;
    } catch (err) {
      console.error("Failed to update segment:", err);
    } finally {
      cancelSegmentEdit();
    }
  }

  async function removeSegment(segmentId: number) {
    if (!meeting) return;
    try {
      await deleteSegment(segmentId);
      meeting.transcript = meeting.transcript.filter((l) => l.id !== segmentId);
      if (editingSegmentId === segmentId) cancelSegmentEdit();
    } catch (err) {
      console.error("Failed to delete segment:", err);
    }
  }

  function onSegmentKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      cancelSegmentEdit();
    } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      saveSegmentEdit();
    }
  }

  // Notes edits persist automatically, debounced.
  function onNotesInput() {
    if (meeting) notesAutosave.schedule(meeting.id, notes);
  }

  function formatMeta(m: MeetingDetail): string {
    const date = new Date(m.created_at).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
    });
    const mins = Math.max(1, Math.round(m.duration_seconds / 60));
    return `${date} · ${mins} min`;
  }

  let configured = $derived(configLoaded && config != null && config.base_url.trim() !== "" && config.model.trim() !== "");

  const templateOptions = $derived(templates.map((t) => ({ value: String(t.id), label: t.name })));
  const selectedTemplateLabel = $derived(
    templates.find((t) => t.id === selectedTemplateId)?.name ?? "Template",
  );
</script>

<svelte:head>
  <title>{meeting?.title ?? "Meeting"} — Kimi Nola</title>
</svelte:head>

<div class="main-content">
  {#if notFound}
    <div class="post-shell">
      <div class="post-back">
        <Button variant="ghost" size="sm" href="/">← Back to meetings</Button>
      </div>
      <div class="empty-state" style="margin-top:16px;">Meeting not found.</div>
    </div>
  {:else if meeting}
    <div class="post-shell">
      <div class="post-header">
        <div class="post-back">
          <Button variant="ghost" size="sm" href="/">← Back to meetings</Button>
        </div>
        <div class="post-title-row">
          {#if editingTitle}
            <Input
              class="title-input display"
              type="text"
              bind:value={editTitle}
              bind:ref={titleInputRef}
              onkeydown={onTitleKeydown}
              onblur={saveTitle}
            />
          {:else}
            <button class="display editable-title" onclick={startTitleEdit}>
              {meeting.title}
            </button>
          {/if}
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>
              {#snippet child({ props })}
                <Button {...props} variant="outline" size="sm">Export</Button>
              {/snippet}
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="end" class="w-56">
              <DropdownMenu.Item onclick={() => runExport("copy-notes")}>
                Copy notes as Markdown
              </DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => runExport("copy-transcript")}>
                Copy transcript
              </DropdownMenu.Item>
              <DropdownMenu.Separator />
              <DropdownMenu.Item onclick={() => runExport("save-notes")}>
                Save notes as .md
              </DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => runExport("save-transcript")}>
                Save transcript as .txt
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </div>
        <div class="post-meta-row">
          <span class="post-meta-pill">{formatMeta(meeting)}</span>
          {#if meeting.location_path ?? meeting.space_name}
            <span class="post-meta-pill">{meeting.location_path ?? meeting.space_name}</span>
          {/if}
        </div>
        {#if exportStatus}
          <div class="export-status">
            <span>{exportStatus}</span>
            {#if exportSavedPath}
              <button class="export-reveal" onclick={revealExport}>Show in folder</button>
            {/if}
          </div>
        {/if}
      </div>

      {#if showTranscriptFinalizationWarning}
        <div class="finalization-warning" role="alert">
          <div>
            <strong>Meeting saved, but the transcript may end early.</strong>
            <span>
              The final speech-processing pass did not finish, so the last few words may be
              missing. Your notes and the rest of the transcript were saved.
            </span>
          </div>
          <div class="finalization-warning-actions">
            <Button
              size="sm"
              variant="outline"
              onclick={() => {
                tab = "transcript";
                showTranscriptFinalizationWarning = false;
              }}>Review transcript</Button
            >
            <Button
              size="sm"
              variant="ghost"
              onclick={() => (showTranscriptFinalizationWarning = false)}>Dismiss</Button
            >
          </div>
        </div>
      {/if}

      <Tabs.Root value={tab} onValueChange={(value) => (tab = value as Tab)} class="w-full">
        <Tabs.List class="pill-tabs-list">
          <Tabs.Trigger value="mynotes" class="pill-tab">My notes</Tabs.Trigger>
          <Tabs.Trigger value="enhance" class="pill-tab">Enhance Notes</Tabs.Trigger>
          <Tabs.Trigger value="transcript" class="pill-tab">Transcript</Tabs.Trigger>
        </Tabs.List>
        <Tabs.Content value="mynotes" class="post-content">
          <div class="my-notes-card">
            <Textarea
              bind:value={notes}
              oninput={onNotesInput}
              placeholder="No notes captured during this meeting."
              aria-label="My notes"
              class="notes-textarea"
            />
            {#if notesSaveError}
              <p role="alert">Some edits could not be saved. Retry saving before quitting Kimi Nola.</p>
              <Button variant="secondary" onclick={() => void notesAutosave.flush().catch((error) => console.error("Failed to save notes:", error))}>
                Retry saving
              </Button>
            {/if}
          </div>
        </Tabs.Content>
        <Tabs.Content value="enhance" class="post-content">
          {#if !configLoaded}
            <div class="empty-state">Loading AI provider settings…</div>
          {:else if !configured}
            <div class="my-notes-card">
              <ProviderConfigForm onSaved={refreshConfig} />
            </div>
          {:else if !hasEverEnhanced && !enhancing}
            <div class="my-notes-card enhance-prompt">
              <div class="enhance-title">Enhance notes with AI</div>
              <div class="enhance-copy">
                Merge your notes with the transcript into a structured summary, action items,
                and key decisions.
              </div>
              <div class="field inline">
                <span>Template</span>
                <Select.Root
                  type="single"
                  value={selectedTemplateId !== undefined ? String(selectedTemplateId) : ""}
                  onValueChange={(value) => (selectedTemplateId = Number(value))}
                >
                  <Select.Trigger class="w-[200px]">
                    {selectedTemplateLabel}
                  </Select.Trigger>
                  <Select.Content>
                    {#each templateOptions as option (option.value)}
                      <Select.Item value={option.value} label={option.label}>
                        {option.label}
                      </Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
              </div>
              <Button onclick={runEnhancement} disabled={enhancing}>
                {enhancing ? "Enhancing…" : "Enhance notes"}
              </Button>
            </div>
          {:else}
            <div class="my-notes-card note-document">
              <div class="enhance-toolbar">
                <div class="tool-left">
                  <span class="tool-label">Template</span>
                  <Select.Root
                    type="single"
                    value={selectedTemplateId !== undefined ? String(selectedTemplateId) : ""}
                    onValueChange={(value) => (selectedTemplateId = Number(value))}
                  >
                    <Select.Trigger class="w-[200px]">
                      {selectedTemplateLabel}
                    </Select.Trigger>
                    <Select.Content>
                      {#each templateOptions as option (option.value)}
                        <Select.Item value={option.value} label={option.label}>
                          {option.label}
                        </Select.Item>
                      {/each}
                    </Select.Content>
                  </Select.Root>
                  <a class="manage-templates" href="/settings">Manage templates</a>
                </div>
                <div class="tool-actions">
                  <Button
                    variant="outline"
                    size="sm"
                    onclick={runEnhancement}
                    disabled={enhancing}
                  >
                    {enhancing ? "Regenerating…" : "Regenerate"}
                  </Button>
                </div>
              </div>
              {#if enhancing}
                <div class="streaming-hint">AI is rewriting your notes…</div>
              {/if}
              {@html enhancedHtml}
            </div>
          {/if}
          {#if enhanceError}
            <div class="test-output error" style="margin-top: 12px;">{enhanceError}</div>
          {/if}
        </Tabs.Content>
        <Tabs.Content value="transcript" class="post-content">
          {#if meeting.transcript.length > 0}
            <div class="raw-transcript">
              {#each meeting.transcript as line (line.id ?? line.text)}
                {#if editingSegmentId === line.id}
                  <div class="raw-line editing">
                    <span class="tag {line.channel}">{line.channel === "you" ? "You" : "Others"}</span>
                    <div class="segment-edit-wrap">
                      <Textarea
                        bind:value={editSegmentText}
                        onkeydown={onSegmentKeydown}
                        onblur={saveSegmentEdit}
                        rows={2}
                        class="segment-edit-textarea"
                      />
                      <div class="segment-edit-actions">
                        <button class="segment-action save" onclick={saveSegmentEdit}>Save</button>
                        <button class="segment-action" onclick={cancelSegmentEdit}>Cancel</button>
                        {#if line.id !== undefined}
                          <button class="segment-action delete" onclick={() => line.id !== undefined && removeSegment(line.id)}>Delete</button>
                        {/if}
                      </div>
                    </div>
                  </div>
                {:else}
                  <button
                    type="button"
                    class="raw-line"
                    onclick={() => startSegmentEdit(line)}
                    title="Click to edit"
                  >
                    <span class="tag {line.channel}">{line.channel === "you" ? "You" : "Others"}</span>
                    {line.text}
                  </button>
                {/if}
              {/each}
            </div>
          {:else}
            <div class="empty-state">No transcript captured for this meeting.</div>
          {/if}
        </Tabs.Content>
      </Tabs.Root>
    </div>
  {/if}
</div>

<style>
  :global(.notes-textarea) {
    width: 100%;
    min-height: 220px;
    border: none;
    background: transparent;
    padding: 0;
    font: inherit;
    font-size: 16px;
    line-height: 1.6;
    color: var(--ink);
    resize: vertical;
  }
  :global(.notes-textarea:focus) {
    box-shadow: none;
    border: none;
    outline: none;
  }
  :global(.notes-textarea::placeholder) {
    color: var(--soft);
  }

  :global(.pill-tabs-list) {
    align-self: flex-start;
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-pill);
    padding: 4px;
    gap: 4px;
  }

  :global(.pill-tab) {
    border-radius: var(--radius-pill);
    padding: 7px 14px;
    font-size: 14px;
    font-weight: 500;
    color: var(--text-muted);
    flex: 0 0 auto;
  }
  :global(.pill-tab:hover) {
    color: var(--ink);
  }
  :global(.pill-tab[data-state="active"]) {
    background: var(--canvas);
    color: var(--ink-strong);
    box-shadow: 0 1px 3px var(--shadow-ambient);
  }

  .export-status {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    font-size: 13px;
    color: var(--text-muted);
  }

  .finalization-warning {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 14px 16px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--ink);
    font-size: 13px;
  }

  .finalization-warning > div:first-child {
    display: grid;
    gap: 3px;
  }

  .finalization-warning span {
    color: var(--text-muted);
  }

  .finalization-warning-actions {
    display: flex;
    flex-shrink: 0;
    gap: 8px;
  }

  @media (max-width: 680px) {
    .finalization-warning {
      align-items: stretch;
      flex-direction: column;
    }

    .finalization-warning-actions {
      flex-wrap: wrap;
    }
  }

  .export-reveal {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--brand);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }

  .manage-templates {
    font-size: 13px;
    color: var(--brand);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .manage-templates:hover {
    color: var(--brand-hover);
  }

  .raw-line {
    cursor: pointer;
  }
  .raw-line.editing {
    cursor: default;
    align-items: flex-start;
  }
  .segment-edit-wrap {
    flex: 1;
    min-width: 0;
  }
  :global(.segment-edit-textarea) {
    width: 100%;
    min-height: 48px;
    font: inherit;
    font-size: 15px;
    line-height: 1.5;
  }
  .segment-edit-actions {
    display: flex;
    gap: 10px;
    margin-top: 6px;
  }
  .segment-action {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-size: 13px;
    color: var(--text-muted);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .segment-action.save {
    color: var(--brand);
  }
  .segment-action.delete {
    color: var(--destructive);
  }
</style>
