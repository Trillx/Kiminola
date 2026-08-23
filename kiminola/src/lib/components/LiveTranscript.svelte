<script lang="ts">
  import type { TranscriptLine } from "$lib/tauri";

  let {
    lines,
    open = $bindable(false),
  }: {
    lines: TranscriptLine[];
    open?: boolean;
  } = $props();

  let bodyEl: HTMLDivElement | undefined = $state();
  let sheetEl: HTMLDivElement | undefined = $state();
  let scrollHideTimer: ReturnType<typeof setTimeout> | undefined;

  let latest = $derived(lines[lines.length - 1]);
  let latestPartial = $derived(latest?.is_partial === true);

  // Clicking anywhere outside the sheet (e.g. back into the notepad) drops it
  // back to the strip; Escape does the same.
  $effect(() => {
    if (!open) return;
    function onPointerDown(e: PointerEvent) {
      if (sheetEl && !sheetEl.contains(e.target as Node)) {
        open = false;
      }
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") open = false;
    }
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  });

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

{#if !open && lines.length === 0}
  <!-- Nothing heard yet: quiet indicator only -->
  <button class="transcript-indicator" onclick={() => (open = true)}>
    <span class="dot"></span>
    <span>Live transcript</span>
  </button>
{:else if !open}
  <!-- Live strip: the latest line, one click away from the full sheet -->
  <button class="transcript-strip" onclick={() => (open = true)} aria-label="Open live transcript">
    <span class="dot"></span>
    <span class="strip-who">{latest.channel === "you" ? "You" : "Others"}</span>
    <span class="strip-text" class:partial={latestPartial}>{latest.text}</span>
    <span class="strip-count">{lines.length} {lines.length === 1 ? "line" : "lines"}</span>
  </button>
{:else}
  <div class="transcript-sheet" bind:this={sheetEl}>
    <div class="sheet-header">
      <span class="sheet-title">Live transcript</span>
      <button class="sheet-close" onclick={() => (open = false)} aria-label="Close transcript">×</button>
    </div>
    <div class="sheet-body" bind:this={bodyEl} onscroll={onScroll}>
      {#each lines as line, i (line.id ?? line.utterance_id ?? i)}
        <div class="transcript-line" class:partial={line.is_partial === true}>
          <div class="speaker {line.channel}">{line.channel === "you" ? "You" : "Others"}</div>
          <div class="text">{line.text}</div>
        </div>
      {/each}
    </div>
  </div>
{/if}
