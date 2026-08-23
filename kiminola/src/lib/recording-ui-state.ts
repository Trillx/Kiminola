export type RecordingPhase =
  | "starting"
  | "recording"
  | "paused"
  | "failed"
  | "finish_failed"
  | "stopping";

export interface ElapsedClockState {
  accumulatedMs: number;
  activeSinceMs: number | null;
}

export function createElapsedClock(initialSeconds = 0): ElapsedClockState {
  return {
    accumulatedMs: Math.max(0, initialSeconds) * 1_000,
    activeSinceMs: null,
  };
}

export function activateElapsedClock(
  clock: ElapsedClockState,
  nowMs: number,
): ElapsedClockState {
  if (clock.activeSinceMs !== null) return clock;
  return { ...clock, activeSinceMs: nowMs };
}

export function freezeElapsedClock(
  clock: ElapsedClockState,
  nowMs: number,
): ElapsedClockState {
  if (clock.activeSinceMs === null) return clock;
  return {
    accumulatedMs: clock.accumulatedMs + Math.max(0, nowMs - clock.activeSinceMs),
    activeSinceMs: null,
  };
}

export function elapsedClockSeconds(clock: ElapsedClockState, nowMs: number): number {
  const activeMs =
    clock.activeSinceMs === null ? 0 : Math.max(0, nowMs - clock.activeSinceMs);
  return Math.floor((clock.accumulatedMs + activeMs) / 1_000);
}

const PHASE_LABELS: Record<RecordingPhase, string> = {
  starting: "Starting",
  recording: "Recording",
  paused: "Paused",
  failed: "Couldn't start",
  finish_failed: "Save failed",
  stopping: "Finishing",
};

export function recordingPhaseLabel(phase: RecordingPhase): string {
  return PHASE_LABELS[phase];
}

export function shouldAdvanceElapsed(phase: RecordingPhase): boolean {
  return phase === "recording";
}

export function canStopRecording(phase: RecordingPhase): boolean {
  return phase === "recording" || phase === "paused";
}

export function canPauseRecording(phase: RecordingPhase, controlBusy: boolean): boolean {
  return phase === "recording" && !controlBusy;
}

export function canResumeRecording(phase: RecordingPhase, controlBusy: boolean): boolean {
  return phase === "paused" && !controlBusy;
}

export function canRetryFinish(phase: RecordingPhase): boolean {
  return phase === "finish_failed";
}

export function canHandleStopShortcut(
  phase: RecordingPhase,
  controlBusy: boolean,
): boolean {
  return !controlBusy && (canStopRecording(phase) || canRetryFinish(phase));
}

export function shouldGuardRecordingNavigation(phase: RecordingPhase): boolean {
  return phase === "starting" || canStopRecording(phase) || canRetryFinish(phase);
}

export function shouldDiscardAutoDraft(
  recoveryDraftCreated: boolean,
  nativeSessionActive: boolean,
): boolean {
  return recoveryDraftCreated && nativeSessionActive;
}
