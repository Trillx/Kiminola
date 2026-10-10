<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { page } from "$app/state";
  import { registerUpdateGuard } from "$lib/pending-work";
  import { notepad } from "$lib/notepad-store";
  import { settingsSectionHref } from "$lib/settings-ui";
  import {
    createMeetingView,
    initialMeetingViewState,
    type MeetingExportAction,
    type MeetingTab,
  } from "$lib/meeting-view";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import {
    getMeeting,
    renameMeeting,
    getLlmConfig,
    listTemplates,
    enhanceMeeting,
    exportNotesMarkdown,
    exportTranscriptText,
    saveNotesExport,
    saveTranscriptExport,
    updateEnhancedActionItems,
    updateSegmentText,
    deleteSegment,
    type MeetingDetail,
  } from "$lib/tauri";
  import { renderMarkdown } from "$lib/markdown";
  import BoardQuickAdd from "$lib/components/BoardQuickAdd.svelte";
  import ProviderConfigForm from "$lib/components/ProviderConfigForm.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Textarea } from "$lib/components/ui/textarea";
  import * as Tabs from "$lib/components/ui/tabs";
  import * as Select from "$lib/components/ui/select";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";

  // The Meeting view owns loading, Note enhancement, exports, and transcript and
  // action-item edits; this page renders it.
  const ui = $state(initialMeetingViewState());
  const view = createMeetingView(
    {
      port: {
        getMeeting,
        getLlmConfig,
        listTemplates,
        enhanceMeeting,
        exportNotesMarkdown,
        exportTranscriptText,
        saveNotesExport,
        saveTranscriptExport,
        updateSegmentText,
        deleteSegment,
        updateEnhancedActionItems,
        writeClipboard: (text) => navigator.clipboard.writeText(text),
        renderMarkdown,
        delay(milliseconds, run) {
          const timer = setTimeout(run, milliseconds);
          return () => clearTimeout(timer);
        },
      },
      notepad,
    },
    ui,
  );
  onDestroy(() => view.dispose());

  // (Re)load whenever the route changes.
  $effect(() => {
    const id = Number(page.params.id);
    const searchParams = page.url.searchParams;
    return untrack(() => view.open(id, searchParams));
  });

  // Inline title editing
  let editingTitle = $state(false);
  let editTitle = $state("");
  let titleInputRef = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (editingTitle && titleInputRef) {
      titleInputRef.focus();
    }
  });

  function startTitleEdit() {
    if (!ui.meeting) return;
    editTitle = ui.meeting.title;
    editingTitle = true;
  }

  async function saveTitle() {
    if (!ui.meeting) return;
    const trimmed = editTitle.trim();
    if (trimmed && trimmed !== ui.meeting.title) {
      try {
        await renameMeeting(ui.meeting.id, trimmed);
        ui.meeting.title = trimmed;
      } catch (err) {
        console.error("Failed to rename meeting:", err);
        return;
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
    if (!ui.meeting) return;
    try {
      const result = await view.export(action);
      flashExportStatus(result.message, result.path ?? null);
    } catch (err) {
      flashExportStatus(`Export failed: ${String(err)}`);
    }
  }

  async function revealExport() {
    if (exportSavedPath) await revealItemInDir(exportSavedPath);
  }

  // Inline transcript editing: Escape cancels, Ctrl/Cmd+Enter saves. Losing
  // focus never saves, so clicking Cancel cannot persist the correction first.
  function onSegmentKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      view.cancelSegmentEdit();
    } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void view.saveSegmentEdit();
    }
  }

  $effect(() => {
    return registerUpdateGuard(() => {
      if (editingTitle || ui.editingSegmentId !== undefined) {
        throw new Error("Finish or cancel the title or transcript edit before updating.");
      }
    });
  });

  function formatMeta(m: MeetingDetail): string {
    const date = new Date(m.created_at).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
    });
    const mins = Math.max(1, Math.round(m.duration_seconds / 60));
    return `${date} · ${mins} min`;
  }

  const actionItems = $derived(view.actionItems);
  const templateOptions = $derived(ui.templates.map((t) => ({ value: String(t.id), label: t.name })));
  const selectedTemplateLabel = $derived(
    ui.templates.find((t) => t.id === ui.selectedTemplateId)?.name ?? "Template",
  );
