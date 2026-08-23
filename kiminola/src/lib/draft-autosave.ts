export type DraftAutosaveStatus = "saving" | "saved" | "error";

export interface DraftAutosave {
  schedule(value: string): void;
  flush(value: string): Promise<void>;
  cancel(): void;
}

/**
 * Debounces draft writes and serializes them so an older, slower request can
 * never overwrite newer notes. `flush` joins the same queue and is used before
 * meeting finalization or navigation.
 */
export function createDraftAutosave(
  save: (value: string) => Promise<void>,
  onStatus: (status: DraftAutosaveStatus) => void,
  delayMs = 500,
): DraftAutosave {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queue = Promise.resolve();
  let latestRequest = 0;
  let cancelled = false;

  function enqueue(value: string): Promise<void> {
    const request = ++latestRequest;
    if (!cancelled) onStatus("saving");

    const run = queue.catch(() => undefined).then(() => save(value));
    queue = run.catch(() => undefined);

    return run.then(
      () => {
        if (!cancelled && request === latestRequest) onStatus("saved");
      },
      (error: unknown) => {
        if (!cancelled && request === latestRequest) onStatus("error");
        throw error;
      },
    );
  }

  return {
    schedule(value: string) {
      if (cancelled) return;
      clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        void enqueue(value).catch(() => undefined);
      }, delayMs);
    },

    flush(value: string) {
      clearTimeout(timer);
      timer = undefined;
      return enqueue(value);
    },

    cancel() {
      cancelled = true;
      clearTimeout(timer);
      timer = undefined;
    },
  };
}
