<script lang="ts">
  import { goto } from "$app/navigation";
  import { Input } from "$lib/components/ui/input";
  import * as Dialog from "$lib/components/ui/dialog";
  import { searchMeetings, type MeetingSummary } from "$lib/tauri";

  interface Props {
    open?: boolean;
  }

  let { open = $bindable(false) }: Props = $props();

  let query = $state("");
  let results = $state<MeetingSummary[]>([]);
  let searching = $state(false);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let inputRef = $state<HTMLInputElement | null>(null);

  function formatMeta(m: MeetingSummary): string {
    const date = new Date(m.created_at).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
    });
    const mins = Math.max(1, Math.round(m.duration_seconds / 60));
    return `${date} · ${mins} min`;
  }

  let searchError = $state("");
  let searchGeneration = 0;

  function invalidateSearch() {
    searchGeneration++;
    clearTimeout(searchTimer);
    searchTimer = undefined;
  }

  async function runSearch(q: string, generation: number) {
    try {
      const matches = await searchMeetings(q);
      if (generation === searchGeneration && open) results = matches;
    } catch (err) {
      if (generation !== searchGeneration || !open) return;
      searchError = err instanceof Error ? err.message : String(err);
      results = [];
    } finally {
      if (generation === searchGeneration && open) searching = false;
    }
  }

  function onInput() {
    invalidateSearch();
    const trimmed = query.trim();
    const generation = searchGeneration;
    results = [];
    searchError = "";
    searching = trimmed.length > 0;
    if (searching) searchTimer = setTimeout(() => runSearch(trimmed, generation), 150);
  }

  function goToMeeting(id: number) {
    open = false;
    query = "";
    results = [];
    goto(`/meeting/${id}`);
  }

  $effect(() => {
    const isOpen = open;
    invalidateSearch();
    query = "";
    results = [];
    searching = false;
    searchError = "";
    const focusTimer = isOpen ? setTimeout(() => inputRef?.focus(), 50) : undefined;
    return () => {
      clearTimeout(focusTimer);
      invalidateSearch();
    };
  });
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>Search meetings</Dialog.Title>
    </Dialog.Header>
    <div class="search-input-wrap">
      <Input
        bind:ref={inputRef}
        type="text"
        aria-label="Search titles, notes, and transcripts"
        placeholder="Search titles, notes, transcripts…"
        bind:value={query}
        oninput={onInput}
        class="search-input"
      />
    </div>
    <div class="search-results">
      {#if searchError}
        <div class="search-empty" role="alert">Search failed: {searchError}</div>
        <button class="search-result" onclick={onInput}>Try again</button>
      {:else if searching && results.length === 0}
        <div class="search-empty" role="status">Searching…</div>
      {:else if query.trim() && results.length === 0}
        <div class="search-empty">No meetings found.</div>
      {:else}
        {#each results as meeting (meeting.id)}
          <button class="search-result" onclick={() => goToMeeting(meeting.id)}>
            <div class="search-result-title">{meeting.title}</div>
            <div class="search-result-meta">
              <span>{formatMeta(meeting)}</span>
              {#if meeting.location_path ?? meeting.space_name}
                <span>· {meeting.location_path ?? meeting.space_name}</span>
              {/if}
            </div>
          </button>
        {/each}
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<style>
  .search-input-wrap {
    margin: 12px 0;
  }
  :global(.search-input) {
    width: 100%;
  }
  .search-results {
    max-height: 320px;
    overflow-y: auto;
  }
  .search-empty {
    padding: 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 14px;
  }
  .search-result {
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: var(--radius-input);
    padding: 10px 12px;
    cursor: pointer;
    color: var(--ink);
  }
  .search-result:hover {
    background: var(--surface);
  }
  .search-result-title {
    font-weight: 500;
    margin-bottom: 2px;
  }
  .search-result-meta {
    font-size: 13px;
    color: var(--text-muted);
  }
</style>
