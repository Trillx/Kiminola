<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { themeState } from "$lib/theme.svelte";
  import { sidebarState } from "$lib/sidebar.svelte";
  import { isOnboardingComplete, onShortcutTriggered, stopRecording } from "$lib/tauri";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Topbar from "$lib/components/Topbar.svelte";
  import MeetingPresencePrompt from "$lib/components/MeetingPresencePrompt.svelte";

  let { children } = $props();
  let compactWindow = $state(false);

  // Reflect theme + sidebar state onto the document so the CSS variables
  // (--sidebar-width drives all fixed-position math) stay in sync.
  $effect(() => {
    document.documentElement.setAttribute("data-theme", themeState.theme);
  });

  $effect(() => {
    const useCompactShell = compactWindow || sidebarState.collapsed;
    document.documentElement.style.setProperty(
      "--sidebar-width",
      useCompactShell ? "0px" : "240px",
    );
    document.documentElement.dataset.compactWindow = compactWindow ? "true" : "false";
  });

  // A companion layout can make the main window much narrower than the
  // library's normal width. Collapse the navigation chrome at that width
  // without changing the user's saved sidebar preference.
  onMount(() => {
    const media = window.matchMedia("(max-width: 760px)");
    const syncCompactWindow = () => {
      compactWindow = media.matches;
    };
    syncCompactWindow();
    media.addEventListener("change", syncCompactWindow);
    return () => media.removeEventListener("change", syncCompactWindow);
  });

  // Onboarding gate: the library is inaccessible until onboarding completes.
  onMount(async () => {
    try {
      const complete = await isOnboardingComplete();
      if (!complete) {
        goto("/onboarding", { replaceState: true });
      }
    } catch (err) {
      console.error("[layout] onboarding check failed:", err);
    }
  });

  // Global start/stop shortcut: if we're already recording, stop and go home;
  // otherwise open the recording view so it can start.
  $effect(() => {
    let unlisten: (() => void) | undefined;
    onShortcutTriggered(async () => {
      if (page.url.pathname === "/record") {
        await stopRecording();
        goto("/");
      } else {
        goto("/record");
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
</script>

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
    </main>
  </div>
{/if}
