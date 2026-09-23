import { invoke as nativeInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { trackOperation } from "./pending-work";
import type { DictationHistoryEntry, DictationMicrophone, DictationResolution, DictationSettingsInput, DictationSnapshot } from "./dictation-types";

function invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
  return trackOperation(nativeInvoke<T>(command, args));
}

export const getDictationState = () => invoke<DictationSnapshot>("get_dictation_state");
export const setDictationSettings = (input: DictationSettingsInput) => invoke<DictationSnapshot>("set_dictation_settings", { input });
export const startDictation = () => invoke<DictationSnapshot>("start_dictation");
export const stopDictation = () => invoke<DictationSnapshot>("stop_dictation");
export const cancelDictation = () => invoke<DictationSnapshot>("cancel_dictation");
export const resolveDictation = (action: DictationResolution) => invoke<DictationSnapshot>("resolve_dictation", { action });
export const listDictationMicrophones = () => invoke<DictationMicrophone[]>("list_dictation_microphones");
export const listDictationHistory = () => invoke<DictationHistoryEntry[]>("list_dictation_history");
export const deleteDictationHistory = (id: number | null) => invoke<void>("delete_dictation_history", { id });
export const onDictationState = (handler: (snapshot: DictationSnapshot) => void) =>
  listen<DictationSnapshot>("dictation:state", event => handler(event.payload));
