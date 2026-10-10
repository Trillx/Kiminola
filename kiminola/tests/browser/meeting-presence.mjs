import assert from 'node:assert/strict';

// Exercise the shipping component and routes, not /prototype/meeting-presence.
// Only IPC/window transport is synthetic; no native capture or user data is used.
const commands = {
  notes: 'jot_notes_from_meeting_prompt',
  start: 'start_recording_from_meeting_prompt',
  dismiss: 'dismiss_meeting_prompt',
};
const buttons = { notes: 'Jot notes', start: 'Start recording', dismiss: 'Not now' };
const makePrompt = (id = 'presence-1', app = 'Synthetic call app') => ({
  id, app_label: app,
  message: 'You may be in a meeting. Want to jot notes?',
  not_recording_message: 'Kimi Nola is not recording.',
  confidence: 'likely', evidence: ['app_or_visible_window', 'active_core_audio'],
});
const card = page => page.getByRole('complementary', { name: 'Meeting prompt', exact: true });
const actionCalls = page => page.evaluate(() => window.audit.calls.filter(c => [
  'jot_notes_from_meeting_prompt', 'start_recording_from_meeting_prompt', 'dismiss_meeting_prompt',
].includes(c.cmd)));
const captureCalls = page => page.evaluate(() => window.audit.calls.filter(c => [
  'start_recording', 'resume_recording',
].includes(c.cmd)));

async function ready(page) {
  await page.waitForFunction(() => ['prompt', 'state', 'error', 'action'].every(name =>
    window.audit?.hasListener(`meeting-presence:${name}`)));
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
}
async function publish(page, prompt = makePrompt()) {
  await page.evaluate(prompt => window.audit.emit('meeting-presence:prompt', prompt), prompt);
  await card(page).waitFor();
  assert.equal(await card(page).locator('.prompt-kicker').textContent(), prompt.app_label);
}
async function clear(page) {
  await page.evaluate(async () => {
    const state = await window.__TAURI_INTERNALS__.invoke('get_meeting_presence_state');
    window.audit.emit('meeting-presence:state', { ...state, hint: null, prompt: null });
  });
  await card(page).waitFor({ state: 'detached' });
}
async function assertNotCapturing(page) {
  assert.deepEqual(await captureCalls(page), [], 'Presence/notes/dismiss must never invoke capture');
}
async function assertConsumed(page, promptId) {
  const result = await page.evaluate(async ({ promptId, commands }) => {
    const errors = [];
    for (const cmd of commands) {
      try { await window.__TAURI_INTERNALS__.invoke(cmd, { promptId }); errors.push('accepted'); }
      catch (error) { errors.push(error.message); }
    }
    return errors;
  }, { promptId, commands: Object.values(commands) });
  assert.deepEqual(result, Object.values(commands).map(() => 'meeting prompt is no longer active'));
}

// A deterministic IPC barrier: "before" races backend validation; "after"
// delays a real fixture response (including errors and captured snapshots).
async function holdNext(page, cmd, phase = 'before') {
  await page.evaluate(({ cmd, phase }) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    let used = false;
    window.audit.gates ??= {};
    const gate = window.audit.gates[cmd] = { pending: false };
    const released = new Promise(resolve => gate.release = resolve);
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command !== cmd || used) return invoke(command, args);
      used = true;
      if (phase === 'before') { gate.pending = true; await released; }
      let value, failure;
      try { value = await invoke(command, args); } catch (error) { failure = error; }
      if (phase === 'after') { gate.pending = true; await released; }
      if (failure) throw failure;
      return value;
    };
  }, { cmd, phase });
}
async function waitPending(page, cmd) {
  await page.waitForFunction(cmd => window.audit.gates[cmd]?.pending, cmd);
}
async function release(page, cmd) {
  await page.evaluate(cmd => window.audit.gates[cmd].release(), cmd);
  // Let the async component continuation and its Svelte DOM flush complete.
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
}

// Use a second real frontend realm for the overlay. The binding only transports
// the actual emitTo IPC and synthetic draft storage; it never drives navigation.
async function connectOverlay(main, origin) {
  const overlay = await main.context().newPage();
  const errors = [];
  overlay.on('pageerror', error => errors.push(error.message));
  overlay.setDefaultTimeout(7000);
  await overlay.exposeBinding('fixtureSendToMain', async (_source, args) => {
    assert.deepEqual(args.target, { kind: 'AnyLabel', label: 'main' });
    assert.equal(args.event, 'meeting-presence:action');
    const draft = args.payload.draft_id === undefined ? null : await overlay.evaluate(
      id => window.__TAURI_INTERNALS__.invoke('get_note_draft', { id }), args.payload.draft_id);
    await main.evaluate(({ event, payload, draft }) => {
      if (draft) window.audit.seedNoteDraft(draft);
      window.audit.emit(event, payload);
    }, { ...args, draft });
  });
  await overlay.goto(origin + '/?window=meeting-prompt');
  await ready(overlay);
  await overlay.evaluate(() => window.audit.sendTo = args => window.fixtureSendToMain(args));
  return { overlay, errors };
}

