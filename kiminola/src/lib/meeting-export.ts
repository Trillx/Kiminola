// @ts-expect-error Node strip-types tests import the production session.
import { meetingNotesAutosave } from "./meeting-notes-session.ts";
// @ts-expect-error Node strip-types tests import the production IPC wrappers.
import { exportNotesMarkdown, exportTranscriptText, saveNotesExport, saveTranscriptExport } from "./tauri.ts";

export type MeetingExportAction = "copy-notes" | "copy-transcript" | "save-notes" | "save-transcript";

export async function exportMeeting(
  action: MeetingExportAction,
  meetingId: number,
): Promise<{ message: string; path?: string }> {
  // Transcript exports only read saved transcript data and need no notes write.
  if (action === "copy-notes" || action === "save-notes") {
    await meetingNotesAutosave.flush(meetingId);
  }
  switch (action) {
    case "copy-notes":
      await navigator.clipboard.writeText(await exportNotesMarkdown(meetingId));
      return { message: "Notes copied to clipboard." };
    case "copy-transcript":
      await navigator.clipboard.writeText(await exportTranscriptText(meetingId));
      return { message: "Transcript copied to clipboard." };
    case "save-notes":
      return { message: "Notes saved.", path: await saveNotesExport(meetingId) };
    case "save-transcript":
      return { message: "Transcript saved.", path: await saveTranscriptExport(meetingId) };
  }
}
