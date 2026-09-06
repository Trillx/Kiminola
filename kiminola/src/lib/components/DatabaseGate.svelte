<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { Button } from "$lib/components/ui/button";

  interface Status {
    ready: boolean;
    error: string | null;
    database_path: string;
    backup_directory: string;
    backups: string[];
    backup_error: string | null;
  }
  let { children, onready }: { children: Snippet; onready: () => void } = $props();
  let status = $state<Status | null>(null);
  let busy = $state(true);
  let error = $state("");
  let backup = $state("");
  let confirming = $state(false);

  onMount(async () => {
    if (!isTauri()) {
      status = { ready: true, error: null, database_path: "", backup_directory: "", backups: [], backup_error: null };
      busy = false;
      onready();
      return;
    }
    try {
      status = await invoke<Status>("database_status");
      backup = status.backups[0] ?? "";
      if (status.ready) onready();
    } catch (cause) { error = String(cause); }
    finally { busy = false; }
  });

  async function retry() {
    busy = true;
    error = "";
    try {
      status = await invoke<Status>("retry_database");
      if (!status.backups.includes(backup)) backup = status.backups[0] ?? "";
    }
    catch (cause) { error = String(cause); }
    finally { busy = false; }
  }

  async function restore() {
    if (!backup) return;
    busy = true;
    error = "";
    try { await invoke("restore_database_backup", { backup }); }
    catch (cause) { error = String(cause); }
    finally { busy = false; confirming = false; }
  }

  async function showFiles() {
    if (!status?.database_path) return;
    try { await revealItemInDir(status.database_path); }
    catch (cause) { error = String(cause); }
  }
</script>

{#if status?.ready}
  {@render children()}
{:else}
  <main class="database-recovery" aria-busy={busy}>
    <section>
      <p class="eyebrow">Kimi Nola</p>
      <h1>{busy ? "Opening your notes…" : "Your notes need recovery"}</h1>
      {#if !busy || status?.error || error}
        <p>The app could not safely open your database. Your existing files have been kept. Recording and editing are paused.</p>
        <p class="failure" role="alert">{error || status?.error}</p>
        {#if status?.database_path}
          <p class="file-path">{status.database_path}</p>
        {/if}
        <div class="actions">
          <Button onclick={retry} disabled={busy}>Retry and restart</Button>
          <Button variant="outline" onclick={showFiles} disabled={busy || !status?.database_path}>Show data files</Button>
        </div>
        {#if status?.backups.length}
          <label for="database-backup">Restore a migration backup</label>
          <select id="database-backup" bind:value={backup} disabled={busy} onchange={() => { confirming = false; }}>
            {#each status.backups as name}<option value={name}>{name}</option>{/each}
          </select>
          <p>Restoring returns your notes to this backup. Later changes will not appear in the restored library. The current database and its recovery files are kept in a separate folder.</p>
          {#if confirming}
            <p>Restore the selected backup and restart Kimi Nola?</p>
            <div class="actions">
              <Button onclick={restore} disabled={busy}>Restore and restart</Button>
              <Button variant="ghost" onclick={() => { confirming = false; }} disabled={busy}>Cancel</Button>
            </div>
          {:else}
            <Button variant="outline" onclick={() => { confirming = true; }} disabled={busy || !backup}>Review restore</Button>
          {/if}
        {:else if status?.backup_error}
          <p role="alert">{status.backup_error}</p>
        {:else if status}
          <p>No migration backup is available to select. Keep these files and retry with a corrected app version.</p>
        {/if}
      {/if}
    </section>
  </main>
{/if}

<style>
  .database-recovery { min-height: 100vh; padding: 48px 24px; display: grid; place-items: center; background: var(--paper); color: var(--ink); }
  section { width: min(100%, 640px); }
  h1 { font-family: var(--font-display); font-size: 32px; margin: 12px 0 20px; }
  p { line-height: 1.6; margin: 12px 0; }
  .eyebrow { font-family: var(--font-mono); letter-spacing: .12em; text-transform: uppercase; font-size: 11px; }
  .failure, .file-path { overflow-wrap: anywhere; }
  .file-path { font-family: var(--font-mono); font-size: 12px; color: var(--text-muted); }
  .actions { display: flex; flex-wrap: wrap; gap: 12px; margin: 20px 0; }
  label { display: block; margin-top: 28px; margin-bottom: 8px; }
  select { width: 100%; padding: 10px; border: 1px solid var(--hairline); background: var(--surface); color: var(--ink); }
</style>
