import { goto } from "$app/navigation";
import {
  createCaptureSession,
  initialCaptureState,
  type CapturePort,
} from "$lib/capture-session";
import { libraryDestinationState } from "$lib/library-tree.svelte";
import { notepad } from "$lib/notepad-store";
import { registerPendingSave } from "$lib/pending-work";
import {
  createNoteDraft,
  deleteNoteDraft,
  getNoteDraft,
  onAudioPressure,
  onRecordingQuitBlocked,
  onShortcutTriggered,
  onTranscriptEvent,
  pauseRecording,
  resumeRecording,
  saveMeeting,
  startRecording,
  stopRecording,
} from "$lib/tauri";

const tauriCapturePort: CapturePort = {
  startRecording,
  stopRecording,
  pauseRecording,
  resumeRecording,
  saveMeeting,
  createNoteDraft,
  getNoteDraft,
  deleteNoteDraft,
  onTranscriptEvent,
  onAudioPressure,
  onShortcutTriggered,
  onRecordingQuitBlocked,
  navigate: (href) => goto(href),
  now: () => performance.now(),
  every(milliseconds, tick) {
    const timer = setInterval(tick, milliseconds);
    return () => clearInterval(timer);
  },
};

const captureState = $state(initialCaptureState());

/** The app's Capture session. The layout, update guard, and prompts read `active`. */
export const captureSession = createCaptureSession(
  {
    port: tauriCapturePort,
    notepad,
    lastLocation: () => libraryDestinationState.last,
  },
  captureState,
);

// An update must not shut down halfway through stopping or saving a meeting.
registerPendingSave(() => captureSession.settled());
