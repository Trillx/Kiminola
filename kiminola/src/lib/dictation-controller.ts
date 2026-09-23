import type { DictationSnapshot } from "./dictation-types";

export type DictationView = { snapshot: DictationSnapshot | null; error: string };
type Transport = {
  get: () => Promise<DictationSnapshot>;
  listen: (handler: (snapshot: DictationSnapshot) => void) => Promise<() => void>;
};

/** Per-mounted-view subscription; no capture or retained-text persistence. */
export function createDictationController(transport: Transport, changed: (view: DictationView) => void) {
  let disposed = false;
  let unlisten: (() => void) | undefined;
  let view: DictationView = { snapshot: null, error: "" };
  let revision = 0;
  // Events and command replies have no ordered revision in the IPC contract.
  // Treat events as invalidations and read current state instead of trusting
  // their payload. A later read invalidates every older in-flight completion.
  async function refresh() {
    const request = ++revision;
    try {
      const snapshot = await transport.get();
      if (!disposed && request === revision) { view = { snapshot, error: "" }; changed(view); }
    } catch (error) {
      if (!disposed && request === revision) { view = { ...view, error: String(error) }; changed(view); }
    }
  }
  return {
    async connect() {
      try {
        const stop = await transport.listen(() => { void refresh(); });
        if (disposed) { stop(); return; }
        unlisten = stop;
        await refresh();
      } catch (error) {
        if (!disposed) { view = { ...view, error: String(error) }; changed(view); }
      }
    },
    refresh,
    dispose() { disposed = true; unlisten?.(); },
  };
}
