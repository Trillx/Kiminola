import { invoke as nativeInvoke, Channel, type InvokeArgs } from "@tauri-apps/api/core";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { trackOperation } from "./pending-work.ts";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

function invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
  return trackOperation(nativeInvoke<T>(command, args));
}

export type TranscriptChannel = "you" | "others";

export interface TranscriptEvent {
  utterance_id: number;
  revision: number;
  channel: TranscriptChannel;
  text: string;
  is_partial: boolean;
  start_ms: number;
  end_ms: number;
}

export interface AudioPressureEvent {
  mic_dropped_samples: number;
  loopback_dropped_samples: number;
}

export interface RecordingStartStatus {
  meeting_audio_available: boolean;
  transcription_available: boolean;
}

export interface RecordingStopResult {
  transcript: TranscriptEvent[];
  finalization_warning: string | null;
}

export interface TranscriptLine {
  id?: number;
  utterance_id?: number;
  revision?: number;
  channel: TranscriptChannel;
  text: string;
  start_ms?: number;
  end_ms?: number;
  is_partial?: boolean;
}

export async function startRecording(): Promise<RecordingStartStatus> {
  return invoke("start_recording");
}

export async function stopRecording(): Promise<RecordingStopResult> {
  return invoke("stop_recording");
}

export async function pauseRecording(): Promise<void> {
  await invoke("pause_recording");
}

export async function resumeRecording(): Promise<RecordingStartStatus> {
  return invoke("resume_recording");
}

export function onTranscriptEvent(
  handler: (event: TranscriptEvent) => void,
): Promise<UnlistenFn> {
  return listen<TranscriptEvent>("transcript:event", (payload) => {
    handler(payload.payload);
  });
}

export function onAudioPressure(
  handler: (event: AudioPressureEvent) => void,
): Promise<UnlistenFn> {
  return listen<AudioPressureEvent>("recording:audio-pressure", (payload) => {
    handler(payload.payload);
  });
}

export function onRecordingQuitBlocked(handler: () => void): Promise<UnlistenFn> {
  return listen<unknown>("recording:quit-blocked", () => handler());
}

/* ---------- persistence (SQLite via src-tauri db.rs) ---------- */

export interface MeetingSummary {
  id: number;
  title: string;
  /** RFC 3339 timestamp. */
  created_at: string;
  duration_seconds: number;
  space_name: string | null;
  location_path: string | null;
  parent_meeting_id: number | null;
}

export interface MeetingDetail {
  id: number;
  title: string;
  created_at: string;
  duration_seconds: number;
  space_name: string | null;
  location_path: string | null;
  parent_meeting_id: number | null;
  notepad: string;
  enhanced_markdown: string | null;
  transcript: TranscriptLine[];
}

export interface NoteDraftSummary {
  id: number;
  title: string;
  created_at: string;
  updated_at: string;
}

export interface NoteDraftDetail extends NoteDraftSummary {
  raw_markdown: string;
  meeting_id: number | null;
  recovery_duration_seconds: number;
  recovery_transcript: TranscriptLine[];
  recovery_location: LibraryLocation | null;
}

export type LibraryLocation =
  | { kind: "space"; id: number }
  | { kind: "meeting"; id: number };

export type LibraryNode =
  | { kind: "space"; id: number; name: string; children: LibraryNode[] }
  | {
      kind: "meeting";
      id: number;
      title: string;
      created_at: string;
      duration_seconds: number;
      children: LibraryNode[];
    };

export async function saveMeeting(input: {
  title: string;
  durationSeconds: number;
  notepad: string;
  segments: TranscriptLine[];
  noteDraftId?: number | null;
  location?: LibraryLocation | null;
}): Promise<number> {
  return invoke("save_meeting", {
    title: input.title,
    durationSeconds: input.durationSeconds,
    notepad: input.notepad,
    segments: input.segments,
    noteDraftId: input.noteDraftId ?? null,
    location: input.location ?? null,
  });
}

export async function listMeetings(): Promise<MeetingSummary[]> {
  return invoke("list_meetings");
}

export async function getMeeting(id: number): Promise<MeetingDetail> {
  return invoke("get_meeting", { id });
}

export async function renameMeeting(meetingId: number, title: string): Promise<void> {
  await invoke("rename_meeting", { meetingId, title });
}

export async function listLibraryTree(): Promise<LibraryNode[]> {
  return invoke("list_library_tree");
}

export async function createSpace(name: string, parentSpaceId?: number | null): Promise<number> {
  return invoke("create_space", { name, parentSpaceId: parentSpaceId ?? null });
}

