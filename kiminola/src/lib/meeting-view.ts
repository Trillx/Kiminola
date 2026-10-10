import type { LlmStreamEvent, MeetingDetail, ProviderConfig, Template } from "./tauri";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { providerIsConfigured } from "./settings-ui.ts";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { meetingNotepad, type NotepadStore } from "./notepad-store.ts";

export type MeetingTab = "mynotes" | "enhance" | "transcript";
export type MeetingExportAction = "copy-notes" | "copy-transcript" | "save-notes" | "save-transcript";

export interface MeetingViewPort {
  getMeeting(id: number): Promise<MeetingDetail>;
  getLlmConfig(): Promise<ProviderConfig>;
  listTemplates(): Promise<Template[]>;
  enhanceMeeting(
    meetingId: number,
    templateId: number | undefined,
    onEvent: (event: LlmStreamEvent) => void,
  ): Promise<void>;
  exportNotesMarkdown(meetingId: number): Promise<string>;
  exportTranscriptText(meetingId: number): Promise<string>;
  saveNotesExport(meetingId: number): Promise<string>;
  saveTranscriptExport(meetingId: number): Promise<string>;
  writeClipboard(text: string): Promise<void>;
  renderMarkdown(markdown: string): string;
  /** One-shot timer; returns its cancel function. */
  delay(milliseconds: number, run: () => void): () => void;
}

export interface MeetingViewDependencies {
  port: MeetingViewPort;
  notepad: NotepadStore;
  log?: (message: string, error: unknown) => void;
}

/** Per-route view state. The page may set the UI-only fields: tab, notes (bound), selectedTemplateId, showFinalizationWarning. */
export interface MeetingViewState {
  meeting: MeetingDetail | null;
  notFound: boolean;
  tab: MeetingTab;
  notes: string;
  showFinalizationWarning: boolean;
  templates: Template[];
  selectedTemplateId: number | undefined;
  config: ProviderConfig | null;
  configLoaded: boolean;
  enhancing: boolean;
  enhanceError: string | null;
  enhancedMd: string;
  /** Rendered on a throttle while streaming: re-parsing every chunk stalls the UI. */
  enhancedHtml: string;
  enhancedGenerated: boolean;
  hasEverEnhanced: boolean;
  /** Some Notepad document (this meeting's or another) failed to save. */
  notesSaveError: boolean;
}

function routeState(): Omit<MeetingViewState, "notesSaveError"> {
  return {
    meeting: null,
    notFound: false,
    tab: "mynotes",
    notes: "",
    showFinalizationWarning: false,
    templates: [],
    selectedTemplateId: undefined,
    config: null,
    configLoaded: false,
    enhancing: false,
    enhanceError: null,
    enhancedMd: "",
    enhancedHtml: "",
    enhancedGenerated: false,
    hasEverEnhanced: false,
  };
}

export function initialMeetingViewState(): MeetingViewState {
  return { ...routeState(), notesSaveError: false };
}

const RENDER_THROTTLE_MS = 100;

/**
 * The Meeting view behind meeting/[id]: loads a Meeting per route (after
 * pending notes are saved), streams Note enhancement, and exports. Every
 * response is checked against the route or stream that asked for it, so a
 * late reply for a previous Meeting or an abandoned run never lands.
 */
