<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import LiveTranscript from "$lib/components/LiveTranscript.svelte";
  import {
    applyTranscriptEvent,
    finalizedTranscript,
    offsetTranscriptEvent,
    recoverableTranscript,
  } from "$lib/transcript-state";
  import { createDraftAutosave, type DraftAutosave } from "$lib/draft-autosave";
  import {
    canStopRecording,
    canRetryFinish,
    recordingPhaseLabel,
    shouldAdvanceElapsed,
    shouldDiscardAutoDraft,
    type RecordingPhase,
  } from "$lib/recording-ui-state";
  import { Button } from "$lib/components/ui/button";
  import { Textarea } from "$lib/components/ui/textarea";
  import * as Dialog from "$lib/components/ui/dialog";
  import {
    startRecording,
    stopRecording,
    pauseRecording,
    resumeRecording,
    saveMeeting,
    createNoteDraft,
    getNoteDraft,
    updateNoteDraftRecovery,
    deleteNoteDraft,
    openMicrophonePrivacySettings,
    onTranscriptEvent,
    onRecordingQuitBlocked,
    type TranscriptEvent,
    type TranscriptLine,
  } from "$lib/tauri";

  // Title is fixed when the recording starts and saved with the meeting.
  const startedAt = new Date();
  const title = `Meeting · ${startedAt.toLocaleDateString("en-US", { month: "short", day: "numeric" })}, ${startedAt.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`;

  let notepad = $state("");
  let elapsed = $state(0);
  let lines = $state<TranscriptLine[]>([]);
  let transcriptOpen = $state(false);
  let phase = $state<RecordingPhase>("starting");
  let startError = $state<string | null>(null);
  let finishError = $state<string | null>(null);
  let nativeSessionActive = false;
  let requestedDraftId = NaN;
  let noteDraftId = $state<number | null>(null);
  let quitBlocked = $state(false);
  let recoveryDraftCreated = $state(false);
  let noteSaveStatus = $state("");
  let transcriptOffsetMs = 0;

  type FinishMode = "save" | "generate" | "enhance";
  let pendingFinishMode: FinishMode | null = null;

  interface RecoverySnapshot {
    rawMarkdown: string;
    durationSeconds: number;
    transcript: TranscriptLine[];
  }

  let noteAutosave: DraftAutosave<RecoverySnapshot> | undefined;

  let timerText = $derived(
    `${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, "0")}`,
  );
  let statusLabel = $derived(recordingPhaseLabel(phase));
  let stopping = $derived(phase === "stopping");

  function errorMessage(error: unknown, fallback: string): string {
    if (error instanceof Error) return error.message;
    if (typeof error === "string") return error;
    return fallback;
  }

  function recoverySnapshot(): RecoverySnapshot {
    return {
      rawMarkdown: notepad,
      durationSeconds: elapsed,
      transcript: recoverableTranscript(lines),
    };
  }

  function configureNoteAutosave(draftId: number) {
    noteAutosave?.cancel();
    noteAutosave = createDraftAutosave(
      async (snapshot) => {
        try {
          await updateNoteDraftRecovery(
            draftId,
            snapshot.rawMarkdown,
            snapshot.durationSeconds,
            snapshot.transcript,
          );
        } catch (error) {
          console.error("Failed to checkpoint recording notes:", error);
          throw error;
        }
      },
      (status) => {
        noteSaveStatus =
          status === "saving"
            ? "Saving for recovery..."
            : status === "saved"
              ? "Saved for recovery"
              : "Could not save recovery copy";
      },
    );
  }

  async function prepareNoteDraft(requestedDraftId: number) {
    if (Number.isFinite(requestedDraftId)) {
      const draft = await getNoteDraft(requestedDraftId);
      noteDraftId = draft.id;
      notepad = draft.raw_markdown;
      elapsed = Math.max(0, draft.recovery_duration_seconds);
      transcriptOffsetMs = elapsed * 1_000;
      lines = draft.recovery_transcript;
      configureNoteAutosave(draft.id);
      return;
    }

    try {
      const draftId = await createNoteDraft();
      noteDraftId = draftId;
      recoveryDraftCreated = true;
      configureNoteAutosave(draftId);
      noteSaveStatus = "Recovery copy ready";
    } catch (error) {
      console.error("Failed to create a recovery draft:", error);
      noteSaveStatus = "Recovery copy unavailable";
    }
  }

  function checkpointRecovery() {
    noteAutosave?.schedule(recoverySnapshot());
  }

  async function flushNoteCheckpoint() {
    const autosave = noteAutosave;
    if (!autosave) return;
    try {
      await autosave.flush(recoverySnapshot());
    } catch {
      // The normal meeting save below still receives the current notepad.
    }
  }

  function closeNoteAutosave() {
    noteAutosave?.cancel();
    noteAutosave = undefined;
  }

  async function prepareAndStart() {
    phase = "starting";
    startError = null;
    try {
      if (noteDraftId === null) await prepareNoteDraft(requestedDraftId);
      await startRecording();
      nativeSessionActive = true;
      phase = "recording";
    } catch (error) {
      nativeSessionActive = false;
      phase = "failed";
      startError = errorMessage(error, "Windows could not start the microphone.");
      console.error("Failed to start recording:", error);
    }
  }

  onMount(() => {
    const timer = setInterval(() => {
      if (shouldAdvanceElapsed(phase)) {
        elapsed += 1;
        if (elapsed % 5 === 0) checkpointRecovery();
      }
    }, 1000);
    let unlisten: (() => void)[] = [];

    const draftParam = page.url.searchParams.get("draft");
    requestedDraftId = draftParam ? Number(draftParam) : NaN;

    const startPromise = Promise.all([
      onTranscriptEvent((event: TranscriptEvent) => {
        lines = applyTranscriptEvent(lines, offsetTranscriptEvent(event, transcriptOffsetMs));
        checkpointRecovery();
      }),
      onRecordingQuitBlocked(() => {
        quitBlocked = true;
      }),
    ])
      .then(async (listeners) => {
        unlisten = listeners;
        await prepareAndStart();
      })
      .catch((err) => {
        phase = "failed";
        startError = errorMessage(err, "The recording page could not initialize.");
        console.error("Failed to initialize recording:", err);
      });

    return () => {
      clearInterval(timer);
      const autosave = noteAutosave;
      if (autosave) {
        void autosave.flush(recoverySnapshot()).catch(() => undefined);
        autosave.cancel();
        noteAutosave = undefined;
      }
      // Wait for the listener + start sequence to finish before stopping,
      // so we don't try to stop a session that hasn't started yet.
      startPromise.finally(() => {
        for (const listener of unlisten) listener();
        if (nativeSessionActive) {
          nativeSessionActive = false;
          stopRecording().catch(() => {
            // Best-effort cleanup; the backend may already have stopped.
          });
        }
      });
    };
  });

  async function pause() {
    await pauseRecording();
    phase = "paused";
  }

  async function resume() {
    await resumeRecording();
    phase = "recording";
  }

  async function finishMeeting(mode: FinishMode) {
    phase = "stopping";
    finishError = null;
    pendingFinishMode = mode;

    let meetingId: number;
    try {
      if (nativeSessionActive) {
        const finalEvents = await stopRecording();
        nativeSessionActive = false;
        for (const event of finalEvents) {
          lines = applyTranscriptEvent(lines, offsetTranscriptEvent(event, transcriptOffsetMs));
        }
      }
      await flushNoteCheckpoint();
      meetingId = await saveMeeting({
        title,
        durationSeconds: elapsed,
        notepad,
        segments: finalizedTranscript(lines),
        noteDraftId,
      });
    } catch (error) {
      phase = "finish_failed";
      finishError = errorMessage(error, "The meeting could not be finalized or saved.");
      console.error("Failed to finish meeting:", error);
      return;
    }

    closeNoteAutosave();
    pendingFinishMode = null;
    const suffix = mode === "save" ? "" : `?mode=${mode}`;
    await goto(`/meeting/${meetingId}${suffix}`);
  }

  function retryFinish() {
    if (pendingFinishMode && canRetryFinish(phase)) {
      void finishMeeting(pendingFinishMode);
    }
  }

  async function openRecoveryDraft() {
    await flushNoteCheckpoint();
    closeNoteAutosave();
    if (nativeSessionActive) {
      await stopRecording().catch(() => undefined);
      nativeSessionActive = false;
    }
    await goto(noteDraftId === null ? "/" : `/note/${noteDraftId}`);
  }

  async function cancel() {
    const discardAutoDraft = shouldDiscardAutoDraft(recoveryDraftCreated, nativeSessionActive);
    phase = "stopping";
    if (!discardAutoDraft) await flushNoteCheckpoint();
    closeNoteAutosave();
    if (nativeSessionActive) {
      await stopRecording();
      nativeSessionActive = false;
    }
    if (discardAutoDraft && noteDraftId !== null) {
      await deleteNoteDraft(noteDraftId).catch((error) => {
        console.error("Failed to remove cancelled recovery draft:", error);
      });
    }
    goto("/");
  }

  function onDialogOpenChange(open: boolean) {
    // Closing the pause dialog resumes recording.
    if (!open && phase === "paused") {
      void resume();
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
      <div
        class="recording-badge"
        class:paused={phase === "paused"}
        class:inactive={phase !== "recording"}
      >{statusLabel}</div>
    </div>

    <div class="waveform" class:inactive={phase !== "recording"} aria-hidden="true">
      <span></span><span></span><span></span><span></span><span></span><span></span><span
      ></span><span></span><span></span><span></span><span></span><span></span>
    </div>

    {#if phase === "failed"}
      <div class="recording-start-error" role="alert">
        <div>
          <strong>Recording couldn't start.</strong>
          <span>{startError}</span>
        </div>
        <div class="recording-start-error-actions">
          <Button size="sm" onclick={prepareAndStart}>Try again</Button>
          <Button
            size="sm"
            variant="outline"
            onclick={() => void openMicrophonePrivacySettings()}
          >Microphone settings</Button>
        </div>
      </div>
    {/if}

    {#if phase === "finish_failed"}
      <div class="recording-finish-error" role="alert">
        <div>
          <strong>The meeting isn't saved yet.</strong>
          <span>{finishError} Your recovery copy is still available.</span>
        </div>
        <div class="recording-start-error-actions">
          <Button size="sm" onclick={retryFinish}>Retry saving</Button>
          <Button size="sm" variant="outline" onclick={openRecoveryDraft}>
            Open recovery copy
          </Button>
        </div>
      </div>
    {/if}

    <div class="notepad-hero">
      <div class="notepad-header">
        <span class="notepad-label">My notes</span>
        <span class="note-save-status" aria-live="polite">
          {noteSaveStatus || "Sketch while you listen"}
        </span>
      </div>
      <Textarea
        bind:value={notepad}
        oninput={checkpointRecovery}
        placeholder="Jot rough thoughts, action items, or quotes here…"
        class="notepad-textarea"
      />
    </div>

    <div class="recording-actions">
      <Button variant="outline" onclick={cancel} disabled={phase === "starting" || stopping}>
        {phase === "failed"
          ? "Back to meetings"
          : phase === "finish_failed"
            ? "Keep recovery copy"
            : "Cancel"}
      </Button>
      <Button variant="secondary" onclick={pause} disabled={phase !== "recording"}>Pause</Button>
      <Button
        variant="destructive"
        onclick={() => void finishMeeting("save")}
        disabled={!canStopRecording(phase) || stopping}
      >
        Stop meeting
      </Button>
    </div>

    {#if quitBlocked}
      <div class="recording-quit-warning" role="alert">
        <div>
          <strong>Finish this meeting before quitting.</strong>
          <span>Stop the meeting to save it, or cancel to discard it intentionally.</span>
        </div>
        <Button variant="ghost" size="sm" onclick={() => (quitBlocked = false)}>Dismiss</Button>
      </div>
    {/if}
  </div>
</div>

<Dialog.Root open={phase === "paused"} onOpenChange={onDialogOpenChange}>
  <Dialog.Content showCloseButton={false} class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Meeting paused</Dialog.Title>
    </Dialog.Header>
    <div class="pause-timer">{timerText}</div>
    <div class="pause-actions">
      <Button class="w-full" onclick={resume} disabled={stopping}>Resume recording</Button>
      <Button class="w-full" variant="secondary" onclick={() => void finishMeeting('generate')} disabled={stopping}>
        Generate meeting notes
      </Button>
      <Button class="w-full" variant="secondary" onclick={() => void finishMeeting('enhance')} disabled={stopping}>
        Enhance meeting notes
      </Button>
    </div>
    <Button variant="ghost" class="pause-stop-link w-full" onclick={() => void finishMeeting('save')} disabled={stopping}>
      Stop meeting without generating notes
    </Button>
  </Dialog.Content>
</Dialog.Root>

<LiveTranscript {lines} bind:open={transcriptOpen} />

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

  .note-save-status {
    font-size: 12px;
    color: var(--soft);
  }

  :global(.recording-badge.inactive) {
    background: var(--surface);
    color: var(--text-muted);
  }

  :global(.recording-badge.inactive::before) {
    background: var(--soft);
    animation: none;
  }

  :global(.waveform.inactive span) {
    animation-play-state: paused;
    opacity: 0.35;
    transform: scaleY(0.15);
  }

  .recording-start-error,
  .recording-finish-error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
    padding: 14px 16px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--ink);
    font-size: 13px;
  }

  .recording-start-error > div:first-child,
  .recording-finish-error > div:first-child {
    display: grid;
    gap: 3px;
  }

  .recording-start-error span,
  .recording-finish-error span {
    color: var(--text-muted);
  }

  .recording-start-error-actions {
    display: flex;
    flex-shrink: 0;
    gap: 8px;
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

  .recording-quit-warning {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-top: 14px;
    padding: 12px 14px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--ink);
    font-size: 13px;
  }

  .recording-quit-warning div {
    display: grid;
    gap: 2px;
  }

  .recording-quit-warning span {
    color: var(--text-muted);
  }

  @media (max-width: 680px) {
    .recording-start-error,
    .recording-finish-error {
      align-items: stretch;
      flex-direction: column;
    }

    .recording-start-error-actions {
      flex-wrap: wrap;
    }
  }
</style>
