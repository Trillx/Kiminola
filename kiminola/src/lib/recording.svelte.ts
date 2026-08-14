import type { RecordedMeeting } from "./mock";

/**
 * Holds the result of the most recent recording session so the post-meeting
 * screen can render it before persistence lands. When the backend arrives,
 * stopping a recording will insert rows via Tauri `invoke` and navigate to the
 * new meeting id instead — this module goes away.
 */
let lastRecording: RecordedMeeting | null = null;

export function setLastRecording(r: RecordedMeeting) {
  lastRecording = r;
}

export function getLastRecording(): RecordedMeeting | null {
  return lastRecording;
}
