export type DraftAutosaveStatus = "saving" | "saved" | "error";
type DraftValue<T> = T | (() => T);

export interface DraftAutosave<T = string> {
  schedule(value: DraftValue<T>): void;
  flush(value: DraftValue<T>): Promise<void>;
  flushPending(): Promise<void>;
  cancel(): void;
}

/**
 * Debounces writes with a non-resetting maximum wait and serializes them so an
 * older request cannot overwrite newer notes. A snapshot factory is evaluated
 * only when its write runs, not for every edit. Failed snapshots stay pending
 * for retry. `flush` joins the same queue before finalization or navigation.
 */
export function createDraftAutosave<T = string>(
  save: (value: T) => Promise<void>,
  onStatus: (status: DraftAutosaveStatus) => void,
  delayMs = 500,
  maxWaitMs = 5_000,
): DraftAutosave<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  let queue = Promise.resolve();
  let latestRequest = 0;
  let cancelled = false;
  let pending: { value: DraftValue<T> } | undefined;

  function clearTimers() {
    clearTimeout(timer);
    clearTimeout(deadline);
    timer = undefined;
    deadline = undefined;
  }

  function dispatchPending() {
    clearTimers();
    if (pending) void enqueue(pending).catch(() => undefined);
  }

  function enqueue(snapshot: { value: DraftValue<T> }): Promise<void> {
    const request = ++latestRequest;
    if (!cancelled) onStatus("saving");

    const run = queue.catch(() => undefined).then(() => save(
      typeof snapshot.value === "function" ? (snapshot.value as () => T)() : snapshot.value,
    ));
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
    schedule(value: DraftValue<T>) {
      if (cancelled) return;
      pending = { value };
      clearTimeout(timer);
      timer = setTimeout(dispatchPending, delayMs);
      deadline ??= setTimeout(dispatchPending, maxWaitMs);
    },

    flush(value: DraftValue<T>) {
      clearTimers();
      pending = { value };
      return enqueue(pending);
    },

    flushPending() {
      clearTimers();
      return pending ? enqueue(pending) : queue;
    },

    cancel() {
      cancelled = true;
      pending = undefined;
      clearTimers();
    },
  };
}
