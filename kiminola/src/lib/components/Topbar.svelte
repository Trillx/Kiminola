<script lang="ts">
  import { themeState, toggleTheme } from "$lib/theme.svelte";
  import { Button } from "$lib/components/ui/button";
</script>

<div class="topbar">
  <button
    class="theme-toggle"
    onclick={toggleTheme}
    title={themeState.theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
    aria-label={themeState.theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
  >
    {themeState.theme === "dark" ? "☀️" : "🌙"}
  </button>
  <Button href="/record">+ New meeting</Button>
</div>
