<script lang="ts">
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import {
    deleteNoteDraft,
    getNoteDraft,
    updateNoteDraft,
    type NoteDraftDetail,
  } from "$lib/tauri";
  import { Button } from "$lib/components/ui/button";
  import { Textarea } from "$lib/components/ui/textarea";

  let draft = $state<NoteDraftDetail | null>(null);
  let notes = $state("");
  let loaded = $state(false);
  let saving = $state(false);
  let status = $state("");
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  function durationLabel(seconds: number): string {
    const minutes = Math.floor(seconds / 60);
    const remainder = seconds % 60;
    return `${minutes}:${String(remainder).padStart(2, "0")}`;
  }

  $effect(() => {
    const id = Number(page.params.id);
    if (!Number.isFinite(id)) return;
    draft = null;
    notes = "";
    loaded = false;
    getNoteDraft(id)
      .then((next) => {
        draft = next;
        notes = next.raw_markdown;
      })
      .catch((err) => console.error("Failed to load note draft:", err))
      .finally(() => (loaded = true));
  });

  function onNotesInput() {
    if (!draft || draft.meeting_id !== null) return;
    status = "Saving…";
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      try {
        saving = true;
        await updateNoteDraft(draft!.id, notes);
        status = "Saved";
      } catch (err) {
        status = "Could not save";
        console.error("Failed to save note draft:", err);
      } finally {
        saving = false;
      }
    }, 500);
  }

  async function startRecording() {
    if (!draft) return;
    clearTimeout(saveTimer);
    if (notes !== draft.raw_markdown) {
      await updateNoteDraft(draft.id, notes);
    }
    await goto(`/record?draft=${draft.id}`);
  }

  async function removeDraft() {
    if (!draft || !window.confirm("Delete this note draft?")) return;
    clearTimeout(saveTimer);
    await deleteNoteDraft(draft.id);
    await goto("/");
  }
</script>

<svelte:head>
  <title>{draft?.title ?? "Note draft"} — Kimi Nola</title>
</svelte:head>

<div class="main-content">
  <div class="post-shell note-shell">
    <div class="post-back">
      <Button variant="ghost" size="sm" href="/">← Back to meetings</Button>
    </div>

    {#if !loaded}
      <div class="empty-state">Loading note draft…</div>
    {:else if !draft}
      <div class="empty-state">Note draft not found.</div>
    {:else}
      <div class="note-header">
        <div>
          <div class="display note-title">{draft.title}</div>
          <div class="post-meta-pill">
            {draft.recovery_transcript.length > 0 ? "Recovered recording" : "Note draft"}
          </div>
        </div>
        <div class="note-actions">
          {#if draft.meeting_id === null}
            <Button onclick={startRecording}>
              {draft.recovery_transcript.length > 0 ? "Continue recording" : "Start recording"}
            </Button>
          {/if}
          <Button variant="outline" onclick={removeDraft}>Delete</Button>
        </div>
      </div>

      {#if draft.meeting_id !== null}
        <div class="note-attached">This draft is attached to a meeting.</div>
      {/if}

      <div class="my-notes-card draft-card">
        <div class="notepad-header">
          <span class="notepad-label">My notes</span>
          <span class="save-status">{saving ? "Saving…" : status}</span>
        </div>
        <Textarea
          bind:value={notes}
          oninput={onNotesInput}
          disabled={draft.meeting_id !== null}
          placeholder="Jot rough thoughts, action items, or quotes here…"
          aria-label="Note draft"
          class="draft-textarea"
        />
      </div>

      {#if draft.recovery_transcript.length > 0}
        <section class="recovery-transcript" aria-labelledby="recovery-transcript-title">
          <div class="recovery-transcript-header">
            <div>
              <div id="recovery-transcript-title" class="notepad-label">Recovered transcript</div>
              <div class="recovery-hint">
                Continue recording to turn this recovery copy into a saved meeting.
              </div>
            </div>
            <span class="recovery-duration">{durationLabel(draft.recovery_duration_seconds)}</span>
          </div>
          <div class="recovery-lines">
            {#each draft.recovery_transcript as line}
              <div class="recovery-line">
                <span class="recovery-speaker">{line.channel === "you" ? "You" : "Others"}</span>
                <span>{line.text}</span>
              </div>
            {/each}
          </div>
        </section>
      {/if}
    {/if}
  </div>
</div>

<style>
  .note-shell {
    max-width: 860px;
  }

  .note-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 20px;
    margin-top: 12px;
  }

  .note-title {
    font-size: 30px;
  }

  .note-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .note-attached {
    margin-top: 16px;
    color: var(--text-muted);
    font-size: 13px;
  }

  .draft-card {
    display: flex;
    flex-direction: column;
    min-height: 420px;
    margin-top: 28px;
  }

  :global(.draft-textarea) {
    flex: 1;
    min-height: 320px;
    resize: vertical;
    border: none;
    background: transparent;
    padding: 0;
    font: inherit;
    font-size: 16px;
    line-height: 1.65;
    color: var(--ink);
  }

  :global(.draft-textarea:focus) {
    box-shadow: none;
    border: none;
    outline: none;
  }

  :global(.draft-textarea::placeholder) {
    color: var(--soft);
  }

  .save-status {
    color: var(--soft);
    font-size: 12px;
  }

  .recovery-transcript {
    margin-top: 20px;
    padding: 20px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-card);
    background: var(--surface);
  }

  .recovery-transcript-header {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
  }

  .recovery-hint,
  .recovery-duration {
    margin-top: 3px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .recovery-duration {
    margin-top: 0;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  .recovery-lines {
    display: grid;
    gap: 12px;
  }

  .recovery-line {
    display: grid;
    grid-template-columns: 56px 1fr;
    gap: 12px;
    color: var(--ink);
    font-size: 14px;
    line-height: 1.5;
  }

  .recovery-speaker {
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 600;
  }

  @media (max-width: 680px) {
    .note-header {
      flex-direction: column;
    }
  }
</style>
