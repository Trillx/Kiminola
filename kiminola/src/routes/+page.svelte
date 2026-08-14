<script lang="ts">
  import AskBar from "$lib/components/AskBar.svelte";
  import { recentMeetings } from "$lib/mock";

  const now = new Date();
  const month = now.toLocaleString("en-US", { month: "long" });
  const weekday = now.toLocaleString("en-US", { weekday: "short" });
  const day = now.getDate();
</script>

<svelte:head>
  <title>Kiminola</title>
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
    <div class="calendar-cta">
      <p><strong>Link your calendar</strong><br />Connect Google or Microsoft to see upcoming meetings</p>
      <button class="btn btn-ghost" style="font-size:13px;padding:6px 12px;">Link calendar</button>
    </div>
  </div>

  <div class="section-title">Recent meetings</div>
  <div class="meeting-list">
    {#each recentMeetings as meeting (meeting.id)}
      <a class="meeting-item" href="/meeting/{meeting.id}">
        <div class="doc-icon">📝</div>
        <div class="details">
          <div class="title">{meeting.title}</div>
          <div class="meta">{meeting.meta}</div>
        </div>
        <div class="time">{meeting.time}</div>
      </a>
    {/each}
  </div>
</div>

<AskBar placeholder="Ask anything across your meetings…" actionLabel="List recent todos" />
