<script lang="ts">
  import { onMount } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { ContextMenu } from "bits-ui";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Move from "@lucide/svelte/icons/move";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import type { LibraryLocation, LibraryNode } from "$lib/tauri";
  import { canDropNode, nodeKey, nodeRef } from "$lib/library-tree";
  import LibraryTreeNode from "./LibraryTreeNode.svelte";

  type Props = {
    node: LibraryNode;
    depth: number;
    collapsed: Record<string, boolean>;
    pathname: string;
    tree: LibraryNode[];
    draggingNode: LibraryLocation | null;
    dropTarget: LibraryLocation | null;
    onToggle: (location: LibraryLocation) => void;
    onNewMeeting: (location: LibraryLocation) => void;
    onNewSpace: (parentSpaceId: number) => void;
    onRename: (node: LibraryNode) => void;
    onMove: (node: LibraryNode) => void;
    onOpenMeeting: (meetingId: number) => void;
    onDragStart: (location: LibraryLocation) => void;
    onDragOver: (location: LibraryLocation | null) => void;
    onDrop: (location: LibraryLocation) => void;
    onDragEnd: () => void;
  };

  let {
    node,
    depth,
    collapsed,
    pathname,
    tree,
    draggingNode,
    dropTarget,
    onToggle,
    onNewMeeting,
    onNewSpace,
    onRename,
    onMove,
    onOpenMeeting,
    onDragStart,
    onDragOver,
    onDrop,
    onDragEnd,
  }: Props = $props();

  let location = $derived(nodeRef(node));
  let key = $derived(nodeKey(location));
  let isCollapsed = $derived(!!collapsed[key]);
  let hasChildren = $derived(node.children.length > 0);
  let label = $derived(node.kind === "space" ? node.name : node.title);
  let reducedMotion = $state(false);
  onMount(() => {
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => { reducedMotion = preference.matches; };
    update();
    preference.addEventListener("change", update);
    return () => preference.removeEventListener("change", update);
  });
  let isDropTarget = $derived(
    !!draggingNode && dropTarget?.kind === location.kind && dropTarget.id === location.id,
  );
  let isValidDropTarget = $derived(
    !!draggingNode && canDropNode(draggingNode, location, tree),
  );

  function handleDragStart(event: DragEvent) {
    if (!event.dataTransfer) return;
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("application/x-kiminola-library-node", JSON.stringify(location));
    onDragStart(location);
  }

  function handleDragOver(event: DragEvent) {
    if (!draggingNode || !isValidDropTarget) return;
    const dataTransfer = event.dataTransfer;
    if (!dataTransfer) return;
    event.preventDefault();
    dataTransfer.dropEffect = "move";
    onDragOver(location);
  }

  function handleDrop(event: DragEvent) {
    if (!draggingNode || !isValidDropTarget) return;
    event.preventDefault();
    onDrop(location);
  }

  function handleTriggerKeydown(event: KeyboardEvent) {
    // Buttons own Enter/Space. Never handle a descendant's key a second time.
    if (!(event.target instanceof Element) || event.target.closest(".library-node-actions")) return;
    if (event.key === "ArrowRight" && (hasChildren || node.kind === "space")) {
      event.preventDefault();
      event.stopPropagation();
      if (isCollapsed) onToggle(location);
      else document.getElementById(`library-children-${node.kind}-${node.id}`)
        ?.querySelector<HTMLElement>(".library-node-main")?.focus();
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      event.stopPropagation();
      if ((hasChildren || node.kind === "space") && !isCollapsed) onToggle(location);
      else event.target.closest(".library-node")
        ?.parentElement?.closest(".library-node")?.querySelector<HTMLElement>(".library-node-main")?.focus();
    }
  }
</script>

