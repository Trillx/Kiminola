import { invoke as nativeInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { trackOperation } from "./pending-work";
import type { DictationHistoryEntry, DictationMicrophone, DictationResolution, DictationSettingsInput, DictationSnapshot } from "./dictation-types";

// Updates must drain mutations, not incidental reads whose errors belong to the UI.
function mutate<T>(command: string, args?: InvokeArgs): Promise<T> {
  return trackOperation(nativeInvoke<T>(command, args));
}

export const getDictationState = () => nativeInvoke<DictationSnapshot>("get_dictation_state");
export const setDictationSettings = (input: DictationSettingsInput) => mutate<DictationSnapshot>("set_dictation_settings", { input });
export const startDictation = () => mutate<DictationSnapshot>("start_dictation");
export const stopDictation = () => mutate<DictationSnapshot>("stop_dictation");
export const cancelDictation = () => mutate<DictationSnapshot>("cancel_dictation");
export const resolveDictation = (action: DictationResolution) => mutate<DictationSnapshot>("resolve_dictation", { action });
export const listDictationMicrophones = () => nativeInvoke<DictationMicrophone[]>("list_dictation_microphones");
export const listDictationHistory = () => nativeInvoke<DictationHistoryEntry[]>("list_dictation_history");
export const deleteDictationHistory = (id: number | null) => mutate<void>("delete_dictation_history", { id });
export const onDictationState = (handler: (snapshot: DictationSnapshot) => void) =>
  listen<DictationSnapshot>("dictation:state", event => handler(event.payload));
