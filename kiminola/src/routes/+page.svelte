<script lang="ts">
  import { onMount } from "svelte";
  import { listMeetings, type MeetingSummary } from "$lib/tauri";

  const now = new Date();
  const month = now.toLocaleString("en-US", { month: "long" });
  const weekday = now.toLocaleString("en-US", { weekday: "short" });
  const day = now.getDate();

  let meetings = $state<MeetingSummary[]>([]);
  let loaded = $state(false);

  onMount(async () => {
    try {
      meetings = await listMeetings();
    } catch (err) {
      console.error("Failed to load meetings:", err);
    } finally {
      loaded = true;
    }
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
  {#if loaded && meetings.length === 0}
    <div class="empty-state">
      No meetings yet — hit <strong>+ New meeting</strong> up top to record your first one.
    </div>
  {:else}
    <div class="meeting-list">
      {#each meetings as meeting (meeting.id)}
        <a class="meeting-item" href="/meeting/{meeting.id}">
          <div class="doc-icon">📝</div>
          <div class="details">
            <div class="title">{meeting.title}</div>
            <div class="meta">{meeting.space_name ?? ""}</div>
          </div>
          <div class="time">{dayLabel(meeting.created_at)}</div>
        </a>
      {/each}
    </div>
  {/if}
</div>
