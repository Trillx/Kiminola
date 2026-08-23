export type RecordingPhase =
  | "starting"
  | "recording"
  | "paused"
  | "failed"
  | "stopping";

const PHASE_LABELS: Record<RecordingPhase, string> = {
  starting: "Starting",
  recording: "Recording",
  paused: "Paused",
  failed: "Couldn't start",
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

export function shouldDiscardAutoDraft(
  recoveryDraftCreated: boolean,
  nativeSessionActive: boolean,
): boolean {
  return recoveryDraftCreated && nativeSessionActive;
}