export async function renameSpace(spaceId: number, name: string): Promise<void> {
  await invoke("rename_space", { spaceId, name });
}

export async function moveLibraryNode(
  node: LibraryLocation,
  destination: LibraryLocation | null,
): Promise<void> {
  await invoke("move_library_node", { node, destination });
}

export async function updateNotes(meetingId: number, rawMarkdown: string): Promise<void> {
  await invoke("update_notes", { meetingId, rawMarkdown });
}

export async function createNoteDraft(location: LibraryLocation | null = null): Promise<number> {
  return invoke("create_note_draft", { location });
}

export async function listNoteDrafts(): Promise<NoteDraftSummary[]> {
  return invoke("list_note_drafts");
}

export async function getNoteDraft(id: number): Promise<NoteDraftDetail> {
  return invoke("get_note_draft", { id });
}

export async function updateNoteDraft(id: number, rawMarkdown: string): Promise<void> {
  await invoke("update_note_draft", { id, rawMarkdown });
}

export async function updateNoteDraftRecovery(
  id: number,
  rawMarkdown: string,
  durationSeconds: number,
  transcript: TranscriptLine[],
  location: LibraryLocation | null = null,
): Promise<void> {
  await invoke("update_note_draft_recovery", {
    id,
    rawMarkdown,
    durationSeconds,
    transcript,
    location,
  });
}

export async function deleteNoteDraft(id: number): Promise<void> {
  await invoke("delete_note_draft", { id });
}

/* ---------- LLM provider (src-tauri llm.rs) ---------- */

export type ProviderKind = "open_ai" | "open_router" | "ollama" | "lm_studio";

export interface ProviderConfig {
  kind: ProviderKind;
  base_url: string;
  model: string;
  has_api_key?: boolean;
}

export interface Template {
  id: number;
  name: string;
  prompt: string;
  is_builtin: number;
}

export async function getLlmConfig(): Promise<ProviderConfig> {
  return invoke("get_llm_config");
}

export async function setLlmConfig(config: ProviderConfig, apiKey?: string): Promise<void> {
  await invoke("set_llm_config", { config, apiKey });
}

export type LlmStreamEvent =
  | { event: "chunk"; data: string }
  | { event: "done" }
  | { event: "error"; data: string };

export async function testLlmConfig(onEvent: (event: LlmStreamEvent) => void = () => {}): Promise<void> {
  const onEventChannel = new Channel<LlmStreamEvent>();
  onEventChannel.onmessage = onEvent;
  await invoke("test_llm_config", { onEvent: onEventChannel });
}

export async function listTemplates(): Promise<Template[]> {
  return invoke("list_templates");
}

export async function createTemplate(name: string, prompt: string): Promise<Template> {
  return invoke("create_template", { name, prompt });
}

export async function updateTemplate(id: number, name: string, prompt: string): Promise<void> {
  await invoke("update_template", { id, name, prompt });
}

export async function deleteTemplate(id: number): Promise<void> {
  await invoke("delete_template", { id });
}

export async function updateSegmentText(segmentId: number, text: string): Promise<void> {
  await invoke("update_segment_text", { segmentId, text });
}

export async function deleteSegment(segmentId: number): Promise<void> {
  await invoke("delete_segment", { segmentId });
}

export async function searchMeetings(query: string): Promise<MeetingSummary[]> {
  return invoke("search_meetings", { query });
}

export async function enhanceMeeting(
  meetingId: number,
  templateId?: number,
  onEvent: (event: LlmStreamEvent) => void = () => {},
): Promise<void> {
  const onEventChannel = new Channel<LlmStreamEvent>();
  onEventChannel.onmessage = onEvent;
  await invoke("enhance_meeting", { meetingId, templateId, onEvent: onEventChannel });
}

/* ---------- export (src-tauri export.rs) ---------- */

export async function exportNotesMarkdown(meetingId: number): Promise<string> {
  return invoke("export_notes_markdown", { meetingId });
}

export async function exportTranscriptText(meetingId: number): Promise<string> {
  return invoke("export_transcript_text", { meetingId });
}

/** Writes the .md export; resolves to the written file path. */
export async function saveNotesExport(meetingId: number): Promise<string> {
  return invoke("save_notes_export", { meetingId });
}

/** Writes the .txt transcript export; resolves to the written file path. */
export async function saveTranscriptExport(meetingId: number): Promise<string> {
  return invoke("save_transcript_export", { meetingId });
}

/* ---------- global shortcut (src-tauri shortcuts.rs) ---------- */

