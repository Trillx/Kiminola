<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { sidebarState, toggleSidebar } from "$lib/sidebar.svelte";
  import { themeState } from "$lib/theme.svelte";
  import {
    createSpace,
    listLibraryTree,
    moveLibraryNode,
    renameMeeting,
    renameSpace,
    type LibraryLocation,
    type LibraryNode,
  } from "$lib/tauri";
  import {
    recordingHref,
    rememberMeetingLocation,
  } from "$lib/library-tree.svelte";
  import { destinationKey, moveOptions, nodeKey, nodeRef } from "$lib/library-tree";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Dialog from "$lib/components/ui/dialog";
  import { ContextMenu } from "bits-ui";
  import Plus from "@lucide/svelte/icons/plus";
  import SearchDialog from "$lib/components/SearchDialog.svelte";
  import LibraryTreeNode from "$lib/components/LibraryTreeNode.svelte";

  let collapsedNodes = $state<Record<string, boolean>>({});
  const collapsedStorageKey = "kiminola-library-collapsed";
  let storageReady = $state(false);
  let loadRequest = 0;
  let revealedPath: string | null = null;

  onMount(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(collapsedStorageKey) ?? "{}");
      if (saved && typeof saved === "object" && !Array.isArray(saved)) {
        collapsedNodes = Object.fromEntries(Object.entries(saved).filter(
          ([key, value]) => /^(space|meeting):[1-9]\d*$/.test(key) && value === true,
        )) as Record<string, boolean>;
      }
    } catch { /* Storage is optional; the tree must still work. */ }
    storageReady = true;
  });

  $effect(() => {
    if (!storageReady) return;
    try {
      localStorage.setItem(collapsedStorageKey, JSON.stringify(Object.fromEntries(
        Object.entries(collapsedNodes).filter(([, value]) => value),
      )));
    } catch { /* An unavailable store must not block navigation. */ }
  });
  let libraryTree = $state<LibraryNode[]>([]);
  let treeLoadError = $state<string | null>(null);
  let searchOpen = $state(false);

  let addingSpace = $state(false);
  let addingSpaceParentId = $state<number | null>(null);
  let newSpaceName = $state("");
  let spaceInputRef = $state<HTMLInputElement | null>(null);

  let draggingNode = $state<LibraryLocation | null>(null);
  let dropTarget = $state<LibraryLocation | null>(null);

  let moveDialogOpen = $state(false);
  let movingNode = $state<LibraryNode | null>(null);
  let renameDialogOpen = $state(false);
  let renamingNode = $state<LibraryNode | null>(null);
  let renameValue = $state("");
  let actionBusy = $state(false);
  let actionError = $state<string | null>(null);

  let pathname = $derived(page.url.pathname);
  let moveDestinationOptions = $derived(
    moveOptions(libraryTree, movingNode ? nodeRef(movingNode) : null),
  );

  $effect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchOpen = true;
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  });

  function errorMessage(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === "string") return error;
    return "The database request failed.";
  }

  async function loadTree() {
    const request = ++loadRequest;
    const route = pathname;
    try {
      const tree = await listLibraryTree();
      if (request !== loadRequest) return;
      libraryTree = tree;
      treeLoadError = null;
      if (route !== revealedPath) {
        const match = /^\/meeting\/(\d+)$/.exec(route);
        if (match) {
          const path = pathTo({ kind: "meeting", id: Number(match[1]) });
          for (const ancestor of path.slice(0, -1)) collapsedNodes[nodeKey(ancestor)] = false;
        }
        revealedPath = route;
      }
    } catch (err) {
      if (request !== loadRequest) return;
      treeLoadError = errorMessage(err);
      console.error("Failed to load library tree:", err);
    }
  }

  function pathTo(location: LibraryLocation, nodes = libraryTree): LibraryLocation[] {
    for (const node of nodes) {
      const ref = nodeRef(node);
      if (nodeKey(ref) === nodeKey(location)) return [ref];
      const children = pathTo(location, node.children);
      if (children.length) return [ref, ...children];
    }
    return [];
  }

  function toggleNode(location: LibraryLocation) {
    const key = nodeKey(location);
    collapsedNodes[key] = !collapsedNodes[key];
  }

  function expandNode(location: LibraryLocation) {
    for (const ancestor of pathTo(location)) collapsedNodes[nodeKey(ancestor)] = false;
  }

  function beginCreateSpace(parentSpaceId: number | null = null) {
    actionError = null;
    addingSpaceParentId = parentSpaceId;
    newSpaceName = "";
    addingSpace = true;
  }

  async function confirmCreateSpace() {
    const name = newSpaceName.trim();
    if (!name) {
      cancelCreateSpace();
      return;
    }
    actionBusy = true;
    actionError = null;
    try {
      const parentSpaceId = addingSpaceParentId;
      await createSpace(name, parentSpaceId);
      if (parentSpaceId !== null) expandNode({ kind: "space", id: parentSpaceId });
      cancelCreateSpace();
      await loadTree();
    } catch (err) {
      actionError = errorMessage(err);
      console.error("Failed to create Space:", err);
    } finally {
      actionBusy = false;
    }
  }

  function cancelCreateSpace() {
    addingSpace = false;
    addingSpaceParentId = null;
    newSpaceName = "";
  }

  function onSpaceInputKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      void confirmCreateSpace();
    } else if (event.key === "Escape") {
      cancelCreateSpace();
    }
  }

  function startMeeting(location: LibraryLocation) {
    rememberMeetingLocation(location);
    void goto(recordingHref(location));
  }

  function openMeeting(meetingId: number) {
    void goto(`/meeting/${meetingId}`);
  }

  function openRename(node: LibraryNode) {
    renamingNode = node;
    renameValue = node.kind === "space" ? node.name : node.title;
    actionError = null;
    renameDialogOpen = true;
  }

  async function saveRename() {
    if (!renamingNode) return;
    const value = renameValue.trim();
    if (!value) {
      actionError = "Name cannot be empty.";
      return;
    }
    actionBusy = true;
    actionError = null;
    try {
      if (renamingNode.kind === "space") {
        await renameSpace(renamingNode.id, value);
      } else {
        await renameMeeting(renamingNode.id, value);
      }
      renameDialogOpen = false;
      renamingNode = null;
      await loadTree();
    } catch (err) {
      actionError = errorMessage(err);
      console.error("Failed to rename library item:", err);
    } finally {
      actionBusy = false;
    }
  }

  function openMove(node: LibraryNode) {
    movingNode = node;
    actionError = null;
    moveDialogOpen = true;
  }

  async function performMove(source: LibraryLocation, destination: LibraryLocation | null) {
    if (destination && source.kind === destination.kind && source.id === destination.id) return;
    actionBusy = true;
    actionError = null;
    try {
      await moveLibraryNode(source, destination);
      if (destination) expandNode(destination);
      moveDialogOpen = false;
      movingNode = null;
      await loadTree();
    } catch (err) {
      actionError = errorMessage(err);
      console.error("Failed to move library item:", err);
    } finally {
      actionBusy = false;
    }
  }

  async function chooseMoveDestination(destination: LibraryLocation | null) {
    if (!movingNode || actionBusy) return;
    await performMove(nodeRef(movingNode), destination);
  }

  function beginDrag(location: LibraryLocation) {
    draggingNode = location;
    dropTarget = null;
  }

  function dragOver(location: LibraryLocation | null) {
    dropTarget = location;
  }

  function endDrag() {
    draggingNode = null;
    dropTarget = null;
  }

  async function dropOn(destination: LibraryLocation) {
    const source = draggingNode;
    if (!source || actionBusy) return;
    endDrag();
    await performMove(source, destination);
  }

  $effect(() => {
    if (addingSpace && spaceInputRef) spaceInputRef.focus();
  });

  // Reload the tree on navigation and when the app becomes visible. The query
  // is small, and this catches a meeting saved immediately before navigation.
  $effect(() => {
    pathname;
    void loadTree();
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") void loadTree();
    };
    window.addEventListener("focus", refreshWhenVisible);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.removeEventListener("focus", refreshWhenVisible);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  });
