<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { createDictationController } from "$lib/dictation-controller";
  import { getDictationState, onDictationState } from "$lib/dictation-ipc";
  import type { DictationSnapshot } from "$lib/dictation-types";

  let snapshot = $state<DictationSnapshot | null>(null);
  let inDictationSettings = $derived(page.url.pathname === "/settings" && page.url.searchParams.get("section") === "dictation");
  onMount(() => {
    const controller = createDictationController({ get: getDictationState, listen: onDictationState }, view => { snapshot = view.snapshot; });
    void controller.connect();
    return () => controller.dispose();
  });
</script>

{#if snapshot?.phase === "review" && !inDictationSettings}
  <aside class="review-notice" aria-label="Dictation review available">
    <div role="status"><strong>{snapshot.exit_intent ? "Dictation needs review before exiting" : "Dictation text is ready to review"}</strong><p>Text is retained in memory. Copy or discard it in Settings.</p></div>
    <a href="/settings?section=dictation">Review dictation</a>
  </aside>
{/if}

<style>
  .review-notice { position: fixed; z-index: 50; right: 20px; bottom: 20px; max-width: min(420px, calc(100vw - 40px)); display: grid; gap: 12px; padding: 18px; border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--canvas); box-shadow: 0 8px 28px var(--shadow-ambient); color: var(--ink); font-size: 14px; }
  p { margin: 4px 0 0; color: var(--text-muted); line-height: 1.65; }
  a { color: var(--ink); text-decoration: underline; text-underline-offset: 3px; }
  a:focus-visible { outline: 2px solid var(--ink); outline-offset: 4px; }
</style>
