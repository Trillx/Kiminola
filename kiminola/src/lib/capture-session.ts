import type {
  AudioPressureEvent,
  LibraryLocation,
  NoteDraftDetail,
  RecordingStartStatus,
  RecordingStopResult,
  TranscriptEvent,
  TranscriptLine,
} from "./tauri";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { applyTranscriptEvent, finalizedTranscript, offsetTranscriptEvent, recoverableTranscript } from "./transcript-state.ts";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { locationFromSearchParams } from "./library-tree.ts";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { draftNotepad, type NotepadKey, type NotepadStatus, type NotepadStore, type RecoveryCheckpoint } from "./notepad-store.ts";

/** "idle" means no Capture session exists; every other phase blocks App updates. */
export type CapturePhase =
  | "idle"
  | "starting"
  | "recording"
  | "paused"
  | "failed"
  | "finish_failed"
  | "stopping";

export type FinishMode = "save" | "generate" | "enhance";

export type CaptureWarning = "audioPressure" | "meetingAudio" | "transcription" | "control" | "quit";

type Unlisten = () => void;

/** Everything native a Capture session needs; production wires tauri.ts and SvelteKit. */
export interface CapturePort {
  startRecording(): Promise<RecordingStartStatus>;
  stopRecording(): Promise<RecordingStopResult>;
  pauseRecording(): Promise<void>;
  resumeRecording(): Promise<RecordingStartStatus>;
  saveMeeting(input: {
    title: string;
    durationSeconds: number;
    notepad: string;
    segments: TranscriptLine[];
    noteDraftId: number | null;
    location: LibraryLocation | null;
  }): Promise<number>;
  createNoteDraft(location: LibraryLocation | null): Promise<number>;
  getNoteDraft(id: number): Promise<NoteDraftDetail>;
  deleteNoteDraft(id: number): Promise<void>;
  onTranscriptEvent(handler: (event: TranscriptEvent) => void): Promise<Unlisten>;
  onAudioPressure(handler: (event: AudioPressureEvent) => void): Promise<Unlisten>;
  onShortcutTriggered(handler: () => void): Promise<Unlisten>;
  onRecordingQuitBlocked(handler: () => void): Promise<Unlisten>;
  navigate(href: string): Promise<void>;
  /** Monotonic milliseconds; wall-clock jumps must not change the elapsed time. */
  now(): number;
  /** Repeating timer; returns its cancel function. */
  every(milliseconds: number, tick: () => void): () => void;
}

export interface CaptureDependencies {
  port: CapturePort;
  notepad: NotepadStore;
  /** The Library location new meetings use when the request names none. */
  lastLocation: () => LibraryLocation | null;
  log?: (message: string, error: unknown) => void;
}

/** Observable session state. Production passes a `$state` proxy; tests a plain object. */
export interface CaptureState {
  phase: CapturePhase;
  title: string;
  elapsedSeconds: number;
  notes: string;
  lines: TranscriptLine[];
  noteDraftId: number | null;
  recoveryStatus: string;
  startError: string | null;
  finishError: string | null;
  controlBusy: boolean;
  controlError: { action: "pause" | "resume"; message: string } | null;
  audioPressure: AudioPressureEvent | null;
  meetingAudioMissing: boolean;
  transcriptionUnavailable: boolean;
  quitBlocked: boolean;
  /** A leave was requested during startup and runs as soon as startup settles. */
  exitPending: boolean;
}

export function initialCaptureState(): CaptureState {
  return {
    phase: "idle",
    title: "",
    elapsedSeconds: 0,
    notes: "",
    lines: [],
    noteDraftId: null,
    recoveryStatus: "",
    startError: null,
    finishError: null,
    controlBusy: false,
    controlError: null,
    audioPressure: null,
    meetingAudioMissing: false,
    transcriptionUnavailable: false,
    quitBlocked: false,
    exitPending: false,
  };
}

const PHASE_LABELS: Record<CapturePhase, string> = {
  idle: "Finished",
  starting: "Starting",
  recording: "Recording",
  paused: "Paused",
  failed: "Couldn't start",
  finish_failed: "Save failed",
  stopping: "Finishing",
};

export function capturePhaseLabel(phase: CapturePhase): string {
  return PHASE_LABELS[phase];
}

/**
 * How a session ends. Every variant runs the same ordered teardown:
 * settle pause/resume -> (defer while starting) -> freeze clock -> checkpoint
 * -> stop native capture -> checkpoint final transcript -> variant step ->
 * close -> navigate.
 */
