export type DraftAutosaveStatus = "saving" | "saved" | "error";

export interface DraftAutosave<T = string> {
  schedule(value: T): void;
  flush(value: T): Promise<void>;
  flushPending(): Promise<void>;
  cancel(): void;
}

/**
 * Debounces draft writes and serializes them so an older, slower request can
 * never overwrite newer notes. `flush` joins the same queue and is used before
 * meeting finalization or navigation.
 */
export function createDraftAutosave<T = string>(
  save: (value: T) => Promise<void>,
  onStatus: (status: DraftAutosaveStatus) => void,
  delayMs = 500,
): DraftAutosave<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queue = Promise.resolve();
  let latestRequest = 0;
  let cancelled = false;
  let pending: { value: T } | undefined;

  function enqueue(snapshot: { value: T }): Promise<void> {
    const request = ++latestRequest;
    if (!cancelled) onStatus("saving");

    const run = queue.catch(() => undefined).then(() => save(snapshot.value));
    queue = run.catch(() => undefined);

    return run.then(
      () => {
        if (pending === snapshot) pending = undefined;
        if (!cancelled && request === latestRequest) onStatus("saved");
      },
      (error: unknown) => {
        if (!cancelled && request === latestRequest) onStatus("error");
        throw error;
      },
    );
  }

  return {
    schedule(value: T) {
      if (cancelled) return;
      pending = { value };
      clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        if (pending) void enqueue(pending).catch(() => undefined);
      }, delayMs);
    },

    flush(value: T) {
      clearTimeout(timer);
      timer = undefined;
      pending = { value };
      return enqueue(pending);
    },

    flushPending() {
      clearTimeout(timer);
      timer = undefined;
      return pending ? enqueue(pending) : queue;
    },

    cancel() {
      cancelled = true;
      pending = undefined;
      clearTimeout(timer);
      timer = undefined;
    },
  };
}
