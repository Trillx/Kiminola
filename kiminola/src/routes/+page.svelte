<script lang="ts">
  import { onMount } from "svelte";
  import { listMeetings, listNoteDrafts, type MeetingSummary, type NoteDraftSummary } from "$lib/tauri";
  import { loadLibraryData } from "$lib/library-loading";

  const now = new Date();
  const month = now.toLocaleString("en-US", { month: "long" });
  const weekday = now.toLocaleString("en-US", { weekday: "short" });
  const day = now.getDate();

  let meetings = $state<MeetingSummary[]>([]);
  let drafts = $state<NoteDraftSummary[]>([]);
  let loaded = $state(false);
  let loading = $state(false);
  let meetingLoadError = $state<string | null>(null);
  let draftsLoadError = $state<string | null>(null);

  function errorMessage(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === "string") return error;
    return "The database request failed.";
  }

  async function loadLibrary() {
    loading = true;
    const result = await loadLibraryData({
      loadMeetings: listMeetings,
      loadNoteDrafts: listNoteDrafts,
    });
    meetings = result.meetings;
    drafts = result.drafts;
    meetingLoadError = result.meetingsError ? errorMessage(result.meetingsError) : null;
    draftsLoadError = result.draftsError ? errorMessage(result.draftsError) : null;
    if (result.meetingsError) {
      console.error("Failed to load meetings:", result.meetingsError);
    }
    if (result.draftsError) {
      console.error("Failed to load note drafts:", result.draftsError);
    }
    loaded = true;
    loading = false;
  }

  onMount(() => {
    void loadLibrary();
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") void loadLibrary();
    };
    window.addEventListener("focus", refreshWhenVisible);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.removeEventListener("focus", refreshWhenVisible);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  });

  function dayLabel(iso: string): string {
    const date = new Date(iso);
    const today = new Date();
    const yesterday = new Date();
    yesterday.setDate(today.getDate() - 1);
    if (date.toDateString() === today.toDateString()) return "Today";
    if (date.toDateString() === yesterday.toDateString()) return "Yesterday";
    return date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
  }
</script>

<svelte:head>
  <title>Kimi Nola</title>
</svelte:head>

<div class="main-content">
  <div class="display" style="margin-bottom:24px;">Coming up</div>

  <div class="date-card">
    <div>
      <div class="date-number">{day}</div>
    </div>
    <div class="date-label">
      <strong>{month}</strong>
      <span>{weekday}</span>
    </div>
  </div>

  <div class="section-title">Recent meetings</div>
  {#if loaded && meetingLoadError}
    <div class="empty-state" role="alert">
      <div>Could not load saved meetings.</div>
      <div style="margin-top: 8px; font-size: 13px;">{meetingLoadError}</div>
      <button class="btn btn-primary btn-sm" style="margin-top: 16px;" onclick={() => void loadLibrary()} disabled={loading}>
        {loading ? "Retrying…" : "Try again"}
      </button>
    </div>
  {:else if loaded && meetings.length === 0 && drafts.length === 0 && !draftsLoadError}
    <div class="empty-state">
      No meetings yet — hit <strong>+ New meeting</strong> up top to record your first one.
    </div>
  {:else}
    {#if draftsLoadError}
      <div class="empty-state" role="status" style="margin-bottom: 16px;">
        Saved meetings loaded, but note drafts could not be loaded: {draftsLoadError}
      </div>
    {/if}
    <div class="meeting-list">
      {#each meetings as meeting (meeting.id)}
        <a class="meeting-item" href="/meeting/{meeting.id}">
          <div class="doc-icon">📝</div>
          <div class="details">
            <div class="title">{meeting.title}</div>
            <div class="meta">{meeting.location_path ?? meeting.space_name ?? ""}</div>
          </div>
          <div class="time">{dayLabel(meeting.created_at)}</div>
        </a>
      {/each}
    </div>
  {/if}

  {#if drafts.length > 0}
    <div class="section-title">Note drafts</div>
    <div class="meeting-list">
      {#each drafts as draft (draft.id)}
        <a class="meeting-item" href="/note/{draft.id}">
          <div class="doc-icon">✎</div>
          <div class="details">
            <div class="title">{draft.title}</div>
            <div class="meta">Standalone notes</div>
          </div>
          <div class="time">{dayLabel(draft.updated_at)}</div>
        </a>
      {/each}
    </div>
  {/if}
</div>