</script>

<aside class="sidebar">
  <button
    class="sidebar-collapse-btn"
    onclick={toggleSidebar}
    title={sidebarState.collapsed ? "Expand sidebar" : "Collapse sidebar"}
    aria-label={sidebarState.collapsed ? "Expand sidebar" : "Collapse sidebar"}
  >
    {sidebarState.collapsed ? "›" : "‹"}
  </button>

  <a class="wordmark" href="/" aria-label="Kimi Nola — home">
    <img
      src={themeState.theme === "dark"
        ? "/brand/kimi-nola-logo-primary-dark.svg"
        : "/brand/kimi-nola-logo-primary-light.svg"}
      alt="Kimi Nola"
    />
  </a>

  <button class="search-pill" onclick={() => (searchOpen = true)} aria-label="Search meetings">
    <span>🔍 Search</span>
    <span>Ctrl+K</span>
  </button>

  <SearchDialog bind:open={searchOpen} />

  <nav>
    <a class="nav-item" class:active={pathname === "/"} href="/">🏠 <span>Home</span></a>

    <ContextMenu.Root>
      <ContextMenu.Trigger class="spaces-header-trigger" role="group" tabindex={0}>
        <div class="nav-section spaces-header">
          <span>Spaces</span>
          <button
            class="add-space-btn"
            onclick={() => beginCreateSpace()}
            title="New Space"
            aria-label="New Space"
          >
            +
          </button>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content class="spaces-context-menu">
          <ContextMenu.Item onSelect={() => beginCreateSpace()}>
            <Plus size={14} /> New Space
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>

    {#if addingSpace}
      <input
        class="space-input"
        type="text"
        placeholder={addingSpaceParentId === null ? "New Space" : "New sub-space"}
        bind:value={newSpaceName}
        bind:this={spaceInputRef}
        onkeydown={onSpaceInputKeydown}
        disabled={actionBusy}
      />
    {/if}

    {#if treeLoadError}
      <div class="empty-state" role="status" style="margin: 8px 12px; padding: 12px; font-size: 12px;">
        Library could not be loaded.
        <button class="btn btn-ghost btn-sm" style="margin-top: 8px;" onclick={() => void loadTree()}>Retry</button>
      </div>
    {:else if libraryTree.length === 0}
      <div class="library-empty">
        <span>No Spaces yet.</span>
        <button class="btn btn-ghost btn-sm" onclick={() => beginCreateSpace()}>New Space</button>
      </div>
    {:else}
      <div class="library-tree" role="list" aria-label="Spaces and meetings">
      {#each libraryTree as node (nodeKey(nodeRef(node)))}
        <LibraryTreeNode
          {node}
          depth={0}
          collapsed={collapsedNodes}
          {pathname}
          tree={libraryTree}
          {draggingNode}
          {dropTarget}
          onToggle={toggleNode}
          onNewMeeting={startMeeting}
          onNewSpace={(parentId) => beginCreateSpace(parentId)}
          onRename={openRename}
          onMove={openMove}
          onOpenMeeting={openMeeting}
          onDragStart={beginDrag}
          onDragOver={dragOver}
          onDrop={dropOn}
          onDragEnd={endDrag}
        />
      {/each}
      </div>
    {/if}

    {#if actionError && !moveDialogOpen && !renameDialogOpen}
      <div class="library-action-error" role="alert">{actionError}</div>
    {/if}
  </nav>

  <div class="sidebar-bottom">
    <a class="nav-item" class:active={pathname === "/settings"} href="/settings">⚙️ <span>Settings</span></a>
    <div class="account-row">
      <div class="avatar">K</div>
      <div class="name">
        Kimi Nola
        <span class="name-sub">Open source · MIT License</span>
      </div>
    </div>
  </div>
</aside>

<Dialog.Root bind:open={moveDialogOpen}>
  <Dialog.Content class="library-dialog">
    <Dialog.Header>
      <Dialog.Title>Move {movingNode?.kind === "space" ? "Space" : "meeting"}</Dialog.Title>
      <Dialog.Description>Choose a destination. Moving keeps the item’s children with it.</Dialog.Description>
    </Dialog.Header>

    <div class="move-options" aria-label="Move destinations">
      {#if moveDestinationOptions.length === 0}
        <div class="move-empty">No valid destinations are available.</div>
      {:else}
        {#each moveDestinationOptions as option (destinationKey(option.location))}
          <button
            class="move-option"
            class:disabled={option.disabled}
            style={`padding-left: ${12 + option.depth * 16}px`}
            disabled={option.disabled || actionBusy}
            onclick={() => void chooseMoveDestination(option.location)}
          >
            <span>{option.label}</span>
            {#if option.disabled}<span class="move-option-note">Not valid</span>{/if}
          </button>
        {/each}
      {/if}
    </div>

    {#if actionError}
      <div class="library-dialog-error" role="alert">{actionError}</div>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={renameDialogOpen}>
  <Dialog.Content class="library-dialog">
    <Dialog.Header>
      <Dialog.Title>Rename {renamingNode?.kind === "space" ? "Space" : "meeting"}</Dialog.Title>
      <Dialog.Description>Names are shown throughout the library and exports.</Dialog.Description>
    </Dialog.Header>
    <Input bind:value={renameValue} aria-label="New name" onkeydown={(event) => event.key === "Enter" && void saveRename()} />
    {#if actionError}
      <div class="library-dialog-error" role="alert">{actionError}</div>
    {/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (renameDialogOpen = false)} disabled={actionBusy}>Cancel</Button>
      <Button onclick={() => void saveRename()} disabled={actionBusy}>Save</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<style>
  .library-empty {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 10px;
    color: var(--text-muted);
    font-size: 12px;
  }

  :global(.spaces-header-trigger) {
    display: block;
    border-radius: 8px;
    outline: none;
  }

  :global(.spaces-header-trigger:focus-visible) {
    box-shadow: 0 0 0 2px var(--brand);
  }

  :global(.spaces-context-menu) {
    z-index: 200;
    min-width: 150px;
    padding: 4px;
    border: 1px solid var(--hairline);
    border-radius: 9px;
    background: var(--surface);
    box-shadow: 0 10px 28px var(--shadow-ambient);
    color: var(--ink);
  }

  :global(.spaces-context-menu [role="menuitem"]) {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    padding: 5px 8px;
    border-radius: 6px;
    color: var(--ink);
    font-size: 12px;
    cursor: pointer;
  }

  :global(.spaces-context-menu [data-highlighted]) {
    background: var(--surface-elev);
  }

  .library-action-error,
  .library-dialog-error {
    padding: 7px 10px;
    border-radius: 7px;
    background: color-mix(in srgb, var(--destructive) 10%, transparent);
    color: var(--destructive);
    font-size: 12px;
  }

  :global(.library-dialog) {
    max-width: 440px;
  }

  .move-options {
    display: flex;
    flex-direction: column;
    max-height: 340px;
    gap: 2px;
    overflow-y: auto;
    padding: 4px;
    border: 1px solid var(--hairline-soft);
    border-radius: 9px;
  }

  .move-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 32px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }

  .move-option:hover:not(:disabled) {
    background: var(--surface-elev);
  }

  .move-option.disabled,
  .move-option:disabled {
    color: var(--text-muted);
    cursor: not-allowed;
    opacity: 0.62;
  }

  .move-option-note {
    padding-right: 8px;
    font-size: 11px;
  }

  .move-empty {
    padding: 18px 12px;
    color: var(--text-muted);
    font-size: 13px;
    text-align: center;
  }
</style>