export async function getGlobalShortcut(): Promise<string | null> {
  return invoke("get_global_shortcut");
}

export async function setGlobalShortcut(shortcut: string | null): Promise<void> {
  await invoke("set_global_shortcut", { shortcut });
}

/* ---------- onboarding & model pack ---------- */

export interface DownloadEvent {
  file: string;
  downloaded: number;
  total: number;
  overall_downloaded: number;
  overall_total: number;
}

export type MicrophonePermission = "Granted" | "Denied" | "Unavailable";

export async function isOnboardingComplete(): Promise<boolean> {
  return invoke("is_onboarding_complete");
}

export async function setOnboardingComplete(complete: boolean): Promise<void> {
  await invoke("set_onboarding_complete", { complete });
}

export async function checkMicrophonePermission(): Promise<MicrophonePermission> {
  return invoke("check_microphone_permission");
}

export async function checkModelPack(): Promise<boolean> {
  return invoke("check_model_pack");
}

export async function downloadModelPack(
  onProgress: (event: DownloadEvent) => void,
): Promise<void> {
  const channel = new Channel<DownloadEvent>();
  channel.onmessage = onProgress;
  await invoke("download_model_pack", { onProgress: channel });
}

export async function openModelFolder(): Promise<void> {
  await invoke("open_model_folder");
}

export async function openMicrophonePrivacySettings(): Promise<void> {
  await openUrl("ms-settings:privacy-microphone");
}

export function onShortcutTriggered(handler: () => void): Promise<UnlistenFn> {
  return listen<unknown>("shortcut:triggered", () => handler());
}

/* ---------- meeting presence companion (src-tauri meeting_presence.rs) ---------- */

export type MeetingPresenceMode = "off" | "paused" | "detecting";

export interface MeetingPrompt {
  id: string;
  app_label: string;
  message: string;
  not_recording_message: string;
  confidence: "possible" | "likely";
  evidence: ("app_or_visible_window" | "active_core_audio")[];
}

export interface MeetingPresenceHint {
  app_label: string;
  confidence: "possible" | "likely";
  evidence: ("app_or_visible_window" | "active_core_audio")[];
}

export interface MeetingPresenceState {
  enabled: boolean;
  paused: boolean;
  start_with_windows: boolean;
  mode: MeetingPresenceMode;
  hint: MeetingPresenceHint | null;
  prompt: MeetingPrompt | null;
}

export interface MeetingPresenceAction {
  action: "notes" | "start";
  draft_id?: number;
}

const MEETING_PRESENCE_ACTION_EVENT = "meeting-presence:action";

export async function sendMeetingPresenceActionToMain(
  action: MeetingPresenceAction,
): Promise<void> {
  await emitTo("main", MEETING_PRESENCE_ACTION_EVENT, action);
}

export async function getMeetingPresenceState(): Promise<MeetingPresenceState> {
  return invoke("get_meeting_presence_state");
}

export async function setMeetingPresenceEnabled(enabled: boolean): Promise<void> {
  await invoke("set_meeting_presence_enabled", { enabled });
}

export async function setMeetingPresencePaused(paused: boolean): Promise<void> {
  await invoke("set_meeting_presence_paused", { paused });
}

export async function setMeetingPresenceStartWithWindows(enabled: boolean): Promise<void> {
  await invoke("set_meeting_presence_start_with_windows", { enabled });
}

export async function jotNotesFromMeetingPrompt(promptId: string): Promise<number> {
  return invoke("jot_notes_from_meeting_prompt", { promptId });
}

export async function startRecordingFromMeetingPrompt(promptId: string): Promise<void> {
  await invoke("start_recording_from_meeting_prompt", { promptId });
}

export async function dismissMeetingPrompt(promptId: string): Promise<void> {
  await invoke("dismiss_meeting_prompt", { promptId });
}

export function onMeetingPresencePrompt(
  handler: (prompt: MeetingPrompt) => void,
): Promise<UnlistenFn> {
  return listen<MeetingPrompt>("meeting-presence:prompt", (payload) => {
    handler(payload.payload);
  });
}

export function onMeetingPresenceState(
  handler: (state: MeetingPresenceState) => void,
): Promise<UnlistenFn> {
  return listen<MeetingPresenceState>("meeting-presence:state", (payload) => {
    handler(payload.payload);
  });
}

export function onMeetingPresenceAction(
  handler: (action: MeetingPresenceAction) => void,
): Promise<UnlistenFn> {
  return listen<MeetingPresenceAction>(MEETING_PRESENCE_ACTION_EVENT, (payload) => {
    handler(payload.payload);
  });
}
