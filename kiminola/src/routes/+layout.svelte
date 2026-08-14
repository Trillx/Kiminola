<script lang="ts">
  import "../app.css";
  import { themeState } from "$lib/theme.svelte";
  import { sidebarState } from "$lib/sidebar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Topbar from "$lib/components/Topbar.svelte";

  let { children } = $props();

  // Reflect theme + sidebar state onto the document so the CSS variables
  // (--sidebar-width drives all fixed-position math) stay in sync.
  $effect(() => {
    document.documentElement.setAttribute("data-theme", themeState.theme);
  });

  $effect(() => {
    document.documentElement.style.setProperty(
      "--sidebar-width",
      sidebarState.collapsed ? "0px" : "240px",
    );
  });
</script>

<div class="app" class:sidebar-collapsed={sidebarState.collapsed}>
  <Sidebar />
  <main class="main">
    <Topbar />
    {@render children()}
  </main>
</div>
