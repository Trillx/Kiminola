/** Shared by editors and the update boundary, including saves from unmounted pages. */
const savers = new Set<() => Promise<void>>();
const operations = new Set<Promise<unknown>>();
const guards = new Set<() => void>();

export function registerUpdateGuard(guard: () => void): () => void {
  guards.add(guard);
  return () => { guards.delete(guard); };
}

export function trackOperation<T>(operation: Promise<T>): Promise<T> {
  operations.add(operation);
  void operation.then(() => operations.delete(operation), () => operations.delete(operation));
  return operation;
}

export function registerPendingSave(flush: () => Promise<void>): () => void {
  let disposed = false;
  const registered = async () => {
    await flush();
    if (disposed) savers.delete(registered);
  };
  savers.add(registered);
  return () => {
    disposed = true;
    // Keep a failed save registered so an update cannot silently discard it.
    void registered().catch(() => undefined);
  };
}

export async function flushPendingWork(): Promise<void> {
  for (const guard of guards) guard();
  const results = await Promise.allSettled([...savers].map((flush) => flush()));
  const failure = results.find((result) => result.status === "rejected");
  if (failure?.status === "rejected") throw failure.reason;
  while (operations.size) await Promise.all([...operations]);
}
