<script lang="ts">
  import { page } from "$app/state";
  import { sidebarState, toggleSidebar } from "$lib/sidebar.svelte";
  import { spaces } from "$lib/mock";

  // Which spaces are expanded in the tree (all expanded by default).
  let collapsedSpaces = $state<Record<string, boolean>>({});

  function toggleSpace(id: string) {
    collapsedSpaces[id] = !collapsedSpaces[id];
  }

  let pathname = $derived(page.url.pathname);
</script>

<aside class="sidebar">
  <button
    class="sidebar-collapse-btn"
    onclick={toggleSidebar}
    title={sidebarState.collapsed ? "Expand sidebar" : "Collapse sidebar"}
    aria-label={sidebarState.collapsed ? "Expand sidebar" : "Collapse sidebar"}
  >
    {sidebarState.collapsed ? "⇥" : "⇤"}
  </button>

  <div class="search-pill">
    <span>🔍 Search</span>
    <span>Ctrl+K</span>
  </div>

  <nav>
    <a class="nav-item" class:active={pathname === "/"} href="/">🏠 <span>Home</span></a>

    <div class="nav-section">Spaces</div>

    {#each spaces as space (space.id)}
      {@const collapsed = !!collapsedSpaces[space.id]}
      <button class="space-item" class:collapsed onclick={() => toggleSpace(space.id)}>
        <span class="space-arrow">▾</span>
        <span>{space.icon}</span>
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
    <div class="sidebar-icons">
      <a class="icon-btn" href="/record" title="New meeting">➕</a>
      <button class="icon-btn" title="Settings">⚙</button>
    </div>
    <div class="trial-card">
      <div class="trial-text">
        <strong>MIT License</strong>
        Open source
      </div>
      <button class="btn-tiny">Contribute</button>
    </div>
    <div class="account-row">
      <div class="avatar">K</div>
      <div class="name">Kiminola</div>
      <span>⋯</span>
    </div>
  </div>
</aside>
