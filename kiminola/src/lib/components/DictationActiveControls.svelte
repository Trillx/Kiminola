<script lang="ts">
  import { onDestroy } from "svelte";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { stopDictation, cancelDictation } from "$lib/dictation-ipc";
  import type { DictationSnapshot } from "$lib/dictation-types";

  let { snapshot, refresh, compact = false }: { snapshot: DictationSnapshot; refresh: () => Promise<void>; compact?: boolean } = $props();
  let stopping = $state(false);
  let cancelling = $state(false);
  let error = $state("");
  let disposed = false;
  let version = 0;
  onDestroy(() => { disposed = true; });
  async function act(action: "stop" | "cancel") {
    if (cancelling || (action === "stop" && stopping)) return;
    const request = ++version;
    const session = snapshot.session_id;
    if (action === "stop") stopping = true; else cancelling = true;
    error = "";
    try {
      await (action === "stop" ? stopDictation() : cancelDictation());
      if (!disposed) await refresh();
    } catch (err) {
      if (!disposed && version === request && snapshot.session_id === session) error = String(err);
    } finally {
      if (!disposed) { if (action === "stop") stopping = false; else cancelling = false; }
    }
  }
</script>

<div class="controls" class:compact>
  {#if snapshot.phase === "listening"}<button aria-label="Stop dictation" title="Stop and finalize dictation" disabled={stopping || cancelling} onclick={() => act("stop")}>{#if compact}<Square size={14} aria-hidden="true" />{:else}{stopping ? "Stopping…" : "Stop dictation"}{/if}</button>{/if}
  <button aria-label="Cancel dictation" title="Cancel and discard this dictation" disabled={cancelling} onclick={() => act("cancel")}>{#if compact}<X size={16} aria-hidden="true" />{:else}{cancelling ? "Cancelling…" : "Cancel dictation"}{/if}</button>
  {#if error}<span role="alert" class:compact-error={compact} title={error}>{compact ? "Error" : `Could not complete dictation action: ${error}`}</span>{/if}
</div>

<style>
  .controls { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  button { min-height: 36px; padding: 8px 14px; border: 1px solid var(--hairline); border-radius: var(--radius-input); color: var(--ink); background: var(--surface); font: 500 14px/1.2 var(--font-body); cursor: pointer; }
  button:hover { color: var(--canvas); background: var(--ink); }
  button:disabled { opacity: .5; cursor: wait; }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .compact { flex-direction: column; flex-wrap: nowrap; }
  .compact button { display: grid; place-items: center; width: 32px; height: 32px; min-height: 32px; flex: none; padding: 0; border-radius: 50%; }
  .compact-error { font: 400 9px/1.2 var(--font-mono); text-transform: uppercase; }
  @media (prefers-reduced-motion: reduce) { * { animation: none !important; transition: none !important; } }
</style>
