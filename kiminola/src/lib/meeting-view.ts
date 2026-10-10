import type { LlmStreamEvent, MeetingDetail, ProviderConfig, Template, TranscriptLine } from "./tauri";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { extractActionItemEntries, replaceActionItems } from "./action-items.ts";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { providerIsConfigured } from "./settings-ui.ts";
// @ts-expect-error Node strip-types tests import the TypeScript source directly.
import { meetingNotepad, type NotepadStore } from "./notepad-store.ts";

export type MeetingTab = "mynotes" | "enhance" | "transcript";
export type MeetingExportAction = "copy-notes" | "copy-transcript" | "save-notes" | "save-transcript";
export type SegmentOperation = "save" | "delete";

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
  updateSegmentText(segmentId: number, text: string): Promise<void>;
  deleteSegment(segmentId: number): Promise<void>;
  updateEnhancedActionItems(input: {
    meetingId: number;
    originalMarkdown: string;
    enhancedMarkdown: string;
    edits: Array<{ sourceIndex: number; originalTitle: string; title: string }>;
  }): Promise<void>;
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

/**
 * Per-route view state. The page may set the UI-only fields: tab, notes,
 * segmentText, and actionItemDrafts (bound), selectedTemplateId, showFinalizationWarning.
 */
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
  /** The transcript segment being corrected, if any. */
  editingSegmentId: number | undefined;
  segmentText: string;
  /** A segment write in flight: the editor is locked until it settles. */
  segmentOperation: SegmentOperation | null;
  /** A failed segment write; the correction stays for a retry. */
  segmentError: { action: SegmentOperation; message: string } | null;
  editingActionItems: boolean;
  actionItemDrafts: string[];
  savingActionItems: boolean;
  actionItemError: string | null;
  actionItemStatus: string | null;
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
    editingSegmentId: undefined,
    segmentText: "",
    segmentOperation: null,
    segmentError: null,
    editingActionItems: false,
    actionItemDrafts: [],
    savingActionItems: false,
    actionItemError: null,
    actionItemStatus: null,
  };
}

export function initialMeetingViewState(): MeetingViewState {
  return { ...routeState(), notesSaveError: false };
}

const RENDER_THROTTLE_MS = 100;

/**
 * The Meeting view behind meeting/[id]: loads a Meeting per route (after
 * pending notes are saved), streams Note enhancement, exports, and edits the
 * transcript and enhanced action items. Every response is checked against the
 * route, stream, or editor that asked for it, so a late reply for a previous
 * Meeting, an abandoned run, or a cancelled edit never lands.
 */
