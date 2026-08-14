<script lang="ts">
  import { page } from "$app/state";
  import AskBar from "$lib/components/AskBar.svelte";
  import { meetings, meetingFromRecording, type Meeting } from "$lib/mock";
  import { getLastRecording } from "$lib/recording.svelte";
  import { renderMarkdown } from "$lib/markdown";

  type Tab = "mynotes" | "enhanced" | "transcript";

  function resolveMeeting(id: string | undefined): Meeting {
    if (id === "latest") {
      const recorded = getLastRecording();
      if (recorded) return meetingFromRecording(recorded);
    }
    return (id && meetings[id]) || meetings["product-standup"];
  }

  let meeting = $derived(resolveMeeting(page.params.id));

  let tab = $state<Tab>("mynotes");
  let enhanced = $state(false);
  let notes = $state("");
  let enhancing = $state(false);

  // Reset per-meeting view state whenever the route param changes.
  $effect(() => {
    meeting.id;
    notes = meeting.notepad;
    tab = "mynotes";
    enhanced = false;
    enhancing = false;
  });

  function enhance() {
    // Simulate the LLM enhancement round-trip, then show the mock markdown.
    enhancing = true;
    setTimeout(() => {
      enhancing = false;
      enhanced = true;
    }, 900);
  }
</script>

<svelte:head>
  <title>{meeting.title} — Kiminola</title>
</svelte:head>

<div class="main-content">
  <div class="post-shell">
    <div class="post-header">
      <div class="post-back">
        <a class="btn btn-ghost" href="/">← Back to meetings</a>
      </div>
      <div class="post-title-row">
        <div class="display">{meeting.title}</div>
      </div>
      <div class="post-meta-row">
        <span class="post-meta-pill">{meeting.meta}</span>
        <span class="post-meta-pill">{meeting.spaceName}</span>
        <span class="post-meta-pill">+ Add to folder</span>
      </div>
    </div>

    <div class="pill-tabs" role="tablist">
      <button
        class="pill-tab"
        class:active={tab === "mynotes"}
        role="tab"
        aria-selected={tab === "mynotes"}
        onclick={() => (tab = "mynotes")}>My notes</button
      >
      <button
        class="pill-tab"
        class:active={tab === "enhanced"}
        role="tab"
        aria-selected={tab === "enhanced"}
        onclick={() => (tab = "enhanced")}>Enhance Notes</button
      >
      <button
        class="pill-tab"
        class:active={tab === "transcript"}
        role="tab"
        aria-selected={tab === "transcript"}
        onclick={() => (tab = "transcript")}>Transcript</button
      >
    </div>

    <div class="post-content">
      {#if tab === "mynotes"}
        <div class="my-notes-card">
          <textarea
            bind:value={notes}
            placeholder="No notes captured during this meeting."
            aria-label="My notes"
          ></textarea>
        </div>
      {:else if tab === "enhanced"}
        {#if enhanced}
          <div class="note-document">
            <!-- eslint-disable-next-line svelte/no-at-html-tags -- mock markdown, escaped in renderMarkdown -->
            {@html renderMarkdown(meeting.enhancedMarkdown)}
          </div>
        {:else}
          <div class="my-notes-card enhance-prompt">
            <div class="enhance-title">Enhance notes with AI</div>
            <div class="enhance-copy">
              Merge your notes with the transcript into a structured summary, action items, and key
              decisions.
            </div>
            <button class="btn btn-primary" onclick={enhance} disabled={enhancing}>
              {enhancing ? "Enhancing…" : "Enhance notes"}
            </button>
          </div>
        {/if}
      {:else}
        <div class="raw-transcript">
          {#each meeting.transcript as line, i (i)}
            <div class="raw-line">
              <span class="tag {line.channel}">{line.channel === "you" ? "You" : "Others"}</span>
              {line.text}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>

<AskBar placeholder="Ask anything about this meeting…" actionLabel="✉ Write follow up" />
