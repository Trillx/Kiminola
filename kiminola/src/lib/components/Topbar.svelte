<script lang="ts">
  import { themeState, toggleTheme } from "$lib/theme.svelte";
  import { Button } from "$lib/components/ui/button";
  import { libraryDestinationState, recordingHref } from "$lib/library-tree.svelte";
  import Moon from "@lucide/svelte/icons/moon";
  import Plus from "@lucide/svelte/icons/plus";
  import Sun from "@lucide/svelte/icons/sun";
</script>

<div class="topbar">
  <button
    class="theme-toggle"
    onclick={toggleTheme}
    title={themeState.theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
    aria-label={themeState.theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
  >
    {#if themeState.theme === "dark"}<Sun size={16} aria-hidden="true" />{:else}<Moon size={16} aria-hidden="true" />{/if}
  </button>
  <Button href={recordingHref(libraryDestinationState.last)}><Plus size={16} aria-hidden="true" /> New meeting</Button>
</div>