export function createMeetingView(
  { port, notepad, log = (message, error) => console.error(message, error) }: MeetingViewDependencies,
  state: MeetingViewState = initialMeetingViewState(),
) {
  let routeToken = 0;
  let streamToken = 0;
  // A route load, a new or cancelled editor, or disposal retires segment callbacks.
  let segmentToken = 0;
  let actionItemToken = 0;
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
    if (!meeting || state.enhancing || state.editingActionItems || state.savingActionItems) return;
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

  /* ---------- transcript corrections ---------- */

  function resetSegmentEdit() {
    segmentToken++;
    state.editingSegmentId = undefined;
    state.segmentText = "";
    state.segmentError = null;
    state.segmentOperation = null;
  }

  function startSegmentEdit(line: TranscriptLine) {
    if (line.id === undefined || state.segmentOperation) return;
    segmentToken++;
    state.editingSegmentId = line.id;
    state.segmentText = line.text;
    state.segmentError = null;
  }

  function cancelSegmentEdit() {
    // An IPC write cannot be undone: Cancel must not imply it aborts one in flight.
    if (state.segmentOperation) return;
    resetSegmentEdit();
  }

  /** One segment write at a time; only the editor that started it sees its outcome. */
  async function writeSegment(
    action: SegmentOperation,
    write: () => Promise<void>,
    apply: (meeting: MeetingDetail) => void,
    message: string,
  ) {
    const meeting = state.meeting;
    if (!meeting) return;
    const token = segmentToken;
    const isCurrent = () => token === segmentToken;
    state.segmentOperation = action;
    try {
      await write();
      if (!isCurrent()) return;
      apply(meeting);
      resetSegmentEdit();
    } catch (error) {
      if (!isCurrent()) return;
      log(`Failed to ${action} segment:`, error);
      state.segmentError = { action, message };
    } finally {
      if (isCurrent()) state.segmentOperation = null;
    }
  }

  async function saveSegmentEdit() {
    const segmentId = state.editingSegmentId;
    if (segmentId === undefined || !state.meeting || state.segmentOperation) return;
    const text = state.segmentText.trim();
    if (!text) {
      cancelSegmentEdit();
      return;
    }
    await writeSegment(
      "save",
      () => port.updateSegmentText(segmentId, text),
      (meeting) => {
        const line = meeting.transcript.find((item) => item.id === segmentId);
        if (line) line.text = text;
      },
      "Could not save this correction. Your edit is still here; retry saving or cancel.",
    );
  }

  async function deleteSegment(segmentId: number) {
    if (!state.meeting || state.segmentOperation || state.editingSegmentId !== segmentId) return;
    await writeSegment(
      "delete",
      () => port.deleteSegment(segmentId),
      (meeting) => {
        meeting.transcript = meeting.transcript.filter((item) => item.id !== segmentId);
      },
      "Could not delete this segment. Your edit is still here; retry deleting or cancel.",
    );
  }

  /* ---------- enhanced action items ---------- */

  function startActionItemEdit() {
    state.actionItemDrafts = extractActionItemEntries(state.enhancedMd).map((item: { title: string }) => item.title);
    state.actionItemError = null;
    state.actionItemStatus = null;
    state.editingActionItems = true;
  }

  function cancelActionItemEdit() {
    if (state.savingActionItems) return;
    state.editingActionItems = false;
    state.actionItemDrafts = [];
    state.actionItemError = null;
  }

  /** Save renamed action items; linked Board cards follow them. */
  async function saveActionItemEdits() {
    const meeting = state.meeting;
    if (!meeting || state.savingActionItems) return;
    const titles = state.actionItemDrafts.map((item) => item.trim());
    if (titles.some((item) => !item)) {
      state.actionItemError = "Action items cannot be empty.";
      return;
    }
    const entries: Array<{ sourceIndex: number; title: string }> = extractActionItemEntries(state.enhancedMd);
    const edits = entries.flatMap(({ sourceIndex, title: originalTitle }, index) => {
      const title = titles[index];
      if (!title || title === originalTitle) return [];
      return [{ sourceIndex, originalTitle, title }];
    });
    if (edits.length === 0) {
      cancelActionItemEdit();
      return;
    }

    const originalMarkdown = state.enhancedMd;
    const enhancedMarkdown: string = replaceActionItems(originalMarkdown, edits);
    const token = ++actionItemToken;
    const isCurrent = () => token === actionItemToken;
    state.savingActionItems = true;
    state.actionItemError = null;
    try {
      await port.updateEnhancedActionItems({ meetingId: meeting.id, originalMarkdown, enhancedMarkdown, edits });
      if (!isCurrent()) return;
      state.enhancedMd = enhancedMarkdown;
      renderNow();
      state.editingActionItems = false;
      state.actionItemDrafts = [];
      state.actionItemStatus = "Action items updated. Linked board cards were updated too.";
    } catch (error) {
      if (!isCurrent()) return;
      state.actionItemError = `Could not update action items: ${String(error)}`;
    } finally {
      if (isCurrent()) state.savingActionItems = false;
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
      segmentToken++;
      actionItemToken++;
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
        segmentToken++;
        actionItemToken++;
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

    /** The enhanced notes' action items, in document order. */
    get actionItems(): Array<{ sourceIndex: number; title: string }> {
      return extractActionItemEntries(state.enhancedMd);
    },

    startSegmentEdit,
    cancelSegmentEdit,
    /** Save the correction; a failure keeps it and offers a retry. */
    saveSegmentEdit,
    /** Delete the segment being edited; a failure keeps the editor and offers a retry. */
    deleteSegment,
    startActionItemEdit,
    cancelActionItemEdit,
    saveActionItemEdits,

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
      segmentToken++;
      actionItemToken++;
      cancelRender?.();
      cancelRender = null;
      unsubscribeNotes();
      void notepad.flush().catch((error) => log("Failed to save notes:", error));
    },
  };
}

export type MeetingView = ReturnType<typeof createMeetingView>;
