// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { createDraftAutosave, type DraftAutosaveStatus } from "./draft-autosave.ts";

type NotesSnapshot = { meetingId: number; notes: string };

/** Keep the destination and text together, including across route changes. */
export function createMeetingNotesAutosave(
  save: (meetingId: number, notes: string) => Promise<void>,
  onStatus: (status: DraftAutosaveStatus) => void,
  delayMs = 500,
) {
  const pending = new Map<number, NotesSnapshot>();
  const failed = new Set<number>();
  let scheduled: NotesSnapshot | undefined;
  const autosave = createDraftAutosave<NotesSnapshot>(
    async (snapshot) => {
      try {
        await save(snapshot.meetingId, snapshot.notes);
        failed.delete(snapshot.meetingId);
        if (pending.get(snapshot.meetingId) === snapshot) pending.delete(snapshot.meetingId);
      } catch (error) {
        failed.add(snapshot.meetingId);
        throw error;
      }
    },
    (status) => onStatus(failed.size ? "error" : status),
    delayMs,
  );

  async function flush() {
    const results = await Promise.allSettled(
      [...pending.values()].map((snapshot) => autosave.flush(snapshot)),
    );
    const failure = results.find((result) => result.status === "rejected");
    if (failure?.status === "rejected") throw failure.reason;
  }

  return {
    schedule(meetingId: number, notes: string) {
      // A new meeting must not cancel the previous meeting's pending edit.
      if (scheduled && scheduled.meetingId !== meetingId && pending.get(scheduled.meetingId) === scheduled) {
        void autosave.flush(scheduled).catch(() => undefined);
      }
      scheduled = { meetingId, notes };
      pending.set(meetingId, scheduled);
      autosave.schedule(scheduled);
    },
    pendingNotes(meetingId: number) {
      return pending.get(meetingId)?.notes;
    },
    flush,
    async close() {
      const saving = flush();
      autosave.cancel();
      await saving;
    },
  };
}

/** A write failure must not turn an existing destination into a missing meeting. */
export async function loadMeetingAfterAutosave<T>(
  autosave: { flush(): Promise<void> },
  load: () => Promise<T>,
  onSaveError: (error: unknown) => void,
): Promise<T> {
  await autosave.flush().catch(onSaveError);
  return load();
}