<div class="library-node" class:nested={depth > 0} role="listitem" style={`--tree-depth: ${depth}`}>
<ContextMenu.Root>
  <ContextMenu.Trigger
    class="library-node-trigger"
    role="group"
    tabindex={-1}
    aria-label={node.kind === "space" ? `Space: ${node.name}` : `Meeting: ${node.title}`}
    onkeydown={handleTriggerKeydown}
  >
    <div
      class="library-node-row"
      class:active={node.kind === "meeting" && pathname === `/meeting/${node.id}`}
      class:drop-target={isDropTarget}
      class:drop-valid={!!draggingNode && isValidDropTarget}
      class:drop-invalid={!!draggingNode && !isValidDropTarget}
      role="presentation"
      draggable="true"
      ondragstart={handleDragStart}
      ondragover={handleDragOver}
      ondrop={handleDrop}
      ondragend={onDragEnd}
    >
      {#if node.kind === "space"}
        <button
          class="library-node-main"
          type="button"
          aria-expanded={!isCollapsed}
          aria-controls={`library-children-${node.kind}-${node.id}`}
          title={label}
          onclick={() => onToggle(location)}
        >
          <span class="library-disclosure" class:expanded={!isCollapsed}>
            <ChevronRight size={13} strokeWidth={1.8} />
          </span>
          <span class="library-node-icon space-icon">
            {#if isCollapsed}<Folder size={15} strokeWidth={1.7} />{:else}<FolderOpen size={15} strokeWidth={1.7} />{/if}
          </span>
          <span class="library-node-label">{node.name}</span>
        </button>
      {:else}
        {#if hasChildren}
          <button class="library-disclosure meeting-disclosure" class:expanded={!isCollapsed}
            type="button" aria-label={`${isCollapsed ? "Expand" : "Collapse"} ${label}`}
            aria-expanded={!isCollapsed} aria-controls={`library-children-${node.kind}-${node.id}`}
            onclick={() => onToggle(location)}>
            <ChevronRight size={13} strokeWidth={1.8} />
          </button>
        {:else}
          <span class="library-disclosure" aria-hidden="true"></span>
        {/if}
        <a class="library-node-main meeting-main" href={`/meeting/${node.id}`} title={label}
          aria-current={pathname === `/meeting/${node.id}` ? "page" : undefined}>
          <span class="library-node-icon"><FileText size={15} strokeWidth={1.7} /></span>
          <span class="library-node-label">{node.title}</span>
        </a>
      {/if}

      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <button
              {...props}
              class="library-node-actions"
              type="button"
              aria-label={`Actions for ${node.kind === "space" ? node.name : node.title}`}
            >
              <EllipsisVertical size={14} strokeWidth={1.8} />
            </button>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="library-menu">
          {#if node.kind === "space"}
            <DropdownMenu.Item onclick={() => onNewMeeting(location)}>
              <Plus size={14} /> New meeting here
            </DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => onNewSpace(node.id)}>
              <FolderPlus size={14} /> New sub-space
            </DropdownMenu.Item>
            <DropdownMenu.Separator />
            <DropdownMenu.Item onclick={() => onRename(node)}>
              <Pencil size={14} /> Rename
            </DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => onMove(node)}>
              <Move size={14} /> Move to…
            </DropdownMenu.Item>
          {:else}
            <DropdownMenu.Item onclick={() => onOpenMeeting(node.id)}>
              <FileText size={14} /> Open
            </DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => onNewMeeting(location)}>
              <Plus size={14} /> New child meeting
            </DropdownMenu.Item>
            <DropdownMenu.Separator />
            <DropdownMenu.Item onclick={() => onRename(node)}>
              <Pencil size={14} /> Rename
            </DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => onMove(node)}>
              <Move size={14} /> Move to…
            </DropdownMenu.Item>
          {/if}
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </div>

  </ContextMenu.Trigger>

  <div id={`library-children-${node.kind}-${node.id}`}>
    {#if !isCollapsed && (hasChildren || node.kind === "space")}
      <div class="library-node-children" role="list" aria-label={`Contents of ${label}`}
        inert={isCollapsed} aria-hidden={isCollapsed}
        transition:slide={{ duration: reducedMotion ? 0 : 180, easing: cubicOut }}>
        {#each node.children as child (nodeKey(nodeRef(child)))}
          <LibraryTreeNode
            node={child}
            depth={depth + 1}
            {collapsed}
            {pathname}
            {tree}
            {draggingNode}
            {dropTarget}
            {onToggle}
            {onNewMeeting}
            {onNewSpace}
            {onRename}
            {onMove}
            {onOpenMeeting}
            {onDragStart}
            {onDragOver}
            {onDrop}
            {onDragEnd}
          />
        {/each}
        {#if !hasChildren}
          <div class="library-node-empty" role="listitem">Empty Space</div>
        {/if}
      </div>
    {/if}
  </div>

  <ContextMenu.Portal>
    <ContextMenu.Content class="library-context-menu">
      {#if node.kind === "space"}
        <ContextMenu.Item onSelect={() => onNewMeeting(location)}>
          <Plus size={14} /> New meeting here
        </ContextMenu.Item>
        <ContextMenu.Item onSelect={() => onNewSpace(node.id)}>
          <FolderPlus size={14} /> New sub-space
        </ContextMenu.Item>
        <ContextMenu.Separator />
        <ContextMenu.Item onSelect={() => onRename(node)}>
          <Pencil size={14} /> Rename
        </ContextMenu.Item>
        <ContextMenu.Item onSelect={() => onMove(node)}>
          <Move size={14} /> Move to…
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item onSelect={() => onOpenMeeting(node.id)}>
          <FileText size={14} /> Open
        </ContextMenu.Item>
        <ContextMenu.Item onSelect={() => onNewMeeting(location)}>
          <Plus size={14} /> New child meeting
        </ContextMenu.Item>
        <ContextMenu.Separator />
        <ContextMenu.Item onSelect={() => onRename(node)}>
          <Pencil size={14} /> Rename
        </ContextMenu.Item>
        <ContextMenu.Item onSelect={() => onMove(node)}>
          <Move size={14} /> Move to…
        </ContextMenu.Item>
      {/if}
    </ContextMenu.Content>
  </ContextMenu.Portal>
</ContextMenu.Root>
</div>

<style>
  .library-node {
    position: relative;
    min-width: 0;
  }

  .library-node.nested::before,
  .library-node.nested:not(:last-child)::after {
    content: "";
    position: absolute;
    left: calc(16px + (var(--tree-depth) - 1) * 18px);
    pointer-events: none;
    border-left: 1px solid var(--hairline);
  }

  .library-node.nested::before {
    top: 0;
    width: 6px;
    height: 17px;
    border-bottom: 1px solid var(--hairline);
    border-bottom-left-radius: 4px;
  }

  .library-node.nested:not(:last-child)::after {
    top: 0;
    bottom: 0;
  }

  :global(.library-node-trigger) {
    display: block;
    border-radius: 8px;
    outline: none;
  }

  .library-node-main:focus-visible,
  .meeting-disclosure:focus-visible,
  .library-node-actions:focus-visible {
    outline: 2px solid var(--text-muted);
    outline-offset: -2px;
  }

  .library-node-row {
    display: flex;
    align-items: center;
    min-width: 0;
    min-height: 34px;
    padding: 2px 2px 2px calc(4px + var(--tree-depth) * 18px);
    border-radius: 8px;
    transition: background 150ms ease, box-shadow 150ms ease, opacity 150ms ease;
  }

  .library-node-row:hover,
  .library-node-row.active {
    background: var(--surface-elev);
  }

  .library-node-row.active {
    background: var(--brand-soft);
    color: var(--brand-deep);
  }

  .library-node-row.drop-target {
    background: var(--brand-soft);
    box-shadow: inset 0 0 0 1px var(--brand);
  }

  .library-node-row.drop-valid:not(.drop-target) {
    box-shadow: inset 0 0 0 1px var(--hairline-strong);
  }

  .library-node-row.drop-invalid {
    opacity: 0.72;
  }

  .library-node-main {
    display: flex;
    align-items: center;
    min-width: 0;
    flex: 1;
    gap: 6px;
    padding: 4px 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 13px;
    text-align: left;
    text-decoration: none;
    cursor: pointer;
  }

  .library-node-main:hover {
    color: var(--ink);
  }

  .library-disclosure {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 24px;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--text-muted);
  }

  .library-disclosure :global(svg) {
    transition: transform 180ms cubic-bezier(0.215, 0.61, 0.355, 1);
  }

  .library-disclosure.expanded :global(svg) {
    transform: rotate(90deg);
  }

  .meeting-disclosure { cursor: pointer; }
  .meeting-disclosure:hover { background: var(--surface-elev); }
  .meeting-main { margin-left: 6px; }

  .library-node-empty {
    padding: 5px 6px 7px calc(34px + (var(--tree-depth) + 1) * 18px);
    color: var(--text-muted);
    font-size: 11px;
  }

  .library-node-icon {
    display: inline-flex;
    flex: 0 0 15px;
    color: var(--text-muted);
  }

  .library-node-icon.space-icon {
    color: var(--text-muted);
  }

  .library-node-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .library-node-actions {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 24px;
    width: 24px;
    height: 24px;
    margin-right: 2px;
    padding: 0;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity 150ms ease, background 150ms ease, color 150ms ease;
  }

  .library-node-row:hover .library-node-actions,
  .library-node-row:focus-within .library-node-actions,
  .library-node-actions[data-state="open"],
  .library-node-actions:focus-visible {
    opacity: 1;
  }

  .library-node-actions:hover {
    background: var(--surface);
    color: var(--ink);
  }

  .library-node-children {
    display: block;
  }

  @media (prefers-reduced-motion: reduce) {
    .library-node-row,
    .library-node-actions,
    .library-disclosure :global(svg) { transition: none; }
  }

  :global(.library-menu),
  :global(.library-context-menu) {
    z-index: 200;
    min-width: 190px;
    padding: 4px;
    border: 1px solid var(--hairline);
    border-radius: 9px;
    background: var(--surface);
    box-shadow: 0 10px 28px var(--shadow-ambient);
    color: var(--ink);
  }

  :global(.library-menu [data-highlighted]),
  :global(.library-context-menu [data-highlighted]) {
    background: var(--surface-elev);
  }

  :global(.library-menu [role="menuitem"]),
  :global(.library-context-menu [role="menuitem"]) {
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

  :global(.library-menu [role="group"]),
  :global(.library-context-menu [role="group"]) {
    height: 1px;
    margin: 4px 0;
    background: var(--hairline-soft);
  }
</style>