export function createMeetingView(
  { port, notepad, log = (message, error) => console.error(message, error) }: MeetingViewDependencies,
  state: MeetingViewState = initialMeetingViewState(),
) {
  let routeToken = 0;
  let streamToken = 0;
  let cancelRender: (() => void) | null = null;

  const unsubscribeNotes = notepad.subscribe("all", (status) => {
    state.notesSaveError = status === "error";
  });

  function configured(): boolean {
    return state.configLoaded && state.config !== null && providerIsConfigured(state.config);
  }

  function renderNow() {
    cancelRender?.();
    cancelRender = null;
    state.enhancedHtml = port.renderMarkdown(state.enhancedMd);
  }

  function scheduleRender() {
    if (cancelRender) return;
    cancelRender = port.delay(RENDER_THROTTLE_MS, () => {
      cancelRender = null;
      state.enhancedHtml = port.renderMarkdown(state.enhancedMd);
    });
  }

  async function loadConfig(isCurrent: () => boolean) {
    try {
      const [config, templates] = await Promise.all([port.getLlmConfig(), port.listTemplates()]);
      if (!isCurrent()) return;
      state.config = config;
      state.templates = templates;
      state.selectedTemplateId = templates[0]?.id;
    } catch (error) {
      if (!isCurrent()) return;
      log("Failed to load LLM config/templates:", error);
      state.config = null;
    } finally {
      if (isCurrent()) state.configLoaded = true;
    }
  }

  async function enhance() {
    const meeting = state.meeting;
    if (!meeting || state.enhancing) return;
    const run = ++streamToken;
    const isCurrent = () => run === streamToken;
    const templateId = state.selectedTemplateId;
    state.enhancing = true;
    state.enhanceError = null;
    try {
      // The provider prompt reads notes from the database: the latest edit must land first.
      await notepad.flush(meetingNotepad(meeting.id));
      if (!isCurrent()) return;
      state.enhancedMd = "";
      state.enhancedHtml = "";
      state.enhancedGenerated = false;
      await port.enhanceMeeting(meeting.id, templateId, (event) => {
        if (!isCurrent()) return;
        if (event.event === "chunk") {
          state.enhancedMd += event.data;
          scheduleRender();
          return;
        }
        state.enhancing = false;
        if (event.event === "done") {
          state.enhancedGenerated = true;
          state.hasEverEnhanced = true;
        } else {
          state.enhanceError = event.data;
        }
        renderNow();
      });
    } catch (error) {
      if (!isCurrent()) return;
      state.enhanceError = String(error);
      state.enhancing = false;
    }
  }

  return {
    state,

    /** The configured Provider can run Note enhancement (one rule, shared with settings). */
    get configured() {
      return configured();
    },

    /**
     * Show the Meeting for this route. `?mode=generate|enhance` enhances once
     * the provider is known to be configured; `?warning=transcript-finalization`
     * shows the finalization warning. Returns the route's cleanup.
     */
    open(id: number, searchParams: URLSearchParams): () => void {
      const route = ++routeToken;
      streamToken++;
      const isCurrent = () => route === routeToken;
      const mode = searchParams.get("mode");
      cancelRender?.();
      cancelRender = null;
      Object.assign(state, routeState(), {
        showFinalizationWarning: searchParams.get("warning") === "transcript-finalization",
      });
      const close = () => {
        if (!isCurrent()) return;
        routeToken++;
        streamToken++;
      };
      if (!Number.isInteger(id)) {
        state.notFound = true;
        return close;
      }

      const configReady = loadConfig(isCurrent);
      // A failed write must not turn an existing Meeting into a missing one.
      notepad
        .read(() => port.getMeeting(id), (error) => log("Failed to save notes:", error))
        .then(async (meeting) => {
          if (!isCurrent()) return;
          state.meeting = meeting;
          state.notes = notepad.pending(meetingNotepad(id))?.text ?? meeting.notepad;
          if (meeting.enhanced_markdown) {
            state.enhancedMd = meeting.enhanced_markdown;
            renderNow();
            state.enhancedGenerated = true;
            state.hasEverEnhanced = true;
          }
          if (mode === "generate" || mode === "enhance") {
            state.tab = "enhance";
            await configReady;
            if (isCurrent() && configured()) await enhance();
          }
        })
        .catch(() => {
          if (isCurrent()) state.notFound = true;
        });
      return close;
    },

    enhance,

    /** The notes textarea changed; save it (debounced) to this Meeting. */
    notesChanged() {
      if (state.meeting) notepad.edit(meetingNotepad(state.meeting.id), state.notes);
    },

    /** Retry every Notepad document that failed to save. */
    retrySave(): Promise<void> {
      return notepad.flush().catch((error) => log("Failed to save notes:", error));
    },

    async refreshConfig() {
      try {
        state.config = await port.getLlmConfig();
      } catch (error) {
        log("Failed to refresh LLM config:", error);
      }
    },

    /** Notes exports save pending edits first and fail rather than export stale notes. */
    async export(action: MeetingExportAction): Promise<{ message: string; path?: string }> {
      const meeting = state.meeting;
      if (!meeting) throw new Error("No meeting is open.");
      const id = meeting.id;
      // Transcript exports only read saved transcript data and need no notes write.
      if (action === "copy-notes" || action === "save-notes") {
        await notepad.flush(meetingNotepad(id));
      }
      switch (action) {
        case "copy-notes":
          await port.writeClipboard(await port.exportNotesMarkdown(id));
          return { message: "Notes copied to clipboard." };
        case "copy-transcript":
          await port.writeClipboard(await port.exportTranscriptText(id));
          return { message: "Transcript copied to clipboard." };
        case "save-notes":
          return { message: "Notes saved.", path: await port.saveNotesExport(id) };
        case "save-transcript":
          return { message: "Transcript saved.", path: await port.saveTranscriptExport(id) };
      }
    },

    /** The page is gone: drop late responses and save edits made on it. */
    dispose() {
      routeToken++;
      streamToken++;
      cancelRender?.();
      cancelRender = null;
      unsubscribeNotes();
      void notepad.flush().catch((error) => log("Failed to save notes:", error));
    },
  };
}

export type MeetingView = ReturnType<typeof createMeetingView>;
