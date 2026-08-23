export type RecordingPhase =
  | "starting"
  | "recording"
  | "paused"
  | "failed"
  | "finish_failed"
  | "stopping";

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

export function shouldDiscardAutoDraft(
  recoveryDraftCreated: boolean,
  nativeSessionActive: boolean,
): boolean {
  return recoveryDraftCreated && nativeSessionActive;
}
