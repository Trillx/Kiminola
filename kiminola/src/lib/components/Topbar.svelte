<script lang="ts">
  import { themeState, toggleTheme } from "$lib/theme.svelte";
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
  <a class="btn btn-primary" href="/record">+ New meeting</a>
</div>
