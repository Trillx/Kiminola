<script lang="ts">
  import { page } from "$app/state";
  import { sidebarState, toggleSidebar } from "$lib/sidebar.svelte";
  import { themeState } from "$lib/theme.svelte";
  import { listSpaces, createSpace, type Space } from "$lib/tauri";
  import SearchDialog from "$lib/components/SearchDialog.svelte";

  // Which spaces are expanded in the tree (all expanded by default).
  let collapsedSpaces = $state<Record<number, boolean>>({});
  let spacesList = $state<Space[]>([]);

  // Inline space creation
  let addingSpace = $state(false);
  let newSpaceName = $state("");
  let spaceInputRef = $state<HTMLInputElement | null>(null);
  let searchOpen = $state(false);

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

  function toggleSpace(id: number) {
    collapsedSpaces[id] = !collapsedSpaces[id];
  }

  async function confirmCreateSpace() {
    const name = newSpaceName.trim();
    if (!name) {
      addingSpace = false;
      return;
    }
    try {
      await createSpace(name);
      spacesList = await listSpaces();
      addingSpace = false;
      newSpaceName = "";
    } catch (err) {
      console.error("Failed to create space:", err);
    }
  }

  function cancelCreateSpace() {
    addingSpace = false;
    newSpaceName = "";
  }

  function onSpaceInputKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      confirmCreateSpace();
    } else if (event.key === "Escape") {
      cancelCreateSpace();
    }
  }

  let pathname = $derived(page.url.pathname);

  $effect(() => {
    if (addingSpace && spaceInputRef) {
      spaceInputRef.focus();
    }
  });

  // Reload the tree on every navigation — cheap query, and it catches a new
  // meeting being saved right before the app routes to its detail page.
  $effect(() => {
    pathname;
    listSpaces()
      .then((s) => (spacesList = s))
      .catch((err) => console.error("Failed to load spaces:", err));
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

    <div class="nav-section spaces-header">
      <span>Spaces</span>
      <button
        class="add-space-btn"
        onclick={() => (addingSpace = true)}
        title="Add space"
        aria-label="Add space"
      >
        +
      </button>
    </div>

    {#if addingSpace}
      <input
        class="space-input"
        type="text"
        placeholder="Space name"
        bind:value={newSpaceName}
        bind:this={spaceInputRef}
        onkeydown={onSpaceInputKeydown}
        onblur={confirmCreateSpace}
      />
    {/if}

    {#each spacesList as space (space.id)}
      {@const collapsed = !!collapsedSpaces[space.id]}
      <button class="space-item" class:collapsed onclick={() => toggleSpace(space.id)}>
        <span class="space-arrow">▾</span>
        <span class="space-name">{space.name}</span>
      </button>
      <div class="space-children" class:collapsed>
        {#each space.meetings as meeting (meeting.id)}
          <a
            class="nav-item space-child"
            class:active={pathname === `/meeting/${meeting.id}`}
            href="/meeting/{meeting.id}">📝 <span>{meeting.title}</span></a
          >
        {/each}
      </div>
    {/each}
  </nav>

  <div class="sidebar-bottom">
    <a class="nav-item" class:active={pathname === "/settings"} href="/settings"
      >⚙️ <span>Settings</span></a
    >
    <div class="account-row">
      <div class="avatar">K</div>
      <div class="name">
        Kimi Nola
        <span class="name-sub">Open source · MIT License</span>
      </div>
    </div>
  </div>
</aside>
