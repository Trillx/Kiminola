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
  const listeners = new Set([onStatus]);
  let currentStatus: DraftAutosaveStatus = "saved";
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
    (status) => {
      currentStatus = failed.size ? "error" : status;
      listeners.forEach((listener) => listener(currentStatus));
    },
    delayMs,
  );

  async function flush(meetingId?: number) {
    const snapshots = [...pending.values()].filter((snapshot) => meetingId === undefined || snapshot.meetingId === meetingId);
    const saving = snapshots.map((snapshot) => autosave.flush(snapshot));
    // flush cancels the shared debounce timer. Keep an unrelated edit scheduled.
    if (meetingId !== undefined && scheduled && scheduled.meetingId !== meetingId && pending.get(scheduled.meetingId) === scheduled) {
      autosave.schedule(scheduled);
    }
    const results = await Promise.allSettled(saving);
    const failure = results.find((result) => result.status === "rejected");
    if (failure?.status === "rejected") throw failure.reason;
  }

  return {
    subscribe(listener: (status: DraftAutosaveStatus) => void) {
      listeners.add(listener);
      listener(currentStatus);
      return () => { listeners.delete(listener); };
    },
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
