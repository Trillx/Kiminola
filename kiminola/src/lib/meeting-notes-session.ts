// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { createMeetingNotesAutosave } from "./meeting-notes.ts";
// @ts-expect-error Node strip-types tests import the production IPC wrapper.
import { updateNotes } from "./tauri.ts";

// The SPA session owns pending edits so leaving a page cannot discard them.
export const meetingNotesAutosave = createMeetingNotesAutosave(updateNotes, () => {});
