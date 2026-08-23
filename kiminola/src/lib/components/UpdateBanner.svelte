<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { Button } from "$lib/components/ui/button";
  import { compactReleaseNotes, isRecordingPath } from "$lib/update-policy";
  import { installUpdate, updateState } from "$lib/update.svelte";

  let dismissedVersion = $state<string | null>(null);
  let actionError = $state("");
  let isRecording = $derived(isRecordingPath(page.url.pathname));
  let visible = $derived(
    updateState.version !== null &&
      dismissedVersion !== updateState.version &&
      ["available", "downloading", "ready", "installing"].includes(updateState.status),
  );
  let notes = $derived(compactReleaseNotes(updateState.notes));

  async function install() {
    actionError = "";
    const installed = await installUpdate(() => !isRecording);
    if (!installed && !isRecording && updateState.status === "error") {
      actionError = updateState.error ?? "The update could not be installed.";
    }
  }

  function defer() {
    dismissedVersion = updateState.version;
  }

  function openSettings() {
    dismissedVersion = updateState.version;
    void goto("/settings?section=about");
  }
</script>

{#if visible}
  <aside class="update-banner" aria-live="polite">
    <div class="update-copy">
      <div class="update-kicker">Stable update</div>
      <strong>Kimi Nola {updateState.version} is available</strong>
      {#if updateState.status === "downloading"}
        <span>Downloading the signed update…</span>
      {:else if updateState.status === "installing"}
        <span>Installing now. Kimi Nola will restart when the update is complete.</span>
      {:else if isRecording}
        <span>Finish and save this meeting before installing the update.</span>
      {:else if updateState.status === "ready"}
        <span>The update is downloaded and ready to install.</span>
      {:else}
        <span>Review the release notes before you choose when to install it.</span>
      {/if}
      {#if notes}<small>{notes}</small>{/if}
      {#if updateState.status === "downloading"}
        <div class="update-progress" aria-label="Update download progress">
          <div class="update-progress-bar" style={`width: ${updateState.progress}%`}></div>
        </div>
      {/if}
      {#if actionError}<span class="update-error" role="alert">{actionError}</span>{/if}
    </div>
    <div class="update-actions">
      {#if updateState.status === "available" || updateState.status === "ready"}
        <Button onclick={install} disabled={isRecording}>
          {updateState.status === "ready" ? "Restart and update" : "Install update"}
        </Button>
      {:else if updateState.status === "installing"}
        <Button disabled>Installing…</Button>
      {/if}
      <Button variant="ghost" onclick={defer} disabled={updateState.status === "installing"}>
        Later
      </Button>
      <Button variant="ghost" size="sm" onclick={openSettings}>Details</Button>
    </div>
  </aside>
{/if}

<style>
  .update-banner {
    position: fixed;
    right: 24px;
    bottom: 24px;
    z-index: 20;
    display: flex;
    align-items: flex-start;
    gap: 18px;
    width: min(620px, calc(100vw - 48px));
    padding: 16px 18px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-card);
    background: var(--surface);
    color: var(--ink);
    box-shadow: 0 16px 44px var(--shadow-paper);
  }

  .update-copy {
    display: grid;
    min-width: 0;
    gap: 3px;
  }

  .update-kicker {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .update-copy strong { color: var(--ink-strong); }
  .update-copy span, .update-copy small { color: var(--text-muted); }
  .update-copy small { line-height: 1.4; }

  .update-actions {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 6px;
  }

  .update-progress {
    height: 5px;
    margin-top: 8px;
    overflow: hidden;
    border-radius: 999px;
    background: var(--surface-soft);
  }

  .update-progress-bar {
    height: 100%;
    background: var(--brand);
  }

  .update-error { color: var(--danger) !important; }

  @media (max-width: 760px) {
    .update-banner {
      right: 12px;
      bottom: 12px;
      width: calc(100vw - 24px);
      flex-direction: column;
      gap: 12px;
    }

    .update-actions { flex-wrap: wrap; }
  }
</style>
