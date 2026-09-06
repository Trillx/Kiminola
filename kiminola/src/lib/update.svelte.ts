import { browser } from "$app/environment";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { updateProgress } from "$lib/update-policy";
import { flushPendingWork } from "$lib/pending-work";
import { installWhenSaved } from "$lib/update-safety";

export type UpdateStatus =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "preparing"
  | "installing"
  | "up_to_date"
  | "error";

export interface UpdateState {
  status: UpdateStatus;
  version: string | null;
  notes: string | null;
  date: string | null;
  progress: number;
  downloadedBytes: number;
  contentLength: number | null;
  checkedAt: number | null;
  error: string | null;
}

export const updateState = $state<UpdateState>({
  status: "idle",
  version: null,
  notes: null,
  date: null,
  progress: 0,
  downloadedBytes: 0,
  contentLength: null,
  checkedAt: null,
  error: null,
});

let candidate: Update | null = null;
let automaticCheckStarted = false;
let currentOperation: Promise<unknown> | null = null;
let installation: Promise<boolean> | null = null;

const AUTOMATIC_CHECK_DELAY_MS = 2_000;

function updaterRuntimeAvailable(): boolean {
  return browser && isTauri();
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function clearCandidateState() {
  candidate = null;
  updateState.version = null;
  updateState.notes = null;
  updateState.date = null;
  updateState.progress = 0;
  updateState.downloadedBytes = 0;
  updateState.contentLength = null;
}

function applyCandidate(update: Update) {
  candidate = update;
  updateState.version = update.version;
  updateState.notes = update.body ?? null;
  updateState.date = update.date ?? null;
  updateState.progress = 0;
  updateState.downloadedBytes = 0;
  updateState.contentLength = null;
}

function applyDownloadEvent(event: DownloadEvent) {
  if (event.event === "Started") {
    updateState.contentLength = event.data.contentLength ?? null;
    updateState.downloadedBytes = 0;
    updateState.progress = 0;
  } else if (event.event === "Progress") {
    updateState.downloadedBytes += event.data.chunkLength;
    updateState.progress = updateProgress(
      updateState.downloadedBytes,
      updateState.contentLength,
    );
  } else if (event.event === "Finished") {
    updateState.progress = 100;
  }
}

/** Start the single non-blocking stable update check for this app launch. */
export function startAutomaticUpdateCheck() {
  if (!updaterRuntimeAvailable() || automaticCheckStarted) return;
  automaticCheckStarted = true;
  window.setTimeout(() => {
    void checkForUpdates();
  }, AUTOMATIC_CHECK_DELAY_MS);
}

export async function checkForUpdates(): Promise<void> {
  if (!updaterRuntimeAvailable()) return;
  if (installation) return;
  if (currentOperation) {
    await currentOperation;
    return;
  }

  updateState.status = "checking";
  updateState.error = null;
  clearCandidateState();

  const operation = (async () => {
    try {
      const available = await check();
      if (available) {
        applyCandidate(available);
        updateState.status = "available";
      } else {
        updateState.status = "up_to_date";
      }
    } catch (error) {
      updateState.status = "error";
      updateState.error = errorMessage(error);
    } finally {
      updateState.checkedAt = Date.now();
    }
  })();

  currentOperation = operation;
  try {
    await operation;
  } finally {
    if (currentOperation === operation) currentOperation = null;
  }
}

export async function downloadUpdate(): Promise<boolean> {
  if (!updaterRuntimeAvailable()) return false;
  if (updateState.status === "ready" && candidate) return true;
  if (currentOperation) await currentOperation;
  if (!candidate) return false;

  updateState.status = "downloading";
  updateState.error = null;

  const operation = (async () => {
    try {
      await candidate?.download(applyDownloadEvent);
      updateState.status = "ready";
      updateState.progress = 100;
      return true;
    } catch (error) {
      updateState.status = "error";
      updateState.error = errorMessage(error);
      return false;
    }
  })();

  currentOperation = operation;
  try {
    return await operation;
  } finally {
    if (currentOperation === operation) currentOperation = null;
  }
}

/** Download explicitly, then install only if the caller still allows shutdown. */
export function installUpdate(canInstall: () => boolean): Promise<boolean> {
  if (installation) return installation;
  installation = runInstallation(canInstall).finally(() => { installation = null; });
  return installation;
}

async function runInstallation(canInstall: () => boolean): Promise<boolean> {
  if (!updaterRuntimeAvailable() || !candidate) return false;
  if (!canInstall()) {
    updateState.error = "Finish and save the current meeting before installing the update.";
    return false;
  }

  if (updateState.status !== "ready" && !(await downloadUpdate())) return false;
  if (!canInstall()) {
    updateState.status = "ready";
    updateState.error = "Finish and save the current meeting before installing the update.";
    return false;
  }

  const downloaded = candidate;
  updateState.status = "preparing";
  updateState.error = null;
  try {
    await installWhenSaved({
      flush: flushPendingWork,
      prepare: async () => {
        if (!canInstall()) throw new Error("Finish and save the current meeting before updating.");
        await invoke("prepare_app_update");
      },
      install: async () => {
        updateState.status = "installing";
        await downloaded.install();
      },
      cancel: () => invoke("cancel_app_update"),
    });
    return true;
  } catch (error) {
    updateState.status = "ready";
    updateState.error = errorMessage(error);
    return false;
  }
}
