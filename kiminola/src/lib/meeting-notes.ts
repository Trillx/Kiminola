// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { createDraftAutosave, type DraftAutosaveStatus } from "./draft-autosave.ts";

type NotesSnapshot = { meetingId: number; notes: string };

/** Keep the destination and text together, including across route changes. */
export function createMeetingNotesAutosave(
  save: (meetingId: number, notes: string) => Promise<void>,
  onStatus: (status: DraftAutosaveStatus) => void,
  delayMs = 500,
) {
  let pending: NotesSnapshot | undefined;
  const autosave = createDraftAutosave<NotesSnapshot>(
    async (snapshot) => {
      await save(snapshot.meetingId, snapshot.notes);
      if (pending === snapshot) pending = undefined;
    },
    onStatus,
    delayMs,
  );

  return {
    schedule(meetingId: number, notes: string) {
      // A new meeting must not cancel the previous meeting's pending edit.
      if (pending && pending.meetingId !== meetingId) {
        void autosave.flush(pending).catch(() => undefined);
      }
      pending = { meetingId, notes };
      autosave.schedule(pending);
    },
    async flush() {
      if (pending) await autosave.flush(pending);
    },
    async close() {
      const saving = pending ? autosave.flush(pending) : Promise.resolve();
      autosave.cancel();
      await saving;
    },
  };
}
