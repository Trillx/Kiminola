<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { Button } from "$lib/components/ui/button";
  import {
    dismissMeetingPrompt,
    getMeetingPresenceState,
    jotNotesFromMeetingPrompt,
    onMeetingPresenceAction,
    onMeetingPresencePrompt,
    onMeetingPresenceState,
    startRecordingFromMeetingPrompt,
    type MeetingPresenceState,
    type MeetingPrompt,
  } from "$lib/tauri";

  let prompt = $state<MeetingPrompt | null>(null);
  let busy = $state(false);
  let error = $state("");

  onMount(() => {
    let unlistenPrompt: (() => void) | undefined;
    let unlistenState: (() => void) | undefined;
    let unlistenAction: (() => void) | undefined;

    onMeetingPresencePrompt((next) => {
      prompt = next;
      error = "";
    }).then((fn) => (unlistenPrompt = fn));

    onMeetingPresenceState((state: MeetingPresenceState) => {
      prompt = state.prompt;
    }).then((fn) => (unlistenState = fn));

    onMeetingPresenceAction(async (action) => {
      prompt = null;
      if (action.action === "notes" && action.draft_id !== undefined) {
        await goto(`/note/${action.draft_id}`);
      } else if (action.action === "start") {
        await goto("/record");
      }
    }).then((fn) => (unlistenAction = fn));

    getMeetingPresenceState()
      .then((state) => (prompt = state.prompt))
      .catch((err) => console.error("Failed to load meeting prompt state:", err));

    return () => {
      unlistenPrompt?.();
      unlistenState?.();
      unlistenAction?.();
    };
  });

  async function resolve(action: "notes" | "start" | "dismiss") {
    if (!prompt || busy) return;
    const promptId = prompt.id;
    busy = true;
    error = "";
    try {
      if (action === "notes") {
        const draftId = await jotNotesFromMeetingPrompt(promptId);
        prompt = null;
        await goto(`/note/${draftId}`);
      } else if (action === "start") {
        await startRecordingFromMeetingPrompt(promptId);
        prompt = null;
        await goto("/record");
      } else {
        await dismissMeetingPrompt(promptId);
        prompt = null;
      }
    } catch (err) {
      error = "That prompt is no longer active.";
      const state = await getMeetingPresenceState().catch(() => null);
      prompt = state?.prompt ?? null;
      console.error("Meeting prompt action failed:", err);
    } finally {
      busy = false;
    }
  }
</script>

{#if prompt}
  <aside class="meeting-prompt" aria-live="assertive" aria-label="Meeting prompt">
    <div class="prompt-kicker">{prompt.app_label}</div>
    <div class="prompt-title">{prompt.message}</div>
    <div class="prompt-copy">{prompt.not_recording_message}</div>
    <div class="prompt-actions">
      <Button onclick={() => resolve("notes")} disabled={busy}>Jot notes</Button>
      <Button variant="secondary" onclick={() => resolve("start")} disabled={busy}>Start recording</Button>
      <Button variant="ghost" onclick={() => resolve("dismiss")} disabled={busy}>Not now</Button>
    </div>
    {#if error}<div class="prompt-error">{error}</div>{/if}
  </aside>
{/if}

<style>
  .meeting-prompt {
    position: fixed;
    z-index: 60;
    right: 24px;
    bottom: 24px;
    width: min(360px, calc(100vw - 32px));
    padding: 18px;
    border: 1px solid var(--hairline);
    border-radius: 16px;
    background: var(--surface);
    box-shadow: 0 16px 40px var(--shadow-ambient);
  }

  .prompt-kicker {
    margin-bottom: 7px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .prompt-title {
    color: var(--ink-strong);
    font-size: 16px;
    font-weight: 600;
    line-height: 1.35;
  }

  .prompt-copy {
    margin-top: 6px;
    color: var(--text-muted);
    font-size: 13px;
  }

  .prompt-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 16px;
  }

  .prompt-error {
    margin-top: 10px;
    color: var(--destructive);
    font-size: 12px;
  }
</style>
