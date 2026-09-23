<script lang="ts">
  import { onDestroy } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { resolveDictation } from "$lib/dictation-ipc";
  import type { DictationResolution, DictationSnapshot } from "$lib/dictation-types";

  let { snapshot, refresh }: { snapshot: DictationSnapshot; refresh: () => Promise<void> } = $props();
  let busy = $state(false);
  let error = $state("");
  let disposed = false;
  let text = $derived(snapshot.text || snapshot.raw_text);
  onDestroy(() => { disposed = true; });
  async function resolve(action: DictationResolution) {
    if (busy) return;
    const session = snapshot.session_id;
    busy = true; error = "";
    try {
      await resolveDictation(action);
      if (!disposed) await refresh();
    } catch (err) {
      if (!disposed && snapshot.session_id === session) error = String(err);
      // Failed refreshes must not clear retained text.
      if (!disposed) await refresh();
    } finally { if (!disposed) busy = false; }
  }
</script>

<section class="recovery" aria-label="Dictation recovery" aria-busy={busy}>
  <header><h3>{snapshot.exit_intent === "quit" ? "Review before quitting" : snapshot.exit_intent === "disable" ? "Review before disabling dictation" : "Review dictation"}</h3><p>Capture has stopped. Resolve this text before starting another dictation. Meetings can still run.</p></header>
  {#if snapshot.error}<p role="alert">{snapshot.error}</p>{/if}
  {#if snapshot.delivery === "uncertain"}<p>Insertion was not verified. Check the destination before copying to avoid a duplicate. Kimi Nola will not retry insertion.</p>{/if}
  {#if text}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (Scrollable text must be reachable by keyboard.) -->
    <pre class="recovery-text ui-scrollbar" role="region" tabindex="0" aria-label="Retained dictation text">{text}</pre>
  {:else}<p>No text was recovered.</p>{/if}
  {#if snapshot.raw_text && snapshot.text && snapshot.raw_text !== snapshot.text}
    <details><summary>Original raw transcript</summary>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Scrollable text must be reachable by keyboard.) -->
      <pre class="recovery-text ui-scrollbar" role="region" aria-label="Raw dictation transcript" tabindex="0">{snapshot.raw_text}</pre>
    </details>
  {/if}
  <p>This recovery is held in memory only, not saved as failed-attempt history. Copy replaces your clipboard; previous clipboard contents are not restored.</p>
  <div class="actions">
    <Button disabled={busy || !text} onclick={() => resolve("copy")}>{snapshot.exit_intent === "quit" ? "Copy and quit" : snapshot.exit_intent === "disable" ? "Copy and disable" : "Copy text"}</Button>
    {#if snapshot.delivery === "uncertain" && !snapshot.exit_intent}<Button variant="outline" disabled={busy} onclick={() => resolve("confirm")}>I verified the insertion</Button>{/if}
    <Button variant="outline" disabled={busy} onclick={() => resolve("dismiss")}>{snapshot.exit_intent === "quit" ? "Discard and quit" : snapshot.exit_intent === "disable" ? "Discard and disable" : "Discard text"}</Button>
    {#if snapshot.exit_intent}<Button variant="outline" disabled={busy} onclick={() => resolve("cancel_exit")}>Cancel {snapshot.exit_intent === "quit" ? "quit" : "disable"}</Button>{/if}
  </div>
  {#if error}<p role="alert">Could not resolve dictation: {error}</p>{/if}
</section>

<style>
  .recovery { display: grid; gap: 14px; padding: 20px; background: var(--surface-soft); border-radius: var(--radius-card); color: var(--ink); }
  h3 { font: 400 24px/1.2 var(--font-display); margin: 0 0 8px; }
  p { font-size: 14px; line-height: 1.65; margin: 0; color: var(--text-muted); }
  .recovery-text { white-space: pre-wrap; overflow-wrap: anywhere; font: 400 15px/1.65 var(--font-body); margin: 0; padding: 14px; background: var(--canvas); border: 1px solid var(--hairline); border-radius: var(--radius-input); max-height: 260px; overflow-y: auto; user-select: text; }
  .actions { display: flex; gap: 10px; flex-wrap: wrap; }
  summary { cursor: pointer; font-size: 14px; margin-bottom: 10px; }
  :focus-visible { outline: 2px solid var(--ink); outline-offset: 3px; }
  @media (max-width: 600px) { .recovery { padding: 14px; } }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; animation: none !important; } }
</style>
