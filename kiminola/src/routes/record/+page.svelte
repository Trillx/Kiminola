<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import LiveTranscript from "$lib/components/LiveTranscript.svelte";
  import { liveSimulation, type TranscriptLine } from "$lib/mock";
  import { setLastRecording } from "$lib/recording.svelte";

  // Mock: a real session would carry the title from the meeting that started it.
  const title = "Product standup";

  let notepad = $state("");
  let elapsed = $state(0);
  let lines = $state<TranscriptLine[]>([]);
  let partialIndex = $state(-1);
  let transcriptOpen = $state(false);

  let timerText = $derived(
    `${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, "0")}`,
  );

  onMount(() => {
    const timer = setInterval(() => (elapsed += 1), 1000);
    const timeouts: ReturnType<typeof setTimeout>[] = [];

    // Simulate a streaming transcript: mock lines appear on a timer.
    for (const cue of liveSimulation) {
      timeouts.push(
        setTimeout(() => {
          lines = [...lines, { channel: cue.channel, text: cue.text }];
          partialIndex = lines.length - 1;
          // After a short beat, settle the line so it feels streamed.
          timeouts.push(
            setTimeout(() => {
              if (partialIndex === lines.length - 1) partialIndex = -1;
            }, 1200),
          );
        }, cue.delay),
      );
    }

    return () => {
      clearInterval(timer);
      timeouts.forEach(clearTimeout);
    };
  });

  function stopMeeting() {
    setLastRecording({
      title,
      durationText: timerText,
      notepad,
      transcript: lines.map((l) => ({ ...l })),
    });
    goto("/meeting/latest");
  }

  function cancel() {
    goto("/");
  }
</script>

<svelte:head>
  <title>Recording — Kiminola</title>
</svelte:head>

<div class="main-content recording-content" class:transcript-open={transcriptOpen}>
  <div class="recording-shell">
    <div class="recording-top">
      <div>
        <div class="recording-title">{title}</div>
        <div class="recording-meta">Recording · {timerText}</div>
      </div>
      <div class="recording-badge">Recording</div>
    </div>

    <div class="waveform" aria-hidden="true">
      <span></span><span></span><span></span><span></span><span></span>
    </div>

    <div class="notepad-hero">
      <div class="notepad-header">
        <span class="notepad-label">My notes</span>
        <span style="font-size:12px;color:var(--soft);">Sketch while you listen</span>
      </div>
      <textarea
        bind:value={notepad}
        placeholder="Jot rough thoughts, action items, or quotes here…"
      ></textarea>
    </div>

    <div class="recording-actions">
      <button class="btn btn-ghost" onclick={cancel}>Cancel</button>
      <button class="btn btn-danger" onclick={stopMeeting}>Stop meeting</button>
    </div>
  </div>
</div>

<LiveTranscript {lines} {partialIndex} bind:open={transcriptOpen} />

<style>
  .recording-content {
    transition: margin-left 200ms ease;
  }
</style>