</script>


<svelte:head>
  <title>{ui.meeting?.title ?? "Meeting"} — Kimi Nola</title>
</svelte:head>

<div class="main-content">
  {#if ui.notFound}
    <div class="post-shell">
      <div class="post-back">
        <Button variant="ghost" size="sm" href="/">← Back to meetings</Button>
      </div>
      <div class="empty-state" style="margin-top:16px;">Meeting not found.</div>
    </div>
  {:else if ui.meeting}
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
              {ui.meeting.title}
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
          <span class="post-meta-pill">{formatMeta(ui.meeting)}</span>
          {#if ui.meeting.location_path ?? ui.meeting.space_name}
            <span class="post-meta-pill">{ui.meeting.location_path ?? ui.meeting.space_name}</span>
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

      {#if ui.showFinalizationWarning}
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
                ui.tab = "transcript";
                ui.showFinalizationWarning = false;
              }}>Review transcript</Button
            >
            <Button
              size="sm"
              variant="ghost"
              onclick={() => (ui.showFinalizationWarning = false)}>Dismiss</Button
            >
          </div>
        </div>
      {/if}

      <Tabs.Root value={ui.tab} onValueChange={(value) => (ui.tab = value as MeetingTab)} class="w-full">
        <Tabs.List class="pill-tabs-list">
          <Tabs.Trigger value="mynotes" class="pill-tab">My notes</Tabs.Trigger>
          <Tabs.Trigger value="enhance" class="pill-tab">Enhance Notes</Tabs.Trigger>
          <Tabs.Trigger value="transcript" class="pill-tab">Transcript</Tabs.Trigger>
        </Tabs.List>
        <Tabs.Content value="mynotes" class="post-content">
          <div class="my-notes-card">
            <Textarea
              bind:value={ui.notes}
              oninput={view.notesChanged}
              placeholder="No notes captured during this meeting."
              aria-label="My notes"
              class="notes-textarea"
            />
            {#if ui.notesSaveError}
              <p role="alert">Some edits could not be saved. Retry saving before quitting Kimi Nola.</p>
              <Button variant="secondary" onclick={() => void view.retrySave()}>
                Retry saving
              </Button>
            {/if}
          </div>
        </Tabs.Content>
        <Tabs.Content value="enhance" class="post-content">
          {#if !ui.configLoaded}
            <div class="empty-state">Loading AI provider settings…</div>
          {:else if !view.configured}
            <div class="my-notes-card">
              <ProviderConfigForm onSaved={view.refreshConfig} />
            </div>
          {:else if !ui.hasEverEnhanced && !ui.enhancing}
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
                  value={ui.selectedTemplateId !== undefined ? String(ui.selectedTemplateId) : ""}
                  onValueChange={(value) => (ui.selectedTemplateId = Number(value))}
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
              <Button onclick={view.enhance} disabled={ui.enhancing}>
                {ui.enhancing ? "Enhancing…" : "Enhance notes"}
              </Button>
            </div>
          {:else}
            <div class="my-notes-card note-document">
              <div class="enhance-toolbar">
                <div class="tool-left">
                  <span class="tool-label">Template</span>
                  <Select.Root
                    type="single"
                    value={ui.selectedTemplateId !== undefined ? String(ui.selectedTemplateId) : ""}
                    onValueChange={(value) => (ui.selectedTemplateId = Number(value))}
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
                  <a class="manage-templates" href={settingsSectionHref("templates")}>Manage templates</a>
                </div>
                <div class="tool-actions">
                  {#if !ui.editingActionItems && !ui.savingActionItems}
                    <BoardQuickAdd meetingId={ui.meeting.id} {actionItems} enhancedMarkdown={ui.enhancedMd} />
                  {/if}
                  {#if actionItems.length > 0}
                    <Button
                      variant="outline"
                      size="sm"
                      onclick={view.startActionItemEdit}
                      disabled={ui.enhancing || ui.editingActionItems || ui.savingActionItems}
                    >
                      Edit action items
                    </Button>
                  {/if}
                  <Button
                    variant="outline"
                    size="sm"
                    onclick={view.enhance}
                    disabled={ui.enhancing || ui.editingActionItems || ui.savingActionItems}
                  >
                    {ui.enhancing ? "Regenerating…" : "Regenerate"}
                  </Button>
                </div>
              </div>
              {#if ui.enhancing}
                <div class="streaming-hint">AI is rewriting your notes…</div>
              {/if}
              {#if ui.editingActionItems}
                <section class="action-item-editor" aria-label="Edit enhanced action items">
                  <div class="action-item-editor-heading">
                    <strong>Action items</strong>
                    <span>Changes also update linked cards on Boards.</span>
                  </div>
                  <div class="action-item-fields">
                    {#each ui.actionItemDrafts as _, index}
                      <label>
                        <span>Action item {index + 1}</span>
                        <Input bind:value={ui.actionItemDrafts[index]} disabled={ui.savingActionItems} />
                      </label>
                    {/each}
                  </div>
                  {#if ui.actionItemError}<p class="action-item-edit-error" role="alert">{ui.actionItemError}</p>{/if}
                  <div class="action-item-edit-actions">
                    <Button size="sm" onclick={view.saveActionItemEdits} disabled={ui.savingActionItems}>
                      {ui.savingActionItems ? "Saving…" : "Save action items"}
                    </Button>
                    <Button variant="ghost" size="sm" onclick={view.cancelActionItemEdit} disabled={ui.savingActionItems}>Cancel</Button>
                  </div>
                </section>
              {/if}
              {#if ui.actionItemStatus}<p class="action-item-edit-status" role="status">{ui.actionItemStatus}</p>{/if}
              {@html ui.enhancedHtml}
            </div>
          {/if}
          {#if ui.enhanceError}
            <div class="test-output error" style="margin-top: 12px;">{ui.enhanceError}</div>
          {/if}
        </Tabs.Content>
        <Tabs.Content value="transcript" class="post-content">
          {#if ui.meeting.transcript.length > 0}
            <div class="raw-transcript">
              {#each ui.meeting.transcript as line (line.id ?? line.text)}
                {#if ui.editingSegmentId === line.id}
                  <div class="raw-line editing">
                    <span class="tag {line.channel}">{line.channel === "you" ? "You" : "Others"}</span>
                    <div class="segment-edit-wrap">
                      <Textarea
                        bind:value={ui.segmentText}
                        onkeydown={onSegmentKeydown}
                        disabled={ui.segmentOperation !== null}
                        rows={2}
                        class="segment-edit-textarea"
                      />
                      {#if ui.segmentError}
                        <p class="segment-edit-error" role="alert">{ui.segmentError.message}</p>
                      {/if}
                      <div class="segment-edit-actions">
                        <button
                          class="segment-action save"
                          disabled={ui.segmentOperation !== null}
                          onclick={view.saveSegmentEdit}
                        >
                          {ui.segmentOperation === "save"
                            ? "Saving…"
                            : ui.segmentError?.action === "save"
                              ? "Retry save"
                              : "Save"}
                        </button>
                        <button
                          class="segment-action"
                          disabled={ui.segmentOperation !== null}
                          onclick={view.cancelSegmentEdit}
                        >Cancel</button>
                        {#if line.id !== undefined}
                          <button
                            class="segment-action delete"
                            disabled={ui.segmentOperation !== null}
                            onclick={() => line.id !== undefined && view.deleteSegment(line.id)}
                          >
                            {ui.segmentOperation === "delete"
                              ? "Deleting…"
                              : ui.segmentError?.action === "delete"
                                ? "Retry delete"
                                : "Delete"}
                          </button>
                        {/if}
                      </div>
                    </div>
                  </div>
                {:else}
                  <button
                    type="button"
                    class="raw-line"
                    disabled={ui.segmentOperation !== null}
                    onclick={() => view.startSegmentEdit(line)}
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

  .action-item-editor {
    display: grid;
    gap: 14px;
    margin: 18px 0;
    padding: 16px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .action-item-editor-heading,
  .action-item-fields,
  .action-item-fields label {
    display: grid;
    gap: 6px;
  }
  .action-item-editor-heading span,
  .action-item-fields label > span,
  .action-item-edit-status {
    color: var(--text-muted);
    font-size: 12px;
  }
  .action-item-fields {
    gap: 12px;
  }
  .action-item-edit-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .action-item-edit-error,
  .action-item-edit-status {
    margin: 0;
  }
  .action-item-edit-error {
    color: var(--danger);
    font-size: 13px;
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
