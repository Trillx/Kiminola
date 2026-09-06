import { createDraftAutosave, type DraftAutosave, type DraftAutosaveStatus } from "$lib/draft-autosave";
import { registerPendingSave } from "$lib/pending-work";

// One queue per note survives navigation and serializes retries with newer edits.
interface Entry { saver: DraftAutosave<string>; onStatus?: (status: DraftAutosaveStatus) => void }
const notes = new Map<string, Entry>();

export function noteSaver(key: string, save: (text: string) => Promise<void>, onStatus?: Entry["onStatus"]): DraftAutosave<string> {
  let entry = notes.get(key);
  if (!entry) {
    const saver = createDraftAutosave(save, (status) => entry?.onStatus?.(status));
    entry = { saver, onStatus };
    notes.set(key, entry);
    registerPendingSave(() => saver.flushPending());
  }
  entry.onStatus = onStatus;
  return entry.saver;
}
