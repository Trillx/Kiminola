<script lang="ts">
  import { tick } from "svelte";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import { addBoardCard, listBoards, type Board } from "$lib/tauri";
  import { Button } from "$lib/components/ui/button";
  import { Textarea } from "$lib/components/ui/textarea";
  import type { ActionItemEntry } from "$lib/action-items";

  type Step = "action" | "board" | "column";
  type Props = {
    meetingId: number;
    actionItems: ActionItemEntry[];
    enhancedMarkdown: string;
  };

  let { meetingId, actionItems, enhancedMarkdown }: Props = $props();
  let open = $state(false);
  let step = $state<Step>("action");
  let boards = $state<Board[]>([]);
  let selectedBoardId = $state<number | null>(null);
  let customAction = $state("");
  let selectedAction = $state("");
  let selectedActionIndex = $state<number | null>(null);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let triggerRef = $state<HTMLButtonElement | null>(null);
  let closeButtonRef = $state<HTMLButtonElement | null>(null);
  let announcement = $state("");

  let selectedBoard = $derived(boards.find((board) => board.id === selectedBoardId) ?? null);

  async function closeMenu(restoreFocus = true, force = false) {
    if (!force && (loading || saving)) return;
    open = false;
    step = "action";
    error = null;
    selectedBoardId = null;
    selectedAction = "";
    selectedActionIndex = null;
    customAction = "";
    if (restoreFocus) {
      await tick();
      triggerRef?.focus();
    }
  }

  async function openMenu() {
    if (loading || saving) return;
    open = true;
    step = "action";
    error = null;
    selectedBoardId = null;
    selectedAction = "";
    selectedActionIndex = null;
    customAction = "";
    await tick();
    closeButtonRef?.focus();
  }

  async function chooseAction(title: string, sourceActionIndex: number | null = null) {
    const trimmed = title.trim();
    if (!trimmed || loading) return;
    selectedAction = trimmed;
    selectedActionIndex = sourceActionIndex;
    loading = true;
    error = null;
    try {
      const snapshot = await listBoards();
      boards = snapshot.boards;
      if (boards.length === 0) {
        error = "No boards are available. Open Boards to create one.";
        return;
      }
      step = "board";
    } catch (cause) {
      error = String(cause);
    } finally {
      loading = false;
    }
  }

  function chooseBoard(board: Board) {
    selectedBoardId = board.id;
    error = null;
    step = "column";
  }

  async function addToColumn(columnId: number) {
    if (selectedBoardId === null || !selectedAction || saving) return;
    const boardName = selectedBoard?.name ?? "board";
    const columnName = selectedBoard?.columns.find((column) => column.id === columnId)?.name ?? "column";
    const actionTitle = selectedAction;
    const actionIndex = selectedActionIndex;
    const sourceMarkdown = actionIndex === null ? null : enhancedMarkdown;
    saving = true;
    error = null;
    try {
      await addBoardCard({
        boardId: selectedBoardId,
        columnId,
        title: actionTitle,
        meetingId,
        sourceActionIndex: actionIndex,
        sourceEnhancedMarkdown: sourceMarkdown,
      });
      announcement = `Added ${actionTitle} to ${boardName}, ${columnName}.`;
      saving = false;
      await closeMenu(true, true);
    } catch (cause) {
      error = String(cause);
    } finally {
      saving = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && open && !loading && !saving) void closeMenu();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="board-quick-add">
  <Button
    bind:ref={triggerRef}
    variant="outline"
    size="sm"
    aria-expanded={open}
    aria-haspopup="dialog"
    aria-label="Add action item to a board"
    disabled={loading || saving}
    onclick={open ? () => void closeMenu() : () => void openMenu()}
  >
    <ListTodo size={15} aria-hidden="true" />
    Add to board
  </Button>
  <span class="sr-only" role="status" aria-live="polite">{announcement}</span>

  {#if open}
    <div class="board-quick-add-panel" role="dialog" aria-label="Add action item to a board">
      <header class="quick-add-header">
        <div>
          <span class="quick-add-eyebrow">Boards</span>
          <h2>
            {step === "action" ? "Choose an action" : step === "board" ? "Choose a board" : "Choose a column"}
          </h2>
        </div>
        <button bind:this={closeButtonRef} class="quick-add-close" type="button" aria-label="Close board menu" disabled={loading || saving} onclick={() => void closeMenu()}>×</button>
      </header>


      {#if step === "action"}
        {#if actionItems.length > 0}
          <p class="quick-add-copy">Select an action item from these enhanced notes.</p>
          <div class="quick-add-options" aria-label="Enhanced note action items">
            {#each actionItems as item (item.sourceIndex)}
              <button class="quick-add-option" type="button" disabled={loading} onclick={() => void chooseAction(item.title, item.sourceIndex)}>
                {item.title}
              </button>
            {/each}
          </div>
        {:else}
          <p class="quick-add-copy">Add a task from this meeting to a board.</p>
        {/if}
        <label class="quick-add-field">
          <span>{actionItems.length > 0 ? "Or add another action" : "Action item"}</span>
          <Textarea bind:value={customAction} rows={3} placeholder="What needs to happen next?" />
        </label>
        <Button onclick={() => void chooseAction(customAction)} disabled={!customAction.trim() || loading}>
          {loading ? "Loading boards…" : "Continue"}
        </Button>
      {:else if step === "board"}
        <button class="quick-add-back" type="button" onclick={() => (step = "action")}>
          <ArrowLeft size={14} aria-hidden="true" /> Action item
        </button>
        <div class="quick-add-options" aria-label="Boards">
          {#each boards as board (board.id)}
            <button class="quick-add-option" type="button" onclick={() => chooseBoard(board)}>
              <span>{board.name}</span>
              <small>{board.columns.length} {board.columns.length === 1 ? "column" : "columns"}</small>
            </button>
          {/each}
        </div>
        <a class="quick-add-manage" href="/boards" onclick={() => void closeMenu(false)}>Manage boards</a>
      {:else if selectedBoard}
        <button class="quick-add-back" type="button" onclick={() => (step = "board")}>
          <ArrowLeft size={14} aria-hidden="true" /> {selectedBoard.name}
        </button>
        <p class="quick-add-copy">Where should this action land?</p>
        <div class="quick-add-options" aria-label="Board columns">
          {#each selectedBoard.columns as column (column.id)}
            <button
              class="quick-add-option"
              type="button"
              disabled={saving}
              onclick={() => void addToColumn(column.id)}
            >
              <span>{column.name}</span>
              <small>{column.cards.length} {column.cards.length === 1 ? "card" : "cards"}</small>
            </button>
          {/each}
        </div>
      {/if}

      {#if error}<p class="quick-add-error" role="alert">{error}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .board-quick-add {
    position: relative;
  }

  .board-quick-add-panel {
    position: absolute;
    z-index: 20;
    top: calc(100% + 8px);
    right: 0;
    width: min(360px, calc(100vw - 32px));
    display: grid;
    gap: 14px;
    padding: 16px;
    background: var(--canvas);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-card);
    box-shadow: 0 14px 34px var(--shadow-paper);
  }

  .quick-add-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .quick-add-eyebrow {
    display: block;
    margin-bottom: 4px;
    color: var(--text-muted);
    font: 500 10px/1.2 var(--font-mono);
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  h2 {
    margin: 0;
    color: var(--ink-strong);
    font-size: 18px;
  }

  .quick-add-close {
    border: 0;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 22px;
    line-height: 1;
  }

  .quick-add-close:hover {
    color: var(--ink);
  }

  .quick-add-copy {
    margin: 0;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.5;
  }

  .quick-add-field {
    display: grid;
    gap: 6px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .quick-add-options {
    display: grid;
    gap: 6px;
    max-height: 240px;
    overflow: auto;
  }

  .quick-add-option {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    padding: 10px 11px;
    border: 1px solid var(--hairline-soft);
    border-radius: var(--radius-input);
    background: var(--surface-soft);
    color: var(--ink);
    cursor: pointer;
    font: inherit;
    text-align: left;
  }

  .quick-add-option:hover:not(:disabled) {
    border-color: var(--brand);
    background: var(--surface);
  }

  .quick-add-option:disabled {
    cursor: wait;
    opacity: 0.6;
  }

  .quick-add-option small {
    flex-shrink: 0;
    color: var(--text-muted);
    font: 10px/1.4 var(--font-mono);
    text-transform: uppercase;
  }

  .quick-add-back,
  .quick-add-manage {
    width: fit-content;
    border: 0;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    font: 12px/1.4 var(--font-mono);
    text-decoration: none;
  }

  .quick-add-back {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0;
  }

  .quick-add-back:hover,
  .quick-add-manage:hover {
    color: var(--brand);
  }

  .quick-add-error {
    padding: 9px 10px;
    border-radius: var(--radius-input);
    font-size: 12px;
    line-height: 1.4;
  }


  .quick-add-error {
    margin: 0;
    background: var(--danger-soft);
    color: var(--danger);
  }
</style>