export async function runMeetingPresenceTests({ check, open, eventually, origin }) {
  for (const surface of ['main', 'overlay']) {
    const openSurface = async page => {
      if (surface === 'main') await open(page);
      else await page.goto(origin + '/?window=meeting-prompt');
      await ready(page);
    };

    await check(`MP ${surface}: received prompt is visible, uncertain, and never auto-records`, async page => {
      await openSurface(page);
      await publish(page);
      assert.equal(await card(page).locator('.prompt-title').innerText(), makePrompt().message);
      assert.equal(await card(page).locator('.prompt-copy').innerText(), 'Kimi Nola is not recording.');
      for (const name of Object.values(buttons)) {
        assert.equal(await card(page).getByRole('button', { name, exact: true }).isEnabled(), true);
      }
      // A state refresh of the same received prompt is still not an action.
      await page.evaluate(async () => window.audit.emit('meeting-presence:state',
        await window.__TAURI_INTERNALS__.invoke('get_meeting_presence_state')));
      await page.waitForTimeout(100);
      assert.deepEqual(await actionCalls(page), []);
      await assertNotCapturing(page);
      assert.equal(new URL(page.url()).pathname, '/');
      if (surface === 'overlay') {
        assert.equal(await page.locator('.main-content').count(), 0);
        assert.deepEqual(await page.evaluate(() => [document.documentElement.style.background, document.body.style.background]), ['transparent', 'transparent']);
      }
    });

    await check(`MP ${surface}: late startup snapshot cannot erase a received prompt`, async page => {
      const route = surface === 'main' ? '/?holdPresenceSnapshot' : '/?window=meeting-prompt&holdPresenceSnapshot';
      await page.goto(origin + route);
      await ready(page);
      await page.waitForFunction(() => window.audit.initialPresenceSnapshot?.release);
      await publish(page, makePrompt('presence-new', 'Newer call app'));
      await page.evaluate(() => window.audit.initialPresenceSnapshot.release());
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
      assert.equal(await card(page).count(), 1, 'A stale initial snapshot must not clear an event-delivered prompt');
      assert.equal(await card(page).locator('.prompt-kicker').textContent(), 'Newer call app');
      await assertNotCapturing(page);
    });

    await check(`MP ${surface}: dismiss consumes only once without notes or capture`, async page => {
      await openSurface(page);
      await publish(page);
      // Same-turn double click also exercises the JS busy guard, not just disabled CSS.
      await card(page).getByRole('button', { name: 'Not now', exact: true }).evaluate(button => {
        button.click(); button.click();
      });
      await card(page).waitFor({ state: 'detached' });
      assert.deepEqual(await actionCalls(page), [{ cmd: commands.dismiss, args: { promptId: 'presence-1' } }]);
      assert.equal(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'create_note_draft')), false);
      await assertConsumed(page, 'presence-1');
      await assertNotCapturing(page);
      if (surface === 'overlay') {
        await eventually(() => page.evaluate(() => window.audit.windows['meeting-prompt'].visible), visible => !visible, 'Dismiss hides overlay window');
      }
    });

    await check(`MP ${surface}: clearing backend state removes the prompt`, async page => {
      await openSurface(page);
      await publish(page);
      await clear(page);
      await assertNotCapturing(page);
      assert.deepEqual(await actionCalls(page), []);
      if (surface === 'overlay') {
        assert.equal(await page.evaluate(() => window.audit.windows['meeting-prompt'].visible), false);
        assert.ok(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'plugin:window|hide' && c.args.label === 'meeting-prompt')));
      }
    });

    await check(`MP ${surface}: late dismiss success cannot discard a newer prompt`, async page => {
      await openSurface(page);
      await publish(page);
      await holdNext(page, commands.dismiss, 'after');
      await card(page).getByRole('button', { name: 'Not now', exact: true }).click();
      await waitPending(page, commands.dismiss);
      await publish(page, makePrompt('presence-2', 'Newer call app'));
      await release(page, commands.dismiss);
      assert.equal(await card(page).count(), 1, 'Old completion must preserve the replacement card');
      assert.equal(await card(page).locator('.prompt-kicker').textContent(), 'Newer call app');
      assert.equal(await card(page).getByRole('button', { name: 'Not now', exact: true }).isEnabled(), true);
      assert.equal(await page.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke('get_meeting_presence_state')).prompt?.id), 'presence-2');
      if (surface === 'overlay') assert.equal(await page.evaluate(() => window.audit.windows['meeting-prompt'].visible), true);
      await assertNotCapturing(page);
    });

    for (const action of ['notes', 'start', 'dismiss']) {
      await check(`MP ${surface}: stale ${buttons[action]} is rejected without consuming its replacement`, async page => {
        await openSurface(page);
        await publish(page);
        await holdNext(page, commands[action]);
        await card(page).getByRole('button', { name: buttons[action], exact: true }).click();
        await waitPending(page, commands[action]);
        for (const name of Object.values(buttons)) {
          assert.equal(await card(page).getByRole('button', { name, exact: true }).isDisabled(), true);
          await card(page).getByRole('button', { name, exact: true }).evaluate(button => button.click());
        }
        await publish(page, makePrompt('presence-2', 'Newer call app'));
        await release(page, commands[action]);
        await eventually(() => card(page).getByRole('button', { name: 'Not now', exact: true }).isEnabled(), enabled => enabled, 'Stale action releases lock');
        assert.equal(await card(page).locator('.prompt-kicker').textContent(), 'Newer call app');
        assert.deepEqual(await actionCalls(page), [{ cmd: commands[action], args: { promptId: 'presence-1' } }]);
        assert.equal(new URL(page.url()).pathname, '/');
        await assertNotCapturing(page);
        if (surface === 'overlay') assert.equal(await page.evaluate(() => window.audit.windows['meeting-prompt'].visible), true);
        // The retained replacement is still actionable using its own ID.
        await card(page).getByRole('button', { name: 'Not now', exact: true }).click();
        await card(page).waitFor({ state: 'detached' });
        assert.deepEqual((await actionCalls(page)).at(-1), { cmd: commands.dismiss, args: { promptId: 'presence-2' } });
      });

      await check(`MP ${surface}: failed ${buttons[action]} retains a usable prompt and never captures`, async page => {
        await openSurface(page);
        await publish(page);
        await page.evaluate(cmd => window.audit.failPresenceAction = cmd, commands[action]);
        await card(page).getByRole('button', { name: buttons[action], exact: true }).click();
        await card(page).locator('.prompt-error').waitFor();
        assert.equal(await card(page).getByRole('button', { name: 'Not now', exact: true }).isEnabled(), true);
        assert.equal(new URL(page.url()).pathname, '/');
        assert.equal(await page.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke('get_meeting_presence_state')).prompt?.id), 'presence-1');
        await assertNotCapturing(page);
        await page.evaluate(() => window.audit.failPresenceAction = null);
        await card(page).getByRole('button', { name: 'Not now', exact: true }).click();
        await card(page).waitFor({ state: 'detached' });
        await assertConsumed(page, 'presence-1');
      });
    }

    await check(`MP ${surface}: consumed start validation failure remains visible`, async page => {
      await openSurface(page);
      await publish(page);
      await page.evaluate(cmd => window.audit.consumeAndFailPresenceAction = cmd, commands.start);
      await card(page).getByRole('button', { name: buttons.start, exact: true }).click();
      await card(page).locator('.prompt-error').waitFor();
      assert.match(
        await card(page).locator('.prompt-error').innerText(),
        /start a new recording manually/i,
      );
      assert.equal(await page.evaluate(async () =>
        (await window.__TAURI_INTERNALS__.invoke('get_meeting_presence_state')).prompt), null);
      assert.equal(new URL(page.url()).pathname, '/');
      await assertNotCapturing(page);
      if (surface === 'overlay') {
        assert.equal(await page.evaluate(() => window.audit.windows['meeting-prompt'].visible), true);
      }
    });

    await check(`MP ${surface}: stale validation failure cannot annotate a replacement prompt`, async page => {
      await openSurface(page);
      await publish(page, makePrompt('replacement', 'Replacement call'));
      await page.evaluate(() => window.audit.emit('meeting-presence:error', {
        prompt_id: 'consumed-prompt',
        message: 'The detected app is no longer available. Please start a new recording manually.',
      }));
      assert.equal(await card(page).locator('.prompt-kicker').textContent(), 'Replacement call');
      assert.equal(await card(page).locator('.prompt-error').count(), 0);
      await assertNotCapturing(page);
    });

    for (const failRefresh of [false, true]) {
      await check(`MP ${surface}: failed action's late ${failRefresh ? 'failed' : 'stale'} refresh cannot discard a newer prompt`, async page => {
        await openSurface(page);
        await publish(page);
        await page.evaluate(({ cmd, failRefresh }) => {
          window.audit.failPresenceAction = cmd;
          window.audit.failPresenceState = failRefresh;
        }, { cmd: commands.notes, failRefresh });
        await holdNext(page, 'get_meeting_presence_state', 'after');
        await card(page).getByRole('button', { name: 'Jot notes', exact: true }).click();
        await waitPending(page, 'get_meeting_presence_state');
        await publish(page, makePrompt('presence-2', 'Newer call app'));
        await release(page, 'get_meeting_presence_state');
        assert.equal(await card(page).count(), 1, 'Failed refresh must not clear an event-delivered replacement');
        assert.equal(await card(page).locator('.prompt-kicker').textContent(), 'Newer call app');
        assert.equal(await card(page).getByRole('button', { name: 'Not now', exact: true }).isEnabled(), true);
        assert.equal(await card(page).locator('.prompt-error').count(), 0, 'Old error must not label the replacement inactive');
        await assertNotCapturing(page);
      });
    }

    await check(`MP ${surface}: late Jot notes success and handoff preserve newer prompts`, async main => {
      await open(main);
      await ready(main);
      const connected = surface === 'overlay' ? await connectOverlay(main, origin) : null;
      const source = connected?.overlay ?? main;
      try {
        await publish(source);
        await holdNext(source, commands.notes, 'after');
        await card(source).getByRole('button', { name: 'Jot notes', exact: true }).click();
        await waitPending(source, commands.notes);
        const replacement = makePrompt('presence-2', 'Newer call app');
        await publish(source, replacement);
        if (connected) await publish(main, replacement);
        await release(source, commands.notes);
        await main.waitForURL('**/note/101');
        assert.equal(await card(source).count(), 1, 'Notes completion must preserve its replacement');
        assert.equal(await card(main).count(), 1, 'Cross-window action must not clear a newer main prompt');
        assert.equal(await card(main).locator('.prompt-kicker').textContent(), 'Newer call app');
        await assertNotCapturing(main);
        await assertNotCapturing(source);
        if (connected) {
          assert.equal(await source.evaluate(() => window.audit.windows['meeting-prompt'].visible), true);
          assert.deepEqual(connected.errors, []);
        }
      } finally {
        await connected?.overlay.close();
      }
    });

    for (const action of ['notes', 'start']) {
      await check(`MP ${surface}: explicit ${buttons[action]} routes ${action === 'notes' ? 'without capture' : 'before capture starts'}`, async main => {
        await open(main);
        await ready(main);
        const connected = surface === 'overlay' ? await connectOverlay(main, origin) : null;
        const source = connected?.overlay ?? main;
        try {
          await publish(source);
          await assertNotCapturing(main);
          await assertNotCapturing(source);
          await card(source).getByRole('button', { name: buttons[action], exact: true }).click();
          await main.waitForURL(action === 'notes' ? '**/note/101' : '**/record');
          await card(source).waitFor({ state: 'detached' });
          assert.deepEqual(await actionCalls(source), [{ cmd: commands[action], args: { promptId: 'presence-1' } }]);
          if (action === 'notes') {
            await main.getByRole('textbox', { name: 'Note draft', exact: true }).waitFor();
            assert.equal(await main.getByRole('textbox', { name: 'Note draft', exact: true }).inputValue(), '');
            assert.equal(await main.locator('.post-meta-pill').innerText(), 'Note draft');
            await assertNotCapturing(main);
          } else {
            await eventually(() => main.locator('.recording-badge').textContent(), text => text === 'Recording', 'Explicit action starts capture');
            assert.equal((await captureCalls(main)).length, 1);
            if (surface === 'main') {
              const calls = await main.evaluate(() => window.audit.calls.map(c => c.cmd));
              assert.ok(calls.indexOf(commands.start) < calls.indexOf('start_recording'), 'Prompt must be validated before capture');
            }
          }
          await assertConsumed(source, 'presence-1');
          if (connected) {
            await assertNotCapturing(source);
            assert.equal(new URL(source.url()).search, '?window=meeting-prompt');
            const calls = await source.evaluate(() => window.audit.calls);
            const handoff = calls.filter(c => c.cmd === 'plugin:event|emit_to');
            assert.deepEqual(handoff.map(c => c.args), [{
              target: { kind: 'AnyLabel', label: 'main' }, event: 'meeting-presence:action',
              payload: action === 'notes' ? { action, draft_id: 101 } : { action },
            }]);
            const windows = await source.evaluate(() => window.audit.windows);
            assert.equal(windows['meeting-prompt'].visible, false);
            assert.deepEqual(windows.main, { visible: true, focused: true });
            assert.deepEqual(connected.errors, [], 'Overlay has no unhandled JavaScript errors');
          }
        } finally {
          await connected?.overlay.close();
        }
      });
    }
  }
}