type Exit =
  | { kind: "finish"; mode: FinishMode }
  | { kind: "leave"; to: string | "recovery-copy" }
  | { kind: "cancel" }
  | { kind: "release" };

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return fallback;
}

function meetingTitle(startedAt: Date): string {
  const date = startedAt.toLocaleDateString("en-US", { month: "short", day: "numeric" });
  const time = startedAt.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  return `Meeting · ${date}, ${time}`;
}

const RECOVERY_STATUS: Record<Exclude<NotepadStatus, "idle">, string> = {
  saving: "Saving for recovery...",
  saved: "Saved for recovery",
  error: "Could not save recovery copy",
};

/**
 * The app's single Capture session: from opening the recording view to the
 * Meeting being saved (or the recovery copy kept, or the capture cancelled).
 *
 * Invariants callers can rely on:
 * - pause/resume never interleave with each other or with an exit;
 * - a leave requested while starting waits for startup, then tears down;
 * - the recovery copy is checkpointed before native capture stops and again
 *   with the final transcript, all through the Notepad store's draft queue;
 * - finishing saves with the session's Note draft as the idempotency key, so
 *   a retry after a lost response cannot duplicate the Meeting;
 * - the Library location is resolved once, when the session's draft is prepared.
 */
export function createCaptureSession(
  { port, notepad, lastLocation, log = (message, error) => console.error(message, error) }: CaptureDependencies,
  state: CaptureState = initialCaptureState(),
) {
  let nativeActive = false;
  let requestedDraftId = Number.NaN;
  let explicitLocation: LibraryLocation | null = null;
  let destination: LibraryLocation | null = null;
  let destinationResolved = false;
  let recoveryKey: NotepadKey | null = null;
  let recoveryDraftCreated = false;
  let accumulatedMs = 0;
  let activeSinceMs: number | null = null;
  let lastCheckpointSecond = 0;
  let transcriptOffsetMs = 0;
  let finalizationWarning: string | null = null;
  let pendingFinishMode: FinishMode | null = null;
  let deferredExit: Exit | null = null;
  let controlSettled: Promise<void> = Promise.resolve();
  let unlisteners: Unlisten[] = [];
  let stopTicker: (() => void) | null = null;
  let unwatchRecovery: (() => void) | null = null;
  // Serializes session lifecycles: a new session opens only after the last teardown.
  let lifecycle: Promise<void> = Promise.resolve();
  let opening = false;
  let exiting: Promise<void> | null = null;

  const canStop = () => state.phase === "recording" || state.phase === "paused";
  const canRetry = () => state.phase === "finish_failed";
  const canHandleStop = () => !state.controlBusy && (canStop() || canRetry());

  /* ---------- clock ---------- */

  function syncClock(nowMs = port.now()) {
    const activeMs = activeSinceMs === null ? 0 : Math.max(0, nowMs - activeSinceMs);
    state.elapsedSeconds = Math.floor((accumulatedMs + activeMs) / 1_000);
  }

  function runClock() {
    const nowMs = port.now();
    if (activeSinceMs === null) activeSinceMs = nowMs;
    syncClock(nowMs);
  }

  function freezeClock() {
    const nowMs = port.now();
    if (activeSinceMs !== null) {
      accumulatedMs += Math.max(0, nowMs - activeSinceMs);
      activeSinceMs = null;
    }
    syncClock(nowMs);
  }

  function restoreClock(seconds: number) {
    state.elapsedSeconds = Math.max(0, Math.floor(seconds));
    accumulatedMs = state.elapsedSeconds * 1_000;
    activeSinceMs = null;
    lastCheckpointSecond = state.elapsedSeconds;
  }

  function tick() {
    if (state.phase !== "recording") return;
    syncClock();
    if (state.elapsedSeconds - lastCheckpointSecond >= 5) {
      lastCheckpointSecond = state.elapsedSeconds;
      checkpoint();
    }
  }

  /* ---------- recovery copy ---------- */

  function recoveryCheckpoint(): RecoveryCheckpoint {
    return {
      durationSeconds: state.elapsedSeconds,
      transcript: recoverableTranscript(state.lines),
      location: destination,
    };
  }

  function checkpoint() {
    if (recoveryKey) notepad.edit(recoveryKey, state.notes, recoveryCheckpoint());
  }

  async function flushCheckpoint() {
    if (!recoveryKey) return;
    // A failed checkpoint stays queued in the Notepad store; the Meeting save
    // still receives the current notepad directly.
    await notepad.save(recoveryKey, state.notes, recoveryCheckpoint()).catch(() => undefined);
  }

  function watchRecovery(draftId: number) {
    state.noteDraftId = draftId;
    recoveryKey = draftNotepad(draftId);
    unwatchRecovery?.();
    unwatchRecovery = notepad.subscribe(recoveryKey, (status) => {
      if (status !== "idle") state.recoveryStatus = RECOVERY_STATUS[status];
    });
  }

  function resolveDestination(recoveryLocation: LibraryLocation | null) {
    if (destinationResolved) return;
    destination = explicitLocation ?? recoveryLocation ?? lastLocation();
    destinationResolved = true;
  }

  async function prepareDraft() {
    if (Number.isFinite(requestedDraftId)) {
      // Flush-before-read: a note-page edit queued for this draft lands first.
      const draft = await notepad.read(
        () => port.getNoteDraft(requestedDraftId),
        (error) => log("Failed to save pending notes before recording:", error),
      );
      const pending = notepad.pending(draftNotepad(draft.id));
      resolveDestination(pending?.recovery?.location ?? draft.recovery_location);
      state.notes = pending?.text ?? draft.raw_markdown;
      restoreClock(pending?.recovery?.durationSeconds ?? draft.recovery_duration_seconds);
      transcriptOffsetMs = state.elapsedSeconds * 1_000;
      state.lines = pending?.recovery?.transcript ?? draft.recovery_transcript;
      watchRecovery(draft.id);
      return;
    }

    resolveDestination(null);
    try {
      const draftId = await port.createNoteDraft(destination);
      recoveryDraftCreated = true;
      watchRecovery(draftId);
      state.recoveryStatus = "Recovery copy ready";
    } catch (error) {
      log("Failed to create a recovery draft:", error);
      state.recoveryStatus = "Recovery copy unavailable";
    }
  }

  /* ---------- native events ---------- */

  function applyStartStatus(status: RecordingStartStatus) {
    state.meetingAudioMissing = !status.meeting_audio_available;
    state.transcriptionUnavailable = !status.transcription_available;
  }

  function applyStopResult(result: RecordingStopResult) {
    finalizationWarning = result.finalization_warning ?? finalizationWarning;
    for (const event of result.transcript) {
      state.lines = applyTranscriptEvent(state.lines, offsetTranscriptEvent(event, transcriptOffsetMs));
    }
  }

  function listen(): Promise<Unlisten[]> {
    return Promise.all([
      port.onTranscriptEvent((event) => {
        state.lines = applyTranscriptEvent(state.lines, offsetTranscriptEvent(event, transcriptOffsetMs));
        checkpoint();
      }),
      port.onAudioPressure((event) => {
        state.audioPressure = event;
      }),
      port.onRecordingQuitBlocked(() => {
        state.quitBlocked = true;
      }),
      // The global shortcut stops (or retries saving) only in a state where
      // that is safe; otherwise the layout treats it as "open the recorder".
      port.onShortcutTriggered(() => {
        if (canHandleStop()) stop();
      }),
    ]);
  }

  /* ---------- startup ---------- */

  async function begin(searchParams: URLSearchParams) {
    await settled();
    if (state.phase !== "idle") await teardown({ kind: "release" });
    Object.assign(state, initialCaptureState(), { phase: "starting", title: meetingTitle(new Date()) });
    opening = false;
    const draftParam = searchParams.get("draft");
    requestedDraftId = draftParam ? Number(draftParam) : Number.NaN;
    explicitLocation = locationFromSearchParams(searchParams);
    destination = null;
    destinationResolved = false;
    recoveryKey = null;
    recoveryDraftCreated = false;
    accumulatedMs = 0;
    activeSinceMs = null;
    lastCheckpointSecond = 0;
    transcriptOffsetMs = 0;
    finalizationWarning = null;
    pendingFinishMode = null;
    stopTicker = port.every(1_000, tick);
    await start();
  }

  async function start() {
    state.phase = "starting";
    if (await runDeferredExit()) return;
    if (!unlisteners.length) {
      try {
        unlisteners = await listen();
      } catch (error) {
        if (await runDeferredExit()) return;
        state.phase = "failed";
        state.startError = errorMessage(error, "The recording page could not initialize.");
        log("Failed to initialize recording:", error);
        return;
      }
    }
    state.startError = null;
    state.audioPressure = null;
    state.meetingAudioMissing = false;
    state.transcriptionUnavailable = false;
    finalizationWarning = null;
    try {
      if (state.noteDraftId === null) await prepareDraft();
      if (await runDeferredExit()) return;
      const status = await port.startRecording();
      nativeActive = true;
      applyStartStatus(status);
      if (await runDeferredExit()) return;
      runClock();
      state.phase = "recording";
    } catch (error) {
      nativeActive = false;
      if (await runDeferredExit()) return;
      state.phase = "failed";
      state.startError = errorMessage(error, "Windows could not start the microphone.");
      log("Failed to start recording:", error);
    }
  }

  async function runDeferredExit(): Promise<boolean> {
    const exit = deferredExit;
    if (!exit) return false;
    deferredExit = null;
    await teardown(exit);
    return true;
  }

  /* ---------- pause / resume ---------- */

  function beginControl(): () => void {
    let settle!: () => void;
    controlSettled = new Promise<void>((resolve) => {
      settle = resolve;
    });
    state.controlBusy = true;
    state.controlError = null;
    return () => {
      state.controlBusy = false;
      settle();
    };
  }

  async function pause() {
    if (state.phase !== "recording" || state.controlBusy) return;
    const done = beginControl();
    try {
      await port.pauseRecording();
      freezeClock();
      state.phase = "paused";
      await flushCheckpoint();
    } catch (error) {
      state.controlError = {
        action: "pause",
        message: errorMessage(error, "Windows could not pause audio capture."),
      };
      log("Failed to pause recording:", error);
    } finally {
      done();
    }
  }

  async function resume() {
    if (state.phase !== "paused" || state.controlBusy) return;
    const done = beginControl();
    try {
      applyStartStatus(await port.resumeRecording());
      runClock();
      state.phase = "recording";
    } catch (error) {
      state.controlError = {
        action: "resume",
        message: errorMessage(error, "Windows could not restart audio capture."),
      };
      log("Failed to resume recording:", error);
    } finally {
      done();
    }
  }

  /* ---------- exits ---------- */

  async function exit(request: Exit) {
    await controlSettled;
    if (state.phase === "idle" || state.phase === "stopping" || exiting) return;
    if (request.kind === "finish" && !canStop() && !canRetry()) return;
    if (state.phase === "starting") {
      deferredExit ??= request;
      state.exitPending = true;
      if (request.kind === "leave") state.recoveryStatus = "Preparing recovery copy before leaving...";
      return;
    }
    await teardown(request);
  }

  function teardown(request: Exit): Promise<void> {
    const running = runTeardown(request).finally(() => {
      if (exiting === running) exiting = null;
    });
    exiting = running;
    return running;
  }

  async function runTeardown(request: Exit) {
    const finishing = request.kind === "finish";
    const discardDraft = request.kind === "cancel" && recoveryDraftCreated && nativeActive;
    const draftId = state.noteDraftId;
    freezeClock();
    state.phase = "stopping";
    state.finishError = null;
    if (request.kind === "finish") pendingFinishMode = request.mode;

    // A cancelled, auto-created draft is deleted, not checkpointed. Close its
    // document first so no transcript event can queue a write to a deleted row.
    if (discardDraft && recoveryKey) {
      const key = recoveryKey;
      recoveryKey = null;
      await notepad.discard(key);
    }

    await flushCheckpoint();
    if (nativeActive) {
      try {
        const result = await port.stopRecording();
        nativeActive = false;
        applyStopResult(result);
      } catch (error) {
        if (finishing) return failFinish(error);
        // Leaving keeps the recovery copy; closing retries the stop once more.
        log("Failed to stop recording:", error);
      }
      await flushCheckpoint();
    }

    let href: string | null = null;
    if (request.kind === "finish") {
      let meetingId: number;
      try {
        meetingId = await port.saveMeeting({
          title: state.title,
          durationSeconds: state.elapsedSeconds,
          notepad: state.notes,
          segments: finalizedTranscript(state.lines),
          noteDraftId: draftId,
          location: destination,
        });
      } catch (error) {
        return failFinish(error);
      }
      // The draft is now attached to the Meeting; further recovery writes would be rejected.
      if (recoveryKey) {
        const key = recoveryKey;
        recoveryKey = null;
        await notepad.discard(key);
      }
      href = meetingHref(meetingId, request.mode);
    } else if (discardDraft) {
      if (draftId !== null) {
        await port.deleteNoteDraft(draftId).catch((error) => {
          log("Failed to remove cancelled recovery draft:", error);
        });
      }
      href = "/";
    } else if (request.kind === "leave") {
      href = request.to === "recovery-copy" ? (draftId === null ? "/" : `/note/${draftId}`) : request.to;
    } else if (request.kind === "cancel") {
      href = "/";
    }

    try {
      if (href !== null) await port.navigate(href);
    } catch (error) {
      log("Failed to leave the recording view:", error);
    } finally {
      close();
    }
  }

  async function settled(): Promise<void> {
    while (exiting) await exiting;
  }

  function failFinish(error: unknown) {
    state.phase = "finish_failed";
    state.finishError = errorMessage(error, "The meeting could not be finalized or saved.");
    log("Failed to finish meeting:", error);
  }

  function meetingHref(meetingId: number, mode: FinishMode): string {
    const params = new URLSearchParams();
    if (mode !== "save") params.set("mode", mode);
    if (finalizationWarning) params.set("warning", "transcript-finalization");
    const query = params.toString();
    return `/meeting/${meetingId}${query ? `?${query}` : ""}`;
  }

  function close() {
    for (const unlisten of unlisteners) unlisten();
    unlisteners = [];
    stopTicker?.();
    stopTicker = null;
    unwatchRecovery?.();
    unwatchRecovery = null;
    recoveryKey = null;
    if (nativeActive) {
      // Second best-effort stop for a capture that refused to stop while leaving.
      nativeActive = false;
      void port.stopRecording().catch(() => undefined);
    }
    state.exitPending = false;
    state.phase = "idle";
  }

  function stop() {
    if (canRetry()) retryFinish();
    else void exit({ kind: "finish", mode: "save" });
  }

  function retryFinish() {
    if (pendingFinishMode && canRetry()) void exit({ kind: "finish", mode: pendingFinishMode });
  }

  return {
    state,

    /** A session exists (any phase but idle): updates wait, the shortcut stops instead of starts. */
    get active() {
      return state.phase !== "idle";
    },
    get canPause() {
      return state.phase === "recording" && !state.controlBusy;
    },
    get canResume() {
      return state.phase === "paused" && !state.controlBusy;
    },
    get canStop() {
      return canStop();
    },
    get canRetry() {
      return canRetry();
    },
    /** "Finish and save" (or "Retry saving") is available without racing a control request. */
    get canFinish() {
      return canHandleStop();
    },
    /** Leaving the recording view needs an explicit decision. */
    get guardsNavigation() {
      return state.phase === "starting" || canStop() || canRetry();
    },

    /** Open a session for the recording view. `?draft=` resumes a Note draft; location params pick the destination. */
    open(searchParams: URLSearchParams): Promise<void> {
      opening = true;
      const run = lifecycle.then(() => begin(searchParams)).finally(() => {
        opening = false;
      });
      lifecycle = run.catch(() => undefined);
      return run;
    },
    /** Try startup again after it failed. */
    retryStart(): Promise<void> {
      return state.phase === "failed" ? start() : Promise.resolve();
    },
    pause,
    resume,
    /** Stop capture, save the Meeting, and open it. */
    finish(mode: FinishMode): Promise<void> {
      return exit({ kind: "finish", mode });
    },
    /** Finish with "save", or retry the failed save with its original mode. */
    stop,
    retryFinish,
    /** Keep the recovery copy and go to `destination` (or the recovery copy itself). */
    leave(destination: string | "recovery-copy"): Promise<void> {
      return exit({ kind: "leave", to: destination });
    },
    /** Discard an auto-created recovery copy; otherwise keep it. Then go home. */
    cancel(): Promise<void> {
      return exit({ kind: "cancel" });
    },
    /** The recording view unmounted without choosing an exit: keep the recovery copy, stop capture. */
    release() {
      if (exiting) return;
      // Unmounted before startup ran: tear down as soon as startup begins.
      if (opening) deferredExit ??= { kind: "release" };
      else if (state.phase !== "idle") void exit({ kind: "release" });
    },
    /** The notepad changed; checkpoint it into the recovery copy (debounced). */
    notesChanged() {
      checkpoint();
    },
    dismiss(warning: CaptureWarning) {
      if (warning === "audioPressure") state.audioPressure = null;
      else if (warning === "meetingAudio") state.meetingAudioMissing = false;
      else if (warning === "transcription") state.transcriptionUnavailable = false;
      else if (warning === "control") state.controlError = null;
      else state.quitBlocked = false;
    },
    /** Resolves when no teardown is in flight; registered with the update boundary. */
    settled,
  };
}

export type CaptureSession = ReturnType<typeof createCaptureSession>;
