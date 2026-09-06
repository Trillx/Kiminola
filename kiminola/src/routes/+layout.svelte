<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { beforeNavigate, goto } from "$app/navigation";
  import { themeState } from "$lib/theme.svelte";
  import { sidebarState } from "$lib/sidebar.svelte";
  import { stopSidebarMotion } from "$lib/sidebar-motion";
  import { isOnboardingComplete, onShortcutTriggered } from "$lib/tauri";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Topbar from "$lib/components/Topbar.svelte";
  import MeetingPresencePrompt from "$lib/components/MeetingPresencePrompt.svelte";
  import UpdateBanner from "$lib/components/UpdateBanner.svelte";
  import { libraryDestinationState, recordingHref } from "$lib/library-tree.svelte";
  import { startAutomaticUpdateCheck, updateState } from "$lib/update.svelte";
  import DatabaseGate from "$lib/components/DatabaseGate.svelte";

  let { children } = $props();
  let databaseReady = $state(false);
  let updateBusy = $derived(updateState.status === "preparing" || updateState.status === "installing");
  beforeNavigate(({ cancel }) => { if (updateBusy) cancel(); });
  import { setupCompactWindowSync } from "$lib/compact-window";
  let compactWindow = $state(false);
  let compactWindowResizing = $state(false);

  // Reflect theme + sidebar state onto the document so the CSS variables
  // (--sidebar-width drives all fixed-position math) stay in sync.
  $effect(() => {
    document.documentElement.setAttribute("data-theme", themeState.theme);
  });

  $effect(() => {
    document.documentElement.dataset.sidebarCollapsed = String(sidebarState.collapsed);
    document.documentElement.setAttribute("data-compact-window-resizing", compactWindowResizing ? "true" : "false");
  });

  // CSS owns compact layout. A resize also ends any in-flight button animation
  // so shell geometry follows the viewport without a trailing transition.
  onMount(() => {
    const media = window.matchMedia("(max-width: 760px)");
    window.addEventListener("resize", stopSidebarMotion);
    const stopCompactSync = setupCompactWindowSync({
      media,
      initialCompactWindow: compactWindow,
      requestFrame: (callback) => requestAnimationFrame(callback),
      cancelFrame: (handle) => cancelAnimationFrame(handle),
      onStateChange: ({ compactWindow: nextCompactWindow, compactWindowResizing: resizing }) => {
        compactWindow = nextCompactWindow;
        compactWindowResizing = resizing;
      },
    });
    return () => { stopCompactSync(); window.removeEventListener("resize", stopSidebarMotion); stopSidebarMotion(); };
  });

  // Onboarding gate: the library is inaccessible until onboarding completes.
  async function onDatabaseReady() {
    databaseReady = true;
    try {
      const complete = await isOnboardingComplete();
      if (!complete) {
        goto("/onboarding", { replaceState: true });
      }
    } catch (err) {
      console.error("[layout] onboarding check failed:", err);
    }
  }

  // The global shortcut opens the recording view. Once there, the page owns
  // the stop action so native finalization and durable meeting save stay one
  // retry-safe transaction.
  $effect(() => {
    let unlisten: (() => void) | undefined;
    onShortcutTriggered(() => {
      if (!databaseReady || updateBusy) return;
      if (page.url.pathname !== "/record") {
        goto(recordingHref(libraryDestinationState.last));
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  });
  let isOnboarding = $derived(page.url.pathname === "/onboarding");
  let isMeetingPromptOverlay = $derived(
    page.url.searchParams.get("window") === "meeting-prompt",
  );

  $effect(() => {
    if (databaseReady && !isOnboarding && !isMeetingPromptOverlay) startAutomaticUpdateCheck();
  });
</script>

<DatabaseGate onready={onDatabaseReady}>
<div inert={updateBusy}>
{#if isOnboarding}
  {@render children()}
{:else if isMeetingPromptOverlay}
  <MeetingPresencePrompt overlay />
{:else}
  <div class="app" class:sidebar-collapsed={sidebarState.collapsed}>
    <Sidebar />
    <main class="main">
      <Topbar />
      {@render children()}
      <MeetingPresencePrompt />
      <UpdateBanner />
    </main>
  </div>
{/if}
</div>
{#if updateBusy}
  <div class="update-shutdown" role="status" aria-live="polite">
    {updateState.status === "preparing" ? "Saving your changes before updating…" : "Installing the update. Kimi Nola will restart…"}
  </div>
{/if}
</DatabaseGate>

<style>
  .update-shutdown { position: fixed; inset: 0; z-index: 100; display: grid; place-items: center; padding: 32px; background: var(--paper); color: var(--ink); }
</style>
