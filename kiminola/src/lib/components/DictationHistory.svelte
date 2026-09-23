<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { listDictationHistory, deleteDictationHistory } from "$lib/dictation-ipc";
  import type { DictationHistoryEntry } from "$lib/dictation-types";

  let entries = $state<DictationHistoryEntry[]>([]);
  let error = $state("");
  let busy = $state(false);
  let loading = $state(true);
  let confirmClear = $state(false);
  let disposed = false;
  let request = 0;
  let status = $state("");
  let clearButton: HTMLButtonElement | null = $state(null);
  async function refresh() {
    const revision = ++request;
    loading = true; error = "";
    try { const next = await listDictationHistory(); if (!disposed && request === revision) entries = next; }
    catch (err) { if (!disposed && request === revision) error = String(err); }
    finally { if (!disposed && request === revision) loading = false; }
  }
  onMount(() => { void refresh(); return () => { disposed = true; request++; }; });
  async function remove(id: number | null) {
    if (busy) return;
    busy = true; error = ""; status = "";
    try {
      await deleteDictationHistory(id);
      if (disposed) return;
      await refresh();
      if (!disposed && !error) { confirmClear = false; status = id === null ? "History cleared." : "History entry deleted."; clearButton?.focus(); }
    } catch (err) { if (!disposed) error = String(err); }
    finally { if (!disposed) busy = false; }
  }
  function displayDate(value: string) {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    const now = new Date();
    // Compare local calendar dates, not elapsed 24-hour periods across DST.
    const day = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
    const today = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
    const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" }).format(Math.round((day - today) / 86_400_000), "day");
    return `${relative} · ${date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}`;
  }
</script>

<section class="settings-card history" aria-label="Dictation history" aria-busy={busy || loading}>
  <header><h2>Dictation history</h2><p>Completed final text, stored locally for up to 30 days when enabled. Existing entries remain visible when new saves are off.</p></header>
  <div class="actions"><Button variant="outline" disabled={busy || loading} onclick={refresh}>Refresh history</Button><Button variant="outline" bind:ref={clearButton} disabled={busy || loading || entries.length === 0} onclick={() => confirmClear = true}>Clear all history</Button></div>
  {#if confirmClear}
    <div class="confirmation" role="group" aria-label="Confirm history deletion"><p>Delete all dictation history? This cannot be undone.</p><div class="actions"><Button disabled={busy} onclick={() => remove(null)}>Delete all entries</Button><Button variant="outline" disabled={busy} onclick={() => { confirmClear = false; clearButton?.focus(); }}>Keep history</Button></div></div>
  {/if}
  {#if loading}<p role="status">Loading history…</p>{/if}
  {#if entries.length}
    <ul>{#each entries as entry (entry.id)}<li><div class="entry-heading"><time datetime={entry.created_at}>{displayDate(entry.created_at)}</time><Button variant="outline" size="sm" disabled={busy} aria-label="Delete history entry" onclick={() => remove(entry.id)}>Delete</Button></div><pre>{entry.text}</pre></li>{/each}</ul>
  {:else if !loading && !error}<p>No completed dictation history.</p>{/if}
  {#if error}<p role="alert">Could not update dictation history: {error}</p>{/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>

<style>
  .history { margin-top: 20px; display: grid; gap: 18px; color: var(--ink); }
  h2 { font: 400 27px/1.2 var(--font-display); margin: 0 0 8px; }
  p { margin: 0; color: var(--text-muted); font-size: 14px; line-height: 1.65; }
  .actions, .entry-heading { display: flex; gap: 10px; flex-wrap: wrap; align-items: center; }
  .entry-heading { justify-content: space-between; }
  .confirmation { display: grid; gap: 12px; padding: 16px; background: var(--surface-soft); border-radius: var(--radius-input); }
  ul { list-style: none; padding: 0; margin: 0; display: grid; gap: 14px; }
  li { border-top: 1px solid var(--hairline); padding-top: 14px; min-width: 0; }
  time { font: 400 10px/1.5 var(--font-mono); text-transform: uppercase; letter-spacing: .1em; color: var(--text-muted); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font: 400 14px/1.65 var(--font-body); margin: 10px 0 0; user-select: text; }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; animation: none !important; } }
</style>
