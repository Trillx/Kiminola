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
  let creatingBoard = $state(false);
  let creatingColumn = $state(false);
  let newBoardTrigger = $state<HTMLElement>();
  let newColumnTrigger = $state<HTMLElement>();
  let editingBoard = $state(false);
  let boardNameDraft = $state("");
  let editingColumnId = $state<number | null>(null);
  let columnNameDraft = $state("");
  let draggingCardId = $state<number | null>(null);
  let draggingFromColumnId = $state<number | null>(null);
  let dropColumnId = $state<number | null>(null);

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
      creatingBoard = false;
      newBoardTrigger?.focus();
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
      creatingColumn = false;
      newColumnTrigger?.focus();
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

  async function moveCard(cardId: number, columnId: number, persistedColumnId: number, select?: HTMLSelectElement) {
    if (busy || columnId === persistedColumnId) return;
    busy = true;
    error = null;
    try {
      await moveBoardCard(cardId, columnId);
      await loadBoards();
    } catch (cause) {
      if (select) select.value = String(persistedColumnId);
      error = errorMessage(cause);
    } finally {
      busy = false;
    }
  }

  function clearCardDrag() {
    draggingCardId = null;
    draggingFromColumnId = null;
    dropColumnId = null;
  }

  function beginCardDrag(event: DragEvent, cardId: number, columnId: number) {
    if (busy || !event.dataTransfer) {
      event.preventDefault();
      return;
    }
    draggingCardId = cardId;
    draggingFromColumnId = columnId;
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", String(cardId));
  }

  function targetCardColumn(event: DragEvent, columnId: number) {
    if (draggingCardId === null || !event.dataTransfer) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    dropColumnId = columnId;
  }

  function leaveCardColumn(event: DragEvent, columnId: number) {
    const column = event.currentTarget as HTMLElement;
    const nextTarget = event.relatedTarget as Node | null;
    if (nextTarget && column.contains(nextTarget)) return;
    if (dropColumnId === columnId) dropColumnId = null;
  }

  function dropCard(event: DragEvent, columnId: number) {
    if (draggingCardId === null || draggingFromColumnId === null) return;
    event.preventDefault();
    const cardId = draggingCardId;
    const sourceColumnId = draggingFromColumnId;
    clearCardDrag();
    void moveCard(cardId, columnId, sourceColumnId);
  }
</script>

<svelte:head>
  <title>Boards — Kimi Nola</title>
</svelte:head>

