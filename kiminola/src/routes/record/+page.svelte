<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { beforeNavigate, goto } from "$app/navigation";
  import LiveTranscript from "$lib/components/LiveTranscript.svelte";
  import { capturePhaseLabel } from "$lib/capture-session";
  import { captureSession as session } from "$lib/capture-session.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Textarea } from "$lib/components/ui/textarea";
  import * as Dialog from "$lib/components/ui/dialog";
  import { openMicrophonePrivacySettings } from "$lib/tauri";

  // The Capture session owns the recording lifecycle. This view renders it and
  // asks where to go when the user tries to leave mid-session.
  const capture = session.state;

  let transcriptOpen = $state(false);
  let navigationDialogOpen = $state(false);
  let pendingNavigationTarget: string | null = null;

  let phase = $derived(capture.phase);
  let timerText = $derived(
    `${Math.floor(capture.elapsedSeconds / 60)}:${String(capture.elapsedSeconds % 60).padStart(2, "0")}`,
  );
  let statusLabel = $derived(capturePhaseLabel(phase));
  let stopping = $derived(phase === "stopping");
  let audioPressureSource = $derived(
    !capture.audioPressure
      ? "audio"
      : capture.audioPressure.mic_dropped_samples > 0 &&
          capture.audioPressure.loopback_dropped_samples > 0
        ? "microphone and meeting audio"
        : capture.audioPressure.mic_dropped_samples > 0
          ? "microphone audio"
          : "meeting audio",
  );

  onMount(() => {
    void session.open(page.url.searchParams);
    return () => session.release();
  });

  beforeNavigate(({ cancel, to }) => {
    if (!to || to.url.href === page.url.href) return;
    // A teardown (or a leave queued behind startup) navigates itself once its
    // writes are done; nothing may bypass a recovery write still pending.
    if (session.blocksNavigation) {
      cancel();
      return;
    }
    if (!session.guardsNavigation) return;
    cancel();
    pendingNavigationTarget = `${to.url.pathname}${to.url.search}${to.url.hash}`;
    navigationDialogOpen = true;
  });

  // Startup failure or finishing ends the need for a leave decision.
  $effect(() => {
    if (!session.guardsNavigation) {
      navigationDialogOpen = false;
      pendingNavigationTarget = null;
    }
  });

  function continueRecordingHere() {
    navigationDialogOpen = false;
    pendingNavigationTarget = null;
  }

  function finishBeforeLeaving() {
    navigationDialogOpen = false;
    pendingNavigationTarget = null;
    session.stop();
  }

  function keepRecoveryAndLeave() {
    const destination = pendingNavigationTarget ?? "/";
    navigationDialogOpen = false;
    pendingNavigationTarget = null;
    void session.leave(destination);
  }

  function onNavigationDialogOpenChange(open: boolean) {
    navigationDialogOpen = open;
    if (!open) pendingNavigationTarget = null;
  }

  function onDialogOpenChange(open: boolean) {
    // Closing the pause dialog resumes recording.
    if (!open && session.canResume) {
      void session.resume();
    }
  }
</script>

<svelte:head>
  <title>Recording — OatWave</title>
</svelte:head>

