<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import LiveTranscript from "$lib/components/LiveTranscript.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Textarea } from "$lib/components/ui/textarea";
  import * as Dialog from "$lib/components/ui/dialog";
  import {
    startRecording,
    stopRecording,
    pauseRecording,
    resumeRecording,
    saveMeeting,
    getNoteDraft,
    onTranscriptEvent,
    type TranscriptEvent,
    type TranscriptLine,
  } from "$lib/tauri";

  // Title is fixed when the recording starts and saved with the meeting.
  const startedAt = new Date();
  const title = `Meeting · ${startedAt.toLocaleDateString("en-US", { month: "short", day: "numeric" })}, ${startedAt.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`;

  let notepad = $state("");
  let elapsed = $state(0);
  let lines = $state<TranscriptLine[]>([]);
  let partialIndex = $state(-1);
  let transcriptOpen = $state(false);
  let paused = $state(false);
  let stopping = $state(false);
  let noteDraftId = $state<number | null>(null);

  let timerText = $derived(
    `${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, "0")}`,
  );
  let statusLabel = $derived(paused ? "Paused" : "Recording");

  onMount(() => {
    const timer = setInterval(() => {
      if (!paused) elapsed += 1;
    }, 1000);
    let unlisten: (() => void) | undefined;

    const draftParam = page.url.searchParams.get("draft");
    const requestedDraftId = draftParam ? Number(draftParam) : NaN;

    const startPromise = onTranscriptEvent((event: TranscriptEvent) => {
      if (partialIndex >= 0 && partialIndex === lines.length - 1) {
        // Update the existing partial line.
        lines[partialIndex] = {
          channel: event.channel,
          text: event.text,
        };
      } else {
        // Append a new line.
        lines = [...lines, { channel: event.channel, text: event.text }];
        partialIndex = lines.length - 1;
      }

      if (!event.is_partial) {
        partialIndex = -1;
      }
      })
      .then((fn) => {
        unlisten = fn;
        if (!Number.isFinite(requestedDraftId)) {
          return startRecording();
        }
        return getNoteDraft(requestedDraftId)
          .then((draft) => {
            noteDraftId = draft.id;
            notepad = draft.raw_markdown;
          })
          .then(() => startRecording());
      })
      .catch((err) => {
        console.error("Failed to start recording:", err);
      });

    return () => {
      clearInterval(timer);
      // Wait for the listener + start sequence to finish before stopping,
      // so we don't try to stop a session that hasn't started yet.
      startPromise.finally(() => {
        unlisten?.();
        stopRecording().catch(() => {
          // Best-effort cleanup; the backend may already have stopped.
        });
      });
    };
  });

  async function pause() {
    await pauseRecording();
    paused = true;
  }

  async function resume() {
    await resumeRecording();
    paused = false;
  }

  async function finishAndNavigate(mode: "generate" | "enhance") {
    stopping = true;
    await stopRecording();
    const id = await saveMeeting({
      title,
      durationSeconds: elapsed,
      notepad,
      segments: lines.map((l) => ({ ...l })),
      noteDraftId,
    });
    goto(`/meeting/${id}?mode=${mode}`);
  }

  async function stopMeeting() {
    stopping = true;
    await stopRecording();
    const id = await saveMeeting({
      title,
      durationSeconds: elapsed,
      notepad,
      segments: lines.map((l) => ({ ...l })),
      noteDraftId,
    });
    goto(`/meeting/${id}`);
  }

  async function cancel() {
    stopping = true;
    await stopRecording();
    goto("/");
  }

  function onDialogOpenChange(open: boolean) {
    // Closing the pause dialog resumes recording.
    if (!open && paused) {
      resume();
    }
  }
</script>

<svelte:head>
  <title>Recording — Kimi Nola</title>
</svelte:head>

<div class="main-content recording-content">
  <div class="recording-shell">
    <div class="recording-top">
      <div>
        <div class="recording-title">{title}</div>
        <div class="recording-meta">{statusLabel} · {timerText}</div>
      </div>
      <div class="recording-badge" class:paused>{statusLabel}</div>
    </div>

    <div class="waveform" aria-hidden="true">
      <span></span><span></span><span></span><span></span><span></span><span></span><span
      ></span><span></span><span></span><span></span><span></span><span></span>
    </div>

    <div class="notepad-hero">
      <div class="notepad-header">
        <span class="notepad-label">My notes</span>
        <span style="font-size:12px;color:var(--soft);">Sketch while you listen</span>
      </div>
      <Textarea
        bind:value={notepad}
        placeholder="Jot rough thoughts, action items, or quotes here…"
        class="notepad-textarea"
      />
    </div>

    <div class="recording-actions">
      <Button variant="outline" onclick={cancel} disabled={stopping}>Cancel</Button>
      <Button variant="secondary" onclick={pause} disabled={paused || stopping}>Pause</Button>
      <Button variant="destructive" onclick={stopMeeting} disabled={stopping}>
        Stop meeting
      </Button>
    </div>
  </div>
</div>

<Dialog.Root bind:open={paused} onOpenChange={onDialogOpenChange}>
  <Dialog.Content showCloseButton={false} class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Meeting paused</Dialog.Title>
    </Dialog.Header>
    <div class="pause-timer">{timerText}</div>
    <div class="pause-actions">
      <Button class="w-full" onclick={resume} disabled={stopping}>Resume recording</Button>
      <Button class="w-full" variant="secondary" onclick={() => finishAndNavigate('generate')} disabled={stopping}>
        Generate meeting notes
      </Button>
      <Button class="w-full" variant="secondary" onclick={() => finishAndNavigate('enhance')} disabled={stopping}>
        Enhance meeting notes
      </Button>
    </div>
    <Button variant="ghost" class="pause-stop-link w-full" onclick={stopMeeting} disabled={stopping}>
      Stop meeting without generating notes
    </Button>
  </Dialog.Content>
</Dialog.Root>

<LiveTranscript {lines} {partialIndex} bind:open={transcriptOpen} />

<style>
  :global(.notepad-textarea) {
    flex: 1;
    min-height: 0;
    resize: none;
    border: none;
    background: transparent;
    padding: 0;
    font: inherit;
    font-size: 16px;
    line-height: 1.65;
    color: var(--ink);
  }
  :global(.notepad-textarea:focus) {
    box-shadow: none;
    border: none;
    outline: none;
  }
  :global(.notepad-textarea::placeholder) {
    color: var(--soft);
  }

  .pause-timer {
    text-align: center;
    font-size: 14px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  .pause-actions {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }

  :global(.pause-stop-link) {
    color: var(--soft);
    font-size: 13px;
    padding: 4px;
  }
  :global(.pause-stop-link:hover) {
    color: var(--ink);
  }
</style>