<div class="main-content boards-page">
  <header class="boards-header">
    <div>
      <h1 class="display">Boards</h1>
    </div>
    <details bind:open={creatingBoard}>
      <summary bind:this={newBoardTrigger}><Plus size={15} aria-hidden="true" /> New board</summary>
    <form class="new-board-form" onsubmit={(event) => { event.preventDefault(); void createNewBoard(); }}>
      <Input bind:value={newBoardName} aria-label="New board name" placeholder="New board name" disabled={busy} />
      <Button type="submit" size="sm" disabled={!newBoardName.trim() || busy}>Create</Button>
      <Button type="button" size="sm" variant="ghost" disabled={busy} onclick={() => { creatingBoard = false; newBoardName = ""; newBoardTrigger?.focus(); }}>Cancel</Button>
    </form>
    </details>
  </header>

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
      <nav class="board-list" aria-label="Boards">
        <div class="board-list-items ui-scrollbar">
          {#each snapshot.boards as board (board.id)}
            <button
              type="button"
              class:active={board.id === activeBoardId}
              class="board-list-item"
              aria-current={board.id === activeBoardId ? "page" : undefined}
              disabled={busy}
              onclick={() => { activeBoardId = board.id; cancelBoardRename(); cancelColumnRename(); creatingColumn = false; newColumnName = ""; }}
            >
              <span>{board.name}</span>
              <small>{board.columns.reduce((total, column) => total + column.cards.length, 0)}</small>
            </button>
          {/each}
        </div>
      </nav>

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
            <details bind:open={creatingColumn}>
              <summary bind:this={newColumnTrigger}><Plus size={14} aria-hidden="true" /> Add column</summary>
            <form class="new-column-form" onsubmit={(event) => { event.preventDefault(); void addColumn(); }}>
              <Input bind:value={newColumnName} aria-label="New column name" placeholder="New column name" disabled={busy} />
              <Button type="submit" size="sm" disabled={!newColumnName.trim() || busy}>Add</Button>
              <Button type="button" size="sm" variant="ghost" disabled={busy} onclick={() => { creatingColumn = false; newColumnName = ""; newColumnTrigger?.focus(); }}>Cancel</Button>
            </form>
            </details>
          </header>

          {#if activeBoard.columns.every(column => column.cards.length === 0)}
            <p class="board-hint">Add action items from a meeting's enhanced notes to this board.</p>
          {/if}

          <div class="kanban-grid ui-scrollbar" aria-label="Kanban columns">
            {#each activeBoard.columns as column (column.id)}
              <article
                class="kanban-column"
                class:drop-target={dropColumnId === column.id}
                aria-label={column.name}
                ondragover={(event) => targetCardColumn(event, column.id)}
                ondragleave={(event) => leaveCardColumn(event, column.id)}
                ondrop={(event) => dropCard(event, column.id)}
              >
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

                <div class="column-cards ui-scrollbar">
                  {#each column.cards as card (card.id)}
                    <article
                      class="board-card"
                      class:dragging={draggingCardId === card.id}
                      draggable={!busy}
                      ondragstart={(event) => beginCardDrag(event, card.id, column.id)}
                      ondragend={clearCardDrag}
                    >
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
                            void moveCard(card.id, Number(select.value), column.id, select);
                          }}
                        >
                          {#each activeBoard.columns as destination (destination.id)}
                            <option value={destination.id}>{destination.name}</option>
                          {/each}
                        </select>
                      </label>
                    </article>
                  {:else}
                    <div class="column-empty">No items</div>
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
  :global(.main):has(> .boards-page) {
    height: 100vh;
    min-height: 0;
    overflow: hidden;
  }

  .boards-page {
    max-width: none;
    min-width: 0;
    min-height: 0;
    padding: 24px 28px 28px;
    overflow: hidden;
  }

  .boards-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
  }

  .boards-header h1 {
    margin: 0;
    font-size: 28px;
  }

  .board-hint {
    margin: 0 0 16px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .new-board-form,
  .new-column-form,
  .rename-board-form,
  .rename-column-form {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  details { min-width: 0; }
  summary {
    display: flex;
    align-items: center;
    gap: 6px;
    width: fit-content;
    margin-left: auto;
    padding: 7px 10px;
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    list-style: none;
  }
  summary::-webkit-details-marker { display: none; }
  summary:hover { background: var(--surface-soft); color: var(--ink); }
  summary:focus-visible, .board-list-item:focus-visible, .board-title-button:focus-visible, .column-title-button:focus-visible { outline: 2px solid var(--ink); outline-offset: 3px; }
  .new-board-form, .new-column-form { width: min(400px, 100%); margin-top: 8px; }

  .new-board-form :global(input),
  .new-column-form :global(input) {
    min-width: 0;
  }

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


  .board-error {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .boards-layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
    flex: 1;
    gap: 20px;
    min-width: 0;
    min-height: 0;
  }

  .board-list {
    min-width: 0;
    border-bottom: 1px solid var(--hairline);
  }

  .board-list-items {
    display: flex;
    gap: 6px;
    overflow-x: auto;
    padding-bottom: 8px;
  }

  .board-list-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex: 0 0 auto;
    max-width: 260px;
    padding: 8px 12px;
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
  .board-list-item > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .board-workspace {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
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
    font: 400 24px/1.2 var(--font-display);
    overflow-wrap: anywhere;
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

  .card-source {
    color: var(--text-muted);
    font-size: 12px;
    text-decoration: none;
  }

  .card-source:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .kanban-grid {
    display: flex;
    align-items: stretch;
    flex: 1;
    gap: 12px;
    max-width: 100%;
    min-height: 0;
    padding-bottom: 12px;
    overflow-x: auto;
  }

  .kanban-column {
    display: flex;
    flex-direction: column;
    flex: 1 0 220px;
    min-width: 0;
    min-height: 0;
    padding: 12px;
    border: 1px solid transparent;
    background: var(--surface-soft);
    border-radius: 8px;
    overflow: hidden;
    transition: border-color 120ms ease, background 120ms ease;
  }

  .kanban-column.drop-target {
    border-color: var(--hairline-strong);
    background: var(--surface-elev);
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
    overflow-wrap: anywhere;
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
    align-content: start;
    flex: 1;
    gap: 8px;
    min-height: 0;
    padding-right: 3px;
    overflow-y: auto;
  }

  .board-card {
    display: grid;
    gap: 9px;
    padding: 12px;
    background: var(--canvas);
    border: 1px solid var(--hairline-soft);
    border-radius: var(--radius-input);
    box-shadow: 0 2px 6px var(--shadow-ambient);
    cursor: grab;
  }

  .board-card:active {
    cursor: grabbing;
  }

  .board-card.dragging {
    opacity: 0.56;
  }

  .board-card p {
    margin: 0;
    color: var(--ink);
    font-size: 13px;
    line-height: 1.45;
    overflow-wrap: anywhere;
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

  @media (max-width: 560px) {
    .boards-page { padding: 20px 16px; }
    .boards-header { flex-wrap: wrap; }
    details[open] { width: 100%; }
    .board-workspace-header {
      flex-wrap: wrap;
    }

    .new-column-form {
      width: 100%;
    }
  }
</style>
