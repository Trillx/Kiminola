import type { LibraryLocation, TranscriptLine } from "./tauri";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { createDraftAutosave, type DraftAutosave, type DraftAutosaveStatus } from "./draft-autosave.ts";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { registerPendingSave } from "./pending-work.ts";
// @ts-expect-error Node strip-types tests import the production IPC wrappers.
import { updateNoteDraft, updateNoteDraftRecovery, updateNotes } from "./tauri.ts";

/** One Notepad document: a Meeting's notes or a Note draft. */
export type NotepadKey = `meeting:${number}` | `draft:${number}`;

export function meetingNotepad(meetingId: number): NotepadKey {
  return `meeting:${meetingId}`;
}

export function draftNotepad(draftId: number): NotepadKey {
  return `draft:${draftId}`;
}

/** What a Capture session adds to a Note draft so an interrupted recording can resume. */
export interface RecoveryCheckpoint {
  durationSeconds: number;
  transcript: TranscriptLine[];
  location: LibraryLocation | null;
}

export interface NotepadValue {
  text: string;
  recovery?: RecoveryCheckpoint;
}

/** A checkpoint, or a factory evaluated only when its write runs (not on every edit). */
export type RecoveryInput = RecoveryCheckpoint | (() => RecoveryCheckpoint);

/** "idle" means the document has no write since the store started (or it was discarded). */
export type NotepadStatus = "idle" | DraftAutosaveStatus;
type Listener = (status: NotepadStatus) => void;

export interface NotepadPort {
  updateNotes(meetingId: number, text: string): Promise<void>;
  updateNoteDraft(draftId: number, text: string): Promise<void>;
  updateNoteDraftRecovery(
    draftId: number,
    text: string,
    durationSeconds: number,
    transcript: TranscriptLine[],
    location: LibraryLocation | null,
  ): Promise<void>;
}

/**
 * App-wide owner of unsaved Notepad text. Each document has exactly one
 * debounced, serialized write queue, so every writer of a document (the note
 * page, the meeting page, a Capture session's recovery checkpoints) shares
 * latest-wins ordering. Unsaved text outlives the page that typed it until a
 * write succeeds or the document is discarded.
 */
export interface NotepadStore {
  /**
   * Schedule a debounced write; continuous edits still write within the
   * autosave's maximum wait. A text-only edit keeps an unsaved recovery checkpoint.
   */
  edit(key: NotepadKey, text: string, recovery?: RecoveryInput): void;
  /** Write now, behind any in-flight write of the same document. Rejects on failure. */
  save(key: NotepadKey, text: string, recovery?: RecoveryInput): Promise<void>;
  /** Write pending text for one document, or all of them. Rejects if any write fails. */
  flush(key?: NotepadKey): Promise<void>;
  /** The newest value not yet confirmed saved; prefer it over a database read. */
  pending(key: NotepadKey): NotepadValue | undefined;
  /** Flush every document, then read. A failed write is reported, never turned into a failed read. */
  read<T>(load: () => Promise<T>, onSaveError?: (error: unknown) => void): Promise<T>;
  /** Status of one document, or "all" for the worst status across documents. Emits immediately. */
  subscribe(key: NotepadKey | "all", listener: Listener): () => void;
  /**
   * Drop a document whose row was deleted or attached to a Meeting, after any
   * in-flight write finishes. Its unsaved text is abandoned deliberately.
   */
  discard(key: NotepadKey): Promise<void>;
}

interface Entry {
  engine: DraftAutosave<NotepadValue>;
  status: NotepadStatus;
}

export function createNotepadStore(
  port: NotepadPort,
  { delayMs = 500 }: { delayMs?: number } = {},
): NotepadStore {
  const entries = new Map<NotepadKey, Entry>();
  const listeners = new Map<NotepadKey | "all", Set<Listener>>();

  function write(key: NotepadKey, value: NotepadValue): Promise<void> {
    const [kind, rawId] = key.split(":");
    const id = Number(rawId);
    if (kind === "meeting") return port.updateNotes(id, value.text);
    const recovery = value.recovery;
    if (!recovery) return port.updateNoteDraft(id, value.text);
    return port.updateNoteDraftRecovery(
      id,
      value.text,
      recovery.durationSeconds,
      recovery.transcript,
      recovery.location,
    );
  }

  function overallStatus(): NotepadStatus {
    const statuses = [...entries.values()].map((entry) => entry.status);
    if (statuses.includes("error")) return "error";
    if (statuses.includes("saving")) return "saving";
    return statuses.includes("saved") ? "saved" : "idle";
  }

  function statusOf(key: NotepadKey | "all"): NotepadStatus {
    return key === "all" ? overallStatus() : entries.get(key)?.status ?? "idle";
  }

  function notify(key: NotepadKey) {
    listeners.get(key)?.forEach((listener) => listener(statusOf(key)));
    listeners.get("all")?.forEach((listener) => listener(overallStatus()));
  }

  function entryFor(key: NotepadKey): Entry {
    let entry = entries.get(key);
    if (!entry) {
      const created: Entry = {
        status: "idle",
        engine: createDraftAutosave<NotepadValue>(
          (value) => write(key, value),
          (status) => {
            created.status = status;
            notify(key);
          },
          delayMs,
        ),
      };
      entry = created;
      entries.set(key, entry);
    }
    return entry;
  }

  function nextValue(entry: Entry, text: string, recovery?: RecoveryInput): NotepadValue | (() => NotepadValue) {
    // A lazy checkpoint is built from the latest capture state when the write
    // runs, so a burst of transcript events copies the transcript once.
    if (typeof recovery === "function") return () => ({ text, recovery: recovery() });
    // A text-only edit (the note page) must not drop a recovery checkpoint
    // the recording queued for the same draft but could not save yet.
    return { text, recovery: recovery ?? entry.engine.pendingValue()?.recovery };
  }

  async function flush(key?: NotepadKey): Promise<void> {
    const targets = key
      ? [entries.get(key)].filter((entry): entry is Entry => entry !== undefined)
      : [...entries.values()];
    const results = await Promise.allSettled(targets.map((entry) => entry.engine.flushPending()));
    const failure = results.find((result) => result.status === "rejected");
    if (failure?.status === "rejected") throw failure.reason;
  }

  return {
    edit(key, text, recovery) {
      const entry = entryFor(key);
      entry.engine.schedule(nextValue(entry, text, recovery));
    },

    save(key, text, recovery) {
      const entry = entryFor(key);
      return entry.engine.flush(nextValue(entry, text, recovery));
    },

    flush,

    pending(key) {
      return entries.get(key)?.engine.pendingValue();
    },

    async read(load, onSaveError = () => {}) {
      await flush().catch(onSaveError);
      return load();
    },

    subscribe(key, listener) {
      let keyed = listeners.get(key);
      if (!keyed) {
        keyed = new Set();
        listeners.set(key, keyed);
      }
      keyed.add(listener);
      listener(statusOf(key));
      return () => {
        keyed.delete(listener);
      };
    },

    async discard(key) {
      const entry = entries.get(key);
      if (!entry) return;
      entries.delete(key);
      entry.engine.cancel();
      notify(key);
      // After cancel only an in-flight write remains; wait so a later delete
      // or attach cannot race it.
      await entry.engine.flushPending();
    },
  };
}

/** The app's Notepad documents, shared by every page and the Capture session. */
export const notepad = createNotepadStore({ updateNotes, updateNoteDraft, updateNoteDraftRecovery });

// The update boundary flushes unsaved notes even when no editor is mounted.
registerPendingSave(() => notepad.flush());
