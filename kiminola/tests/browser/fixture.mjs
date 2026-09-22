// Synthetic browser-only Tauri IPC. No native APIs, user files, credentials or audio.
(() => {
  let serial = 0;
  let nextDraftId = 101;
  const callbacks = new Map();
  const listeners = new Map();
  const summary = (id, title) => ({ id, title, created_at: '2026-09-21T15:00:00Z', duration_seconds: 600, space_name: 'Personal', location_path: 'Personal', parent_meeting_id: null });
  const meetings = [summary(1, 'Alpha planning'), summary(2, 'Beta launch')];
  const transcript = [{ id: 11, channel: 'you', text: 'Original transcript sentence.', start_ms: 0, end_ms: 1000 }];
  const presence = { enabled: false, paused: false, start_with_windows: false, mode: 'off', hint: null, prompt: null };
  const windowLabel = new URLSearchParams(location.search).get('window') === 'meeting-prompt' ? 'meeting-prompt' : 'main';
  const drafts = new Map();
  const defaultBoards = [{
    id: 1,
    name: "To-Do's",
    created_at: '2026-09-21T15:00:00Z',
    columns: [
      { id: 1, name: 'Backlog', position: 0, cards: [] },
      { id: 2, name: 'To Do', position: 1, cards: [] },
      { id: 3, name: 'Working', position: 2, cards: [] },
      { id: 4, name: 'Update', position: 3, cards: [] },
      { id: 5, name: 'Done', position: 4, cards: [] },
    ],
  }];
  const storedBoards = localStorage.getItem('kiminola-test-boards');
  const boards = storedBoards ? JSON.parse(storedBoards) : defaultBoards;
  const persistBoards = () => localStorage.setItem('kiminola-test-boards', JSON.stringify(boards));
  let nextBoardId = Math.max(0, ...boards.map(board => board.id)) + 1;
  let nextColumnId = Math.max(0, ...boards.flatMap(board => board.columns.map(column => column.id))) + 1;
  let nextCardId = Math.max(0, ...boards.flatMap(board => board.columns.flatMap(column => column.cards.map(card => card.id)))) + 1;
  let boardListFirstRead = localStorage.getItem('kiminola-test-boards-read') !== '1';
  const boardSnapshot = () => {
    const createdDefault = boardListFirstRead;
    boardListFirstRead = false;
    localStorage.setItem('kiminola-test-boards-read', '1');
    return { boards: structuredClone(boards), created_default: createdDefault };
  };
  const boardById = id => boards.find(board => board.id === id);
  const columnById = id => boards.flatMap(board => board.columns).find(column => column.id === id);
  function createDraft() {
    const id = nextDraftId++;
    drafts.set(id, { id, title: 'Synthetic meeting notes', created_at: '2026-09-21T15:00:00Z', updated_at: '2026-09-21T15:00:00Z', raw_markdown: '', meeting_id: null, recovery_duration_seconds: 0, recovery_location: null, recovery_transcript: [] });
    return id;
  }
  function claimPrompt(promptId, cmd) {
    // Match the backend's single-use claim: stale IDs must never consume a replacement.
    if (!presence.prompt) throw new Error('meeting prompt is no longer active');
    if (presence.prompt.id !== promptId) throw new Error('meeting prompt is stale');
    if (window.audit.failPresenceAction === cmd) throw new Error('Fixture: meeting prompt action failed');
    presence.prompt = null;
    presence.hint = null;
    if (window.audit.consumeAndFailPresenceAction === cmd) {
      const message = 'The detected app is no longer available. Please start a new recording manually.';
      window.audit.emit('meeting-presence:error', { prompt_id: promptId, message });
      window.audit.emit('meeting-presence:state', structuredClone(presence));
      throw new Error(message);
    }
    window.audit.emit('meeting-presence:state', structuredClone(presence));
  }
  let config = { kind: 'open_ai', base_url: 'https://api.openai.com/v1', model: 'gpt-4o-mini', has_api_key: true };
  // Presence flags only: fixture credentials never contain secret values.
  const savedIdentities = new Set([
    JSON.stringify(['open_ai', 'https://api.openai.com/v1']),
    JSON.stringify(['open_router', 'https://openrouter.ai/api/v1']),
  ]);
  window.audit = {
    calls: [], failConfig: false, failShortcut: false, failSegment: false, failSearch: false,
    slowSearch: false, modelDelay: 0, treeCount: 2, failRecovery: false, recovery: null,
    failPresenceAction: null, consumeAndFailPresenceAction: null, failPresenceState: false, failBoardMove: false,
    windows: { main: { visible: windowLabel === 'main', focused: false }, 'meeting-prompt': { visible: windowLabel === 'meeting-prompt', focused: false } },
    hasListener(event) { return [...listeners.values()].some(item => item.event === event); },
    seedNoteDraft(draft) { drafts.set(draft.id, structuredClone(draft)); },
    emit(event, payload) {
      if (event === 'meeting-presence:prompt') {
        presence.enabled = true;
        presence.mode = 'detecting';
        presence.prompt = structuredClone(payload);
        if (windowLabel === 'meeting-prompt') window.audit.windows['meeting-prompt'].visible = true;
      } else if (event === 'meeting-presence:state') {
        Object.assign(presence, structuredClone(payload));
      }
      for (const [id, item] of listeners) if (item.event === event) callbacks.get(item.handler)?.({ event, id, payload });
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener(event, id) { listeners.delete(id); } };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: windowLabel }, currentWebview: { label: windowLabel } },
    transformCallback(fn) { const id = ++serial; callbacks.set(id, fn); return id; },
    unregisterCallback(id) { callbacks.delete(id); },
    async invoke(cmd, args = {}) {
      // Match native IPC's JSON boundary: Svelte state proxies cannot be
      // structured-cloned directly, but are valid serializable arguments.
      args = JSON.parse(JSON.stringify(args));
      window.audit.calls.push({ cmd, args: structuredClone(args) });
      switch (cmd) {
        case 'plugin:event|listen': { const id = ++serial; listeners.set(id, args); return id; }
        case 'plugin:event|unlisten': listeners.delete(args.eventId); return;
        case 'plugin:window|get_all_windows': return ['main', 'meeting-prompt'];
        case 'plugin:window|hide':
        case 'plugin:window|show':
        case 'plugin:window|set_focus': {
          const target = window.audit.windows[args.label];
          if (!target) throw new Error(`Fixture: unknown window ${args.label}`);
          if (cmd === 'plugin:window|set_focus') target.focused = true;
          else target.visible = cmd === 'plugin:window|show';
          return;
        }
        case 'plugin:event|emit_to':
          if (!window.audit.sendTo) throw new Error('Fixture: no cross-window event transport installed');
          return window.audit.sendTo(args);
        case 'is_onboarding_complete': return true;
        case 'set_onboarding_complete': return;
        case 'check_microphone_permission': return 'Granted';
        case 'plugin:app|version': return '0.1.4';
        case 'get_meeting_presence_state': {
          if (window.audit.failPresenceState) throw new Error('Fixture: meeting presence state unavailable');
          const snapshot = structuredClone(presence);
          if (new URLSearchParams(location.search).has('holdPresenceSnapshot') && !window.audit.initialPresenceSnapshot) {
            const gate = window.audit.initialPresenceSnapshot = {};
            await new Promise(resolve => gate.release = resolve);
          }
          return snapshot;
        }
        case 'jot_notes_from_meeting_prompt': claimPrompt(args.promptId, cmd); return createDraft();
        case 'start_recording_from_meeting_prompt':
        case 'dismiss_meeting_prompt': claimPrompt(args.promptId, cmd); return;
        case 'list_boards': return boardSnapshot();
        case 'create_board': {
          const id = nextBoardId++;
          boards.push({ id, name: args.name, created_at: '2026-09-21T15:00:00Z', columns: [{ id: nextColumnId++, name: 'To Do', position: 0, cards: [] }] });
          persistBoards();
          return id;
        }
        case 'rename_board': {
          const board = boardById(args.boardId);
          if (!board) throw new Error('Fixture: board not found');
          board.name = args.name;
          persistBoards();
          return;
        }
        case 'create_board_column': {
          const board = boardById(args.boardId);
          if (!board) throw new Error('Fixture: board not found');
          board.columns.push({ id: nextColumnId++, name: args.name, position: board.columns.length, cards: [] });
          persistBoards();
          return board.columns.at(-1).id;
        }
        case 'rename_board_column': {
          const column = columnById(args.columnId);
          if (!column) throw new Error('Fixture: column not found');
          column.name = args.name;
          persistBoards();
          return;
        }
        case 'add_board_card': {
          const board = boardById(args.boardId);
          const column = board?.columns.find(item => item.id === args.columnId);
          if (!column) throw new Error('Fixture: column does not belong to board');
          const meeting = meetings.find(item => item.id === args.meetingId);
          const card = { id: nextCardId++, title: args.title, position: column.cards.length, meeting_id: args.meetingId ?? null, meeting_title: meeting?.title ?? null };
          column.cards.push(card);
          persistBoards();
          return structuredClone(card);
        }
        case 'move_board_card': {
          if (window.audit.failBoardMove) throw new Error('Fixture: board card move failed');
          const target = columnById(args.columnId);
          const source = boards.flatMap(board => board.columns).find(column => column.cards.some(card => card.id === args.cardId));
          const index = source?.cards.findIndex(card => card.id === args.cardId) ?? -1;
          if (!source || !target || index < 0) throw new Error('Fixture: board card move failed');
          const [card] = source.cards.splice(index, 1);
          card.position = target.cards.length;
          target.cards.push(card);
          persistBoards();
          return;
        }
        case 'list_meetings': return structuredClone(meetings);
        case 'list_note_drafts': return [...drafts.values()].map(({ id, title, created_at, updated_at }) => ({ id, title, created_at, updated_at }));
        case 'list_library_tree': return [{ kind: 'space', id: 1, name: 'Personal', children: window.audit.treeCount > 2 ? Array.from({ length: window.audit.treeCount }, (_, i) => ({ ...summary(i + 1, `Meeting ${i + 1}`), kind: 'meeting', children: [] })) : meetings.map(m => ({ ...m, kind: 'meeting', children: [] })) }];
        case 'get_note_draft':
          if (drafts.has(args.id)) return structuredClone(drafts.get(args.id));
          if (args.id !== 100) throw new Error('Fixture: note draft not found');
          return { id: 100, title: 'Recovery fixture', created_at: '2026-09-21T15:00:00Z', updated_at: '2026-09-21T15:00:00Z', raw_markdown: 'Recovered notes', meeting_id: null, recovery_duration_seconds: 60, recovery_location: null, recovery_transcript: Array.from({ length: 3 }, (_, i) => ({ channel: 'you', text: `Recovered sentence ${i + 1}`, start_ms: i * 2000, end_ms: i * 2000 + 1000 })) };
        case 'get_meeting': return { ...meetings.find(m => m.id === args.id), notepad: 'Sample meeting notes.', enhanced_markdown: '## Summary\n\nFixture summary.\n\n## Action items\n\n- Follow up with the team.', transcript: structuredClone(transcript) };
        case 'get_llm_config': if (window.audit.failConfig) throw new Error('Fixture: config database unavailable'); return structuredClone(config);
        case 'set_llm_config':
          config = { ...args.config, has_api_key: savedIdentities.has(JSON.stringify([args.config.kind, args.config.base_url])) };
          return;
        case 'test_llm_config': return;
        case 'list_templates': return [{ id: 1, name: 'General', prompt: 'Summarize {transcript} using {notes}', is_builtin: 1 }];
        case 'get_global_shortcut': return 'Ctrl+Shift+R';
        case 'set_global_shortcut': if (window.audit.failShortcut) throw new Error('Fixture: invalid shortcut'); return;
        case 'check_model_pack': await new Promise(r => setTimeout(r, window.audit.modelDelay)); return true;
        case 'search_meetings': {
          const q = args.query;
          if (window.audit.slowSearch) await new Promise(r => setTimeout(r, q === 'alpha' ? 800 : 20));
          if (window.audit.failSearch) throw new Error('Fixture: search unavailable');
          return structuredClone(meetings.filter(m => m.title.toLowerCase().includes(q)));
        }
        case 'update_segment_text':
          if (window.audit.failSegment) throw new Error('Fixture: segment write failed');
          await new Promise(r => setTimeout(r, 80));
          transcript.find(x => x.id === args.segmentId).text = args.text;
          return;
        case 'delete_segment': return;
        case 'update_notes': return;
        case 'rename_meeting': meetings.find(m => m.id === args.meetingId).title = args.title; return;
        case 'create_note_draft': return createDraft();
        case 'update_note_draft_recovery':
          if (window.audit.failRecovery) throw new Error('Fixture: recovery write rejected');
          window.audit.recovery = structuredClone(args); return;
        case 'delete_note_draft': return;
        case 'start_recording': return { meeting_audio_available: true, transcription_available: true };
        case 'pause_recording': return;
        case 'resume_recording': return { meeting_audio_available: true, transcription_available: true };
        case 'stop_recording': return { transcript: [], finalization_warning: null };
        default: throw new Error(`Fixture has no handler for ${cmd}`);
      }
    },
  };
})();
