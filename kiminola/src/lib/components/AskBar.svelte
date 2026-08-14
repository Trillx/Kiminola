<script lang="ts">
  let {
    placeholder = "Ask anything across your meetings…",
    actionLabel,
  }: {
    placeholder?: string;
    actionLabel?: string;
  } = $props();

  let query = $state("");

  function submit(event: SubmitEvent) {
    event.preventDefault();
    // Mock only — the Ask flow lands with the ChatProvider backend seam.
    query = "";
  }
</script>

<form class="bottom-bar" onsubmit={submit}>
  <button type="button" class="mic-btn" title="Dictate" aria-label="Dictate">🎙</button>
  <input type="text" {placeholder} bind:value={query} />
  {#if actionLabel}
    <button type="submit" class="action-btn">{actionLabel}</button>
  {/if}
</form>
<div class="ai-disclaimer">Kiminola uses AI and can make mistakes.</div>