<div class="main-content recording-content">
  <div class="recording-shell">
    <div class="recording-top">
      <div>
        <div class="recording-title">{capture.title}</div>
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
          <span>{capture.startError}</span>
        </div>
        <div class="recording-start-error-actions">
          <Button size="sm" onclick={() => void session.retryStart()}>Try again</Button>
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
          <span>{capture.finishError} Your latest work is still here. Retry before leaving.</span>
        </div>
        <div class="recording-start-error-actions">
          <Button size="sm" onclick={() => void session.retryFinish()}>Retry saving</Button>
          {#if capture.failedLeave !== null}
            <Button size="sm" variant="outline" onclick={() => void session.finish("save")}>
              Save as meeting
            </Button>
            <Button size="sm" variant="outline" onclick={() => void session.discard()}>
              Discard unsaved changes and leave
            </Button>
          {:else}
            <Button size="sm" variant="outline" onclick={() => void session.leave("recovery-copy")}>
              Open recovery copy
            </Button>
          {/if}
        </div>
      </div>
    {/if}

    {#if capture.controlError?.action === "pause" && phase === "recording"}
      <div class="recording-control-error" role="alert">
        <div>
          <strong>Recording is still active.</strong>
          <span>The pause request failed: {capture.controlError.message}</span>
        </div>
        <Button size="sm" variant="ghost" onclick={() => session.dismiss("control")}>Dismiss</Button>
      </div>
    {/if}

    {#if capture.meetingAudioMissing}
      <div class="recording-source-warning" role="alert">
        <div>
          <strong>Other participants aren't being captured.</strong>
          <span>
            Your microphone is still recording, but Windows meeting audio is unavailable. Make
            sure the call is playing through this computer, then pause and resume to retry.
          </span>
        </div>
        <Button
          size="sm"
          variant="ghost"
          onclick={() => session.dismiss("meetingAudio")}>Dismiss</Button
        >
      </div>
    {/if}

    {#if capture.transcriptionUnavailable}
      <div class="recording-transcription-warning" role="alert">
        <div>
          <strong>Live transcription isn't available.</strong>
          <span>
            The local speech model could not load. Your typed notes still save for recovery, but
            spoken audio will not appear in the transcript. Finish this meeting, then repair the
            model pack in Settings.
          </span>
        </div>
        <div class="recording-start-error-actions">
          <Button size="sm" variant="outline" onclick={() => void goto("/settings?section=models")}>
            Repair model
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onclick={() => session.dismiss("transcription")}>Dismiss</Button
          >
        </div>
      </div>
    {/if}

    {#if capture.audioPressure}
      <div class="recording-audio-warning" role="alert">
        <div>
          <strong>Audio processing briefly fell behind.</strong>
          <span>
            Some {audioPressureSource} may be missing from the transcript. Close other heavy apps
            if this continues.
          </span>
        </div>
        <Button size="sm" variant="ghost" onclick={() => session.dismiss("audioPressure")}>Dismiss</Button>
      </div>
    {/if}

    <div class="notepad-hero">
      <div class="notepad-header">
        <span class="notepad-label">My notes</span>
        <span class="note-save-status" aria-live="polite">
          {capture.recoveryStatus || "Sketch while you listen"}
        </span>
      </div>
      <Textarea
        bind:value={capture.notes}
        disabled={stopping}
        oninput={session.notesChanged}
        placeholder="Jot rough thoughts, action items, or quotes here…"
        class="notepad-textarea"
      />
    </div>

    <div class="recording-actions">
      <Button
        variant="outline"
        onclick={() => void session.cancel()}
        disabled={phase === "starting" || stopping || capture.controlBusy}
      >
        {phase === "failed"
          ? "Back to meetings"
          : phase === "finish_failed"
            ? "Keep recovery copy"
            : "Cancel"}
      </Button>
      <Button
        variant="secondary"
        onclick={() => void session.pause()}
        disabled={!session.canPause}
      >{capture.controlBusy && phase === "recording" ? "Pausing..." : "Pause"}</Button>
      <Button
        variant="destructive"
        onclick={() => void session.finish("save")}
        disabled={!session.canStop || stopping || capture.controlBusy}
      >
        Stop meeting
      </Button>
    </div>

    {#if capture.quitBlocked}
      <div class="recording-quit-warning" role="alert">
        <div>
          <strong>Finish this meeting before quitting.</strong>
          <span>Stop the meeting to save it, or cancel to discard it intentionally.</span>
        </div>
        <Button variant="ghost" size="sm" onclick={() => session.dismiss("quit")}>Dismiss</Button>
      </div>
    {/if}
  </div>
</div>

<Dialog.Root open={phase === "paused"} onOpenChange={onDialogOpenChange}>
  <Dialog.Content showCloseButton={false} class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Meeting paused</Dialog.Title>
    </Dialog.Header>
    {#if capture.controlError?.action === "resume"}
      <div class="pause-resume-error" role="alert">
        <strong>Recording is still paused.</strong>
        <span>{capture.controlError.message}</span>
        <Button
          size="sm"
          variant="outline"
          onclick={() => void openMicrophonePrivacySettings()}
        >Microphone settings</Button>
      </div>
    {/if}
    <div class="pause-timer">{timerText}</div>
    <div class="pause-actions">
      <Button class="w-full" onclick={() => void session.resume()} disabled={!session.canResume}>
        {capture.controlBusy ? "Resuming..." : "Resume recording"}
      </Button>
      <Button class="w-full" variant="secondary" onclick={() => void session.finish('generate')} disabled={stopping || capture.controlBusy}>
        Generate meeting notes
      </Button>
      <Button class="w-full" variant="secondary" onclick={() => void session.finish('enhance')} disabled={stopping || capture.controlBusy}>
        Enhance meeting notes
      </Button>
    </div>
    <Button variant="ghost" class="pause-stop-link w-full" onclick={() => void session.finish('save')} disabled={stopping || capture.controlBusy}>
      Stop meeting without generating notes
    </Button>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={navigationDialogOpen} onOpenChange={onNavigationDialogOpenChange}>
  <Dialog.Content showCloseButton={false} class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>
        {phase === "starting"
          ? "Recording is still starting"
          : phase === "finish_failed"
            ? "This meeting isn't saved yet"
            : "Leave this recording?"}
      </Dialog.Title>
      <Dialog.Description>
        {#if phase === "starting"}
          Stay while Windows finishes startup, or prepare a recovery copy and leave without
            creating an empty meeting.
        {:else if capture.controlBusy}
          Windows is finishing the current pause or resume request. Leave actions wait for that
          transition before stopping capture.
        {:else if phase === "finish_failed"}
          Retry the save, keep the recovery copy and leave, or stay here without losing your work.
        {:else if phase === "failed"}
          Keep your notes as a recovery copy and leave, or stay here and try starting again.
        {:else}
          Finish and save the meeting, keep only its recovery copy, or continue recording here.
        {/if}
      </Dialog.Description>
    </Dialog.Header>
    <div class="navigation-guard-actions">
      <Button variant="outline" onclick={continueRecordingHere}>
        {phase === "starting" || phase === "failed" || phase === "finish_failed"
          ? "Stay here"
          : "Continue recording"}
      </Button>
      <Button
        variant="secondary"
        onclick={keepRecoveryAndLeave}
        disabled={capture.controlBusy}
      >
        Keep recovery copy and leave
      </Button>
      {#if session.canFinish}
        <Button onclick={finishBeforeLeaving} disabled={capture.controlBusy}>
          {phase === "finish_failed" ? "Retry saving" : "Finish and save"}
        </Button>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<LiveTranscript lines={capture.lines} bind:open={transcriptOpen} />


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
  .recording-finish-error,
  .recording-control-error,
  .recording-source-warning,
  .recording-transcription-warning,
  .recording-audio-warning {
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
  .recording-finish-error > div:first-child,
  .recording-control-error > div:first-child,
  .recording-source-warning > div:first-child,
  .recording-transcription-warning > div:first-child,
  .recording-audio-warning > div:first-child {
    display: grid;
    gap: 3px;
  }

  .recording-start-error span,
  .recording-finish-error span,
  .recording-control-error span,
  .recording-source-warning span,
  .recording-transcription-warning span,
  .recording-audio-warning span {
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

  .pause-resume-error {
    display: grid;
    gap: 6px;
    padding: 12px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--ink);
    font-size: 13px;
  }

  .pause-resume-error span {
    color: var(--text-muted);
  }

  .pause-resume-error :global(.btn) {
    justify-self: start;
    margin-top: 2px;
  }

  .pause-actions {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }

  .navigation-guard-actions {
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
    .recording-finish-error,
    .recording-control-error,
    .recording-source-warning,
    .recording-transcription-warning,
    .recording-audio-warning {
      align-items: stretch;
      flex-direction: column;
    }

    .recording-start-error-actions {
      flex-wrap: wrap;
    }
  }
</style>
