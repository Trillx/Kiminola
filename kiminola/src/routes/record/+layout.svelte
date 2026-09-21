<script lang="ts">
  import { page } from "$app/state";

  let { children } = $props();
</script>

<!-- A committed recording URL starts a new episode, even for query-only
     navigation. The current page's guard must finish persistence before this
     key changes; rejected/cancelled navigation keeps the same editor alive. -->
{#key page.url.href}
  {@render children()}
{/key}
