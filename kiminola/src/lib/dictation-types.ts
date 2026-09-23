export type DictationSettings = {
  enabled: boolean;
  activation: "hold" | "toggle";
  shortcut: string;
  side: "left" | "right";
  cleanup: "raw" | "provider";
  clipboard_consent: boolean;
  history_enabled: boolean;
  microphone_id: string | null;
};
export type DictationSnapshot = {
  settings: DictationSettings;
  provider_authorized: boolean;
  phase: "disabled" | "idle" | "starting" | "listening" | "processing" | "review";
  session_id: number | null;
  text: string;
  raw_text: string;
  level: number;
  elapsed_seconds: number;
  error: string | null;
  exit_intent: "quit" | "disable" | null;
  delivery: "none" | "verified" | "uncertain";
};
export type DictationSettingsInput = DictationSettings & { consent_provider: boolean };
export type DictationResolution = "copy" | "dismiss" | "confirm" | "cancel_exit";
export type DictationMicrophone = { id: string; name: string };
export type DictationHistoryEntry = { id: number; text: string; created_at: string };

export function sameDictationSettings(a: DictationSettings | null, b: DictationSettings | null): boolean {
  if (!a || !b) return a === b;
  return a.enabled === b.enabled && a.activation === b.activation && a.shortcut === b.shortcut &&
    a.side === b.side && a.cleanup === b.cleanup && a.clipboard_consent === b.clipboard_consent &&
    a.history_enabled === b.history_enabled && a.microphone_id === b.microphone_id;
}
