<script lang="ts">
  import { onMount } from "svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import Pencil from "@lucide/svelte/icons/pencil";
  import { listBoards, createBoard, renameBoard, createBoardColumn, renameBoardColumn, moveBoardCard, type Board, type BoardColumn, type BoardsSnapshot } from "$lib/tauri";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";

  let snapshot = $state<BoardsSnapshot | null>(null);
  let activeBoardId = $state<number | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state<string | null>(null);

  let newBoardName = $state("");
  let newColumnName = $state("");
  let editingBoard = $state(false);
  let boardNameDraft = $state("");
  let editingColumnId = $state<number | null>(null);
  let columnNameDraft = $state("");

  let activeBoard = $derived(snapshot?.boards.find((board) => board.id === activeBoardId) ?? null);

  function errorMessage(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }

  async function loadBoards() {
    loading = true;
    error = null;
    try {
      const next = await listBoards();
      snapshot = next;
      if (!next.boards.some((board) => board.id === activeBoardId)) {
        activeBoardId = next.boards[0]?.id ?? null;
      }
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void loadBoards();
  });

  async function createNewBoard() {
    const name = newBoardName.trim();
    if (!name || busy) return;
    busy = true;
    error = null;
    try {
      const id = await createBoard(name);
      newBoardName = "";
      await loadBoards();
      activeBoardId = id;
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      busy = false;
    }
  }

  function beginBoardRename(board: Board) {
    editingBoard = true;
    boardNameDraft = board.name;
    error = null;
  }

  function cancelBoardRename() {
    editingBoard = false;
    boardNameDraft = "";
  }

  async function saveBoardName() {
    if (!activeBoard || busy) return;
    const name = boardNameDraft.trim();
    if (!name) {
      error = "Board name cannot be empty.";
      return;
    }
    busy = true;
    error = null;
    try {
      await renameBoard(activeBoard.id, name);
      cancelBoardRename();
      await loadBoards();
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      busy = false;
    }
  }

  async function addColumn() {
    if (!activeBoard || busy) return;
    const name = newColumnName.trim();
    if (!name) return;
    busy = true;
    error = null;
    try {
      await createBoardColumn(activeBoard.id, name);
      newColumnName = "";
      await loadBoards();
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      busy = false;
    }
  }

  function beginColumnRename(column: BoardColumn) {
    editingColumnId = column.id;
    columnNameDraft = column.name;
    error = null;
  }

  function cancelColumnRename() {
    editingColumnId = null;
    columnNameDraft = "";
  }

  async function saveColumnName(columnId: number) {
    if (busy) return;
    const name = columnNameDraft.trim();
    if (!name) {
      error = "Column name cannot be empty.";
      return;
    }
    busy = true;
    error = null;
    try {
      await renameBoardColumn(columnId, name);
      cancelColumnRename();
      await loadBoards();
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      busy = false;
    }
  }

  async function moveCard(cardId: number, columnId: number, select: HTMLSelectElement, persistedColumnId: number) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await moveBoardCard(cardId, columnId);
      await loadBoards();
    } catch (cause) {
      select.value = String(persistedColumnId);
      error = errorMessage(cause);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head>
  <title>Boards — Kimi Nola</title>
</svelte:head>

<div class="main-content boards-page">
  <header class="boards-header">
    <div>
      <span class="eyebrow">Action items</span>
      <h1 class="display">Boards</h1>
      <p class="boards-copy">Keep meeting follow-ups visible, organized, and moving.</p>
    </div>
    <form class="new-board-form" onsubmit={(event) => { event.preventDefault(); void createNewBoard(); }}>
      <Input bind:value={newBoardName} aria-label="New board name" placeholder="New board name" disabled={busy} />
      <Button type="submit" disabled={!newBoardName.trim() || busy}><Plus size={15} aria-hidden="true" /> New board</Button>
    </form>
  </header>

  {#if snapshot?.created_default}
    <div class="board-welcome" role="status">
      <strong>Your To-Do's board is ready.</strong>
      <span>You can create boards to track these actions here.</span>
    </div>
  {/if}

  {#if error}
    <div class="board-error" role="alert">
      <span>{error}</span>
      <Button variant="outline" size="sm" onclick={() => void loadBoards()} disabled={loading}>Retry</Button>
    </div>
  {/if}

  {#if loading && !snapshot}
    <div class="empty-state" role="status">Loading boards…</div>
  {:else if snapshot}
    <div class="boards-layout">
      <aside class="board-list" aria-label="Boards">
        <div class="board-list-heading">
          <span>Your boards</span>
          <span class="mono">{snapshot.boards.length}</span>
        </div>
        <div class="board-list-items">
          {#each snapshot.boards as board (board.id)}
            <button
              type="button"
              class:active={board.id === activeBoardId}
              class="board-list-item"
              aria-current={board.id === activeBoardId ? "page" : undefined}
              onclick={() => { activeBoardId = board.id; cancelBoardRename(); cancelColumnRename(); }}
            >
              <span>{board.name}</span>
              <small>{board.columns.reduce((total, column) => total + column.cards.length, 0)}</small>
            </button>
          {/each}
        </div>
      </aside>

      {#if activeBoard}
        <section class="board-workspace" aria-label={`${activeBoard.name} board`}>
          <header class="board-workspace-header">
            <div>
              {#if editingBoard}
                <form class="rename-board-form" onsubmit={(event) => { event.preventDefault(); void saveBoardName(); }}>
                  <Input bind:value={boardNameDraft} aria-label="Board name" disabled={busy} />
                  <Button size="sm" type="submit" disabled={busy}>Save</Button>
                  <Button size="sm" variant="ghost" type="button" onclick={cancelBoardRename} disabled={busy}>Cancel</Button>
                </form>
              {:else}
                <button class="board-title-button" type="button" onclick={() => beginBoardRename(activeBoard)}>
                  <h2>{activeBoard.name}</h2>
                  <Pencil size={15} aria-hidden="true" />
                </button>
              {/if}
              <p>{activeBoard.columns.length} {activeBoard.columns.length === 1 ? "column" : "columns"} · {activeBoard.columns.reduce((total, column) => total + column.cards.length, 0)} action items</p>
            </div>
            <a class="board-home-link" href="/">Back to meetings</a>
          </header>

          <div class="column-toolbar">
            <form class="new-column-form" onsubmit={(event) => { event.preventDefault(); void addColumn(); }}>
              <Input bind:value={newColumnName} aria-label="New column name" placeholder="New column name" disabled={busy} />
              <Button type="submit" variant="outline" disabled={!newColumnName.trim() || busy}><Plus size={14} aria-hidden="true" /> Add column</Button>
            </form>
          </div>

          <div class="kanban-grid" aria-label="Kanban columns">
            {#each activeBoard.columns as column (column.id)}
              <article class="kanban-column" aria-label={column.name}>
                <header class="column-header">
                  {#if editingColumnId === column.id}
                    <form class="rename-column-form" onsubmit={(event) => { event.preventDefault(); void saveColumnName(column.id); }}>
                      <Input bind:value={columnNameDraft} aria-label="Column name" disabled={busy} />
                      <Button size="icon-xs" type="submit" aria-label="Save column name" disabled={busy}>✓</Button>
                      <Button size="icon-xs" variant="ghost" type="button" aria-label="Cancel column rename" onclick={cancelColumnRename} disabled={busy}>×</Button>
                    </form>
                  {:else}
                    <button class="column-title-button" type="button" onclick={() => beginColumnRename(column)}>
                      <h3>{column.name}</h3>
                      <Pencil size={13} aria-hidden="true" />
                    </button>
                  {/if}
                  <span class="column-count">{column.cards.length}</span>
                </header>

                <div class="column-cards">
                  {#each column.cards as card (card.id)}
                    <article class="board-card">
                      <p>{card.title}</p>
                      {#if card.meeting_id}
                        <a href={`/meeting/${card.meeting_id}`} class="card-source">{card.meeting_title ?? "Meeting note"}</a>
                      {/if}
                      <label class="card-move">
                        <span>Move to</span>
                        <select
                          aria-label={`Move ${card.title}`}
                          value={String(column.id)}
                          disabled={busy}
                          onchange={(event) => {
                            const select = event.currentTarget as HTMLSelectElement;
                            void moveCard(card.id, Number(select.value), select, column.id);
                          }}
                        >
                          {#each activeBoard.columns as destination (destination.id)}
                            <option value={destination.id}>{destination.name}</option>
                          {/each}
                        </select>
                      </label>
                    </article>
                  {:else}
                    <div class="column-empty">No action items here yet.</div>
                  {/each}
                </div>
              </article>
            {/each}
          </div>
        </section>
      {:else}
        <div class="empty-state" role="status">Create a board to start tracking action items.</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .boards-page {
    padding-bottom: 72px;
  }

  .boards-header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 24px;
    margin-bottom: 24px;
  }

  .eyebrow {
    display: block;
    margin-bottom: 8px;
    color: var(--text-muted);
    font: 500 10px/1.2 var(--font-mono);
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .boards-header h1 {
    margin: 0;
  }

  .boards-copy {
    margin: 8px 0 0;
    color: var(--text-muted);
    font-size: 14px;
  }

  .new-board-form,
  .new-column-form,
  .rename-board-form,
  .rename-column-form {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .new-board-form {
    width: min(360px, 100%);
  }

  .new-board-form :global(input),
  .new-column-form :global(input) {
    min-width: 0;
  }

  .board-welcome,
  .board-error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 14px;
    margin-bottom: 18px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-input);
    background: var(--surface-soft);
    font-size: 13px;
  }

  .board-welcome {
    color: var(--ink-strong);
  }

  .board-welcome span {
    color: var(--text-muted);
  }

  .board-error {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .boards-layout {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    gap: 22px;
    min-width: 0;
  }

  .board-list {
    align-self: start;
    padding: 14px 10px;
    background: var(--surface-soft);
    border: 1px solid var(--hairline-soft);
    border-radius: var(--radius-card);
  }

  .board-list-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px 10px;
    color: var(--text-muted);
    font: 500 10px/1.2 var(--font-mono);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .mono {
    font-family: var(--font-mono);
  }

  .board-list-items {
    display: grid;
    gap: 3px;
  }

  .board-list-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 9px 8px;
    border: 0;
    border-radius: var(--radius-input);
    background: transparent;
    color: var(--ink);
    cursor: pointer;
    font: inherit;
    text-align: left;
  }

  .board-list-item:hover {
    background: var(--surface-elev);
  }

  .board-list-item.active {
    background: var(--brand-soft);
    color: var(--brand-deep);
  }

  .board-list-item small {
    color: var(--text-muted);
    font: 10px/1.2 var(--font-mono);
  }

  .board-workspace {
    min-width: 0;
  }

  .board-workspace-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 14px;
  }

  .board-workspace-header h2 {
    margin: 0;
    color: var(--ink-strong);
    font: 700 28px/1.2 var(--font-display);
  }

  .board-workspace-header p {
    margin: 5px 0 0;
    color: var(--text-muted);
    font-size: 12px;
  }

  .board-title-button,
  .column-title-button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .board-title-button:hover,
  .column-title-button:hover {
    color: var(--text-muted);
  }

  .board-home-link,
  .card-source {
    color: var(--text-muted);
    font-size: 12px;
    text-decoration: none;
  }

  .board-home-link:hover,
  .card-source:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .column-toolbar {
    display: flex;
    justify-content: flex-end;
    margin-bottom: 12px;
  }

  .new-column-form {
    width: min(320px, 100%);
  }

  .kanban-grid {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    max-width: 100%;
    padding-bottom: 12px;
    overflow-x: auto;
  }

  .kanban-column {
    flex: 0 0 260px;
    min-height: 310px;
    padding: 12px;
    background: var(--surface-soft);
    border: 1px solid var(--hairline-soft);
    border-radius: var(--radius-card);
  }

  .column-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 32px;
    margin-bottom: 10px;
  }

  .column-title-button h3 {
    margin: 0;
    color: var(--ink-strong);
    font: 600 14px/1.3 var(--font-body);
  }

  .column-count {
    min-width: 22px;
    padding: 3px 6px;
    border-radius: var(--radius-pill);
    background: var(--surface);
    color: var(--text-muted);
    font: 10px/1.2 var(--font-mono);
    text-align: center;
  }

  .rename-column-form {
    min-width: 0;
    width: 100%;
  }

  .rename-column-form :global(input) {
    min-width: 0;
  }

  .column-cards {
    display: grid;
    gap: 8px;
  }

  .board-card {
    display: grid;
    gap: 9px;
    padding: 12px;
    background: var(--canvas);
    border: 1px solid var(--hairline-soft);
    border-radius: var(--radius-input);
    box-shadow: 0 2px 6px var(--shadow-ambient);
  }

  .board-card p {
    margin: 0;
    color: var(--ink);
    font-size: 13px;
    line-height: 1.45;
  }

  .card-source {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-move {
    display: grid;
    gap: 4px;
    color: var(--text-muted);
    font: 10px/1.2 var(--font-mono);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .card-move select {
    width: 100%;
    min-height: 28px;
    padding: 3px 7px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-input);
    background: var(--surface-soft);
    color: var(--ink);
    font: 12px/1.2 var(--font-body);
    text-transform: none;
  }

  .column-empty {
    padding: 20px 10px;
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }

  @media (max-width: 860px) {
    .boards-header {
      align-items: stretch;
      flex-direction: column;
    }

    .new-board-form {
      width: min(420px, 100%);
    }

    .boards-layout {
      grid-template-columns: 1fr;
    }

    .board-list-items {
      display: flex;
      gap: 4px;
      max-width: 100%;
      overflow-x: auto;
    }

    .board-list-item {
      flex: 0 0 auto;
      width: auto;
      min-width: 130px;
    }
  }

  @media (max-width: 560px) {
    .board-workspace-header {
      flex-direction: column;
    }

    .new-column-form {
      width: 100%;
    }
  }
</style>
