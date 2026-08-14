<script lang="ts">
  import type { TranscriptLine } from "$lib/mock";

  let {
    lines,
    partialIndex = -1,
    open = $bindable(false),
  }: {
    lines: TranscriptLine[];
    /** Index of the line currently showing the streaming cursor (-1 = none). */
    partialIndex?: number;
    open?: boolean;
  } = $props();

  let bodyEl: HTMLDivElement | undefined = $state();
  let scrollHideTimer: ReturnType<typeof setTimeout> | undefined;

  // Auto-scroll to new lines, but only when the user is already near the bottom.
  $effect(() => {
    lines.length;
    if (!bodyEl) return;
    const nearBottom = bodyEl.scrollHeight - bodyEl.scrollTop - bodyEl.clientHeight < 60;
    if (nearBottom) bodyEl.scrollTop = bodyEl.scrollHeight;
  });

  // Show the scrollbar only while the user is actively scrolling.
  function onScroll() {
    if (!bodyEl) return;
    bodyEl.classList.add("show-scroll");
    clearTimeout(scrollHideTimer);
    scrollHideTimer = setTimeout(() => bodyEl?.classList.remove("show-scroll"), 1200);
  }
</script>

{#if !open}
  <button class="transcript-indicator" onclick={() => (open = true)}>
    <span class="dot"></span>
    <span>Live transcript</span>
  </button>
{:else}
  <div class="transcript-floating">
    <div class="floating-header">
      <span class="floating-title">Live transcript</span>
      <button class="floating-close" onclick={() => (open = false)} aria-label="Close transcript">×</button>
    </div>
    <div class="floating-body" bind:this={bodyEl} onscroll={onScroll}>
      {#each lines as line, i (i)}
        <div class="transcript-line" class:partial={i === partialIndex}>
          <div class="speaker {line.channel}">{line.channel === "you" ? "You" : "Others"}</div>
          <div class="text">{line.text}</div>
        </div>
      {/each}
    </div>
  </div>
{/if}
