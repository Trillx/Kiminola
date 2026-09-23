import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);

export async function runDictationTests({ check, open, eventually, origin }) {
  await check('Dictation settings require explicit enable and preserve a failed draft', async page => {
    await open(page, '/settings?section=dictation');
    const toggle = page.getByRole('switch', { name: 'Enable dictation', exact: true });
    await toggle.waitFor();
    assert.equal(await toggle.getAttribute('aria-checked'), 'false');
    assert.equal(await page.getByRole('checkbox', { name: 'Allow guarded paste using the clipboard' }).isChecked(), false);
    assert.equal(await page.getByRole('checkbox', { name: 'Save completed dictation history for 30 days' }).isChecked(), false);
    assert.equal(await page.evaluate(() => window.audit.calls.filter(call => call.cmd === 'start_dictation').length), 0);
    await page.evaluate(() => window.audit.failDictationSave = true);
    const thumb = toggle.locator('[data-slot="switch-thumb"]');
    const uncheckedPosition = await thumb.evaluate(element => getComputedStyle(element).translate);
    await toggle.click();
    await eventually(() => thumb.evaluate(element => getComputedStyle(element).translate), position => position !== uncheckedPosition, 'Enable switch visibly moves');
    await page.getByLabel('Dictation shortcut', { exact: true }).fill('Ctrl+Alt+D');
    await page.getByRole('button', { name: 'Save dictation settings', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'shortcut conflicts' }).waitFor();
    assert.equal(await page.getByLabel('Dictation shortcut', { exact: true }).inputValue(), 'Ctrl+Alt+D');
    await page.evaluate(() => window.audit.emit('dictation:state', window.audit.dictationState()));
    assert.equal(await toggle.getAttribute('aria-checked'), 'true');
    await page.evaluate(() => window.audit.failDictationSave = false);
    await page.getByRole('button', { name: 'Save dictation settings', exact: true }).click();
    await page.getByRole('status').filter({ hasText: 'Dictation settings saved.' }).waitFor();
    const actual = await page.evaluate(() => window.audit.dictationState());
    assert.equal(actual.settings.enabled, true);
    assert.equal(actual.settings.shortcut, 'Ctrl+Alt+D');
    assert.equal(actual.settings.clipboard_consent, false);
    const input = await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'set_dictation_settings').at(-1).args.input);
    assert.equal(input.consent_provider, false);
  });

  await check('Dictation settings explain backend start failures outside review and clear recovered errors', async page => {
    await open(page, '/settings?section=dictation');
    await page.getByRole('switch', { name: 'Enable dictation', exact: true }).waitFor();
    const settings = page.getByRole('region', { name: 'Dictation', exact: true });
    const recovery = page.getByRole('region', { name: 'Dictation recovery', exact: true });
    for (const failure of [
      { phase: 'idle', enabled: true, error: 'A Meeting is starting, recording, paused, or finalizing. Wait for it to end.' },
      { phase: 'disabled', enabled: false, error: 'The dictation shortcut could not be registered.' },
    ]) {
      await page.evaluate(({ phase, enabled, error }) => window.audit.setDictation({
        settings: { ...window.audit.dictationState().settings, enabled }, phase, error, text: '', raw_text: '', session_id: null,
      }), failure);
      const alert = settings.getByRole('alert').filter({ hasText: failure.error });
      await alert.waitFor();
      assert.equal(await recovery.count(), 0);
      assert.equal(await page.getByRole('complementary', { name: 'Dictation review available' }).count(), 0);
      assert.equal(await settings.getByText(/^Ready\./).count(), 0);
      for (const viewport of [{ width: 1200, height: 800 }, { width: 390, height: 844 }]) {
        await page.setViewportSize(viewport);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
        if (process.env.DICTATION_EVIDENCE_DIR) {
          await mkdir(process.env.DICTATION_EVIDENCE_DIR, { recursive: true });
          await alert.scrollIntoViewIfNeeded();
          await page.screenshot({ path: join(process.env.DICTATION_EVIDENCE_DIR, `dictation-settings-error-${failure.phase}-${viewport.width}x${viewport.height}.png`) });
        }
      }
      await page.evaluate(() => window.audit.setDictation({ error: null }));
      await alert.waitFor({ state: 'detached' });
      if (failure.enabled) await settings.getByText(/^Ready\./).waitFor();
    }
    assert.equal(await page.evaluate(() => window.audit.calls.some(c => ['start_dictation', 'start_recording', 'stop_recording', 'cancel_dictation'].includes(c.cmd))), false);
  });

  await check('Dictation responsive recovery is scrollable and accessible', async page => {
    await open(page, '/settings?section=dictation');
    await page.getByRole('switch', { name: 'Enable dictation', exact: true }).waitFor();
    await page.evaluate(() => window.audit.setDictation({ phase: 'review', session_id: 77, text: Array.from({ length: 80 }, (_, i) => `Synthetic recovery line ${i + 1}.`).join('\n'), error: 'The microphone was disconnected. Review available text.' }));
    const retained = page.getByRole('region', { name: 'Retained dictation text', exact: true });
    await retained.waitFor();
    await page.addScriptTag({ path: require.resolve('axe-core/axe.min.js') });
    const metrics = [];
    for (const viewport of [{ width: 1200, height: 800 }, { width: 1024, height: 1200 }, { width: 390, height: 844 }]) {
      await page.setViewportSize(viewport);
      await page.evaluate(() => document.fonts.ready);
      const measured = await retained.evaluate(element => ({
        overflowX: document.documentElement.scrollWidth > innerWidth,
        scrollable: element.scrollHeight > element.clientHeight,
        width: element.getBoundingClientRect().width,
        viewport: { width: innerWidth, height: innerHeight },
      }));
      assert.equal(measured.overflowX, false);
      assert.equal(measured.scrollable, true);
      metrics.push(measured);
      if (process.env.DICTATION_EVIDENCE_DIR) {
        await mkdir(process.env.DICTATION_EVIDENCE_DIR, { recursive: true });
        await retained.evaluate(element => { element.blur(); element.scrollTop = 0; });
        await page.getByRole('heading', { name: 'Settings', exact: true }).scrollIntoViewIfNeeded();
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
        await page.screenshot({ path: join(process.env.DICTATION_EVIDENCE_DIR, `dictation-recovery-${viewport.width}x${viewport.height}.png`) });
      }
      await retained.focus();
      await page.keyboard.press('End');
      await eventually(() => retained.evaluate(element => element.scrollTop), top => top > 0, 'Keyboard scrolls retained text');
    }
    const violations = await page.evaluate(async () => (await window.axe.run('.dictation-settings', { runOnly: ['wcag2a', 'wcag2aa', 'wcag21aa'] })).violations.map(({ id, impact, nodes }) => ({ id, impact, targets: nodes.map(node => node.target) })));
    assert.deepEqual(violations, []);
    if (process.env.DICTATION_EVIDENCE_DIR) await writeFile(join(process.env.DICTATION_EVIDENCE_DIR, 'dictation-layout-metrics.json'), JSON.stringify({ metrics, violations }, null, 2));
  });

  await check('Dictation pill fits 64 by 220 and skips main-window startup', async page => {
    await page.setViewportSize({ width: 64, height: 220 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto(origin + '/?window=dictation');
    const pill = page.getByRole('region', { name: 'Dictation pill', exact: true });
    await pill.waitFor({ timeout: 30000 });
    await page.waitForFunction(() => window.audit.hasListener('dictation:state'));
    const startup = await page.evaluate(() => window.audit.calls);
    assert.equal(startup.some(c => ['start_dictation', 'is_onboarding_complete', 'get_meeting_presence_state', 'plugin:updater|check', 'get_global_shortcut'].includes(c.cmd)), false);
    assert.equal(startup.some(c => c.cmd === 'plugin:event|listen' && c.args.event === 'shortcut:triggered'), false);
    await page.evaluate(() => window.audit.setDictation({ phase: 'listening', session_id: 12, level: 0.7, elapsed_seconds: 12, text: 'Never show spoken text here.' }));
    await pill.getByRole('button', { name: 'Stop dictation', exact: true }).waitFor();
    const geometry = await pill.evaluate(element => {
      const r = element.getBoundingClientRect();
      return { width: r.width, height: r.height, left: r.left, top: r.top, overflow: document.documentElement.scrollWidth > innerWidth || document.documentElement.scrollHeight > innerHeight };
    });
    assert.ok(geometry.width <= 64 && geometry.height <= 220 && geometry.left >= 0 && geometry.top >= 0);
    assert.equal(geometry.overflow, false);
    assert.equal((await pill.innerText()).includes('Never show spoken text here.'), false);
    if (process.env.DICTATION_EVIDENCE_DIR) {
      await mkdir(process.env.DICTATION_EVIDENCE_DIR, { recursive: true });
      await page.screenshot({ path: join(process.env.DICTATION_EVIDENCE_DIR, 'dictation-pill-64x220.png') });
    }
    await pill.getByRole('button', { name: 'Stop dictation', exact: true }).click();
    await eventually(() => pill.getAttribute('data-phase'), value => value === 'review', 'Stop transitions to review');
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'stop_dictation').length), 1);
    assert.equal((await pill.innerText()).includes('Synthetic final text.'), false);
  });

  await check('Dictation pill explains backend start failures without capture and clears recovered errors', async page => {
    await page.setViewportSize({ width: 64, height: 220 });
    await page.goto(origin + '/?window=dictation');
    const pill = page.getByRole('region', { name: 'Dictation pill', exact: true });
    await pill.waitFor({ timeout: 30000 });
    await page.waitForFunction(() => window.audit.hasListener('dictation:state'));
    const failures = [
      { phase: 'idle', enabled: true, error: 'A Meeting is starting, recording, paused, or finalizing. Wait for it to end.' },
      { phase: 'disabled', enabled: false, error: 'The dictation shortcut could not be registered.' },
    ];
    for (const failure of failures) {
      await page.evaluate(({ phase, enabled, error }) => window.audit.setDictation({
        settings: { ...window.audit.dictationState().settings, enabled }, phase, error, text: '', raw_text: '', session_id: null,
      }), failure);
      await pill.getByText('Error', { exact: true }).waitFor();
      assert.equal(await pill.getAttribute('data-phase'), failure.phase);
      assert.equal(await pill.getAttribute('title'), failure.error);
      assert.ok((await pill.getByRole('status').textContent()).includes(failure.error));
      assert.ok((await pill.getByRole('status').ariaSnapshot()).toLowerCase().includes(failure.error.toLowerCase()), 'Failure details are exposed to assistive technology');
      assert.equal(await pill.getByRole('status').getAttribute('aria-atomic'), 'true');
      const geometry = await pill.evaluate(element => {
        const r = element.getBoundingClientRect();
        return { width: r.width, height: r.height, left: r.left, top: r.top, overflow: document.documentElement.scrollWidth > innerWidth || document.documentElement.scrollHeight > innerHeight };
      });
      assert.ok(geometry.width <= 64 && geometry.height <= 220 && geometry.left >= 0 && geometry.top >= 0);
      assert.equal(geometry.overflow, false);
      assert.equal(await pill.getByRole('button').count(), 0);
      assert.equal(await pill.getByRole('meter').count(), 0);
      if (process.env.DICTATION_EVIDENCE_DIR) {
        await mkdir(process.env.DICTATION_EVIDENCE_DIR, { recursive: true });
        await page.screenshot({ path: join(process.env.DICTATION_EVIDENCE_DIR, `dictation-pill-error-${failure.phase}-64x220.png`) });
      }
      await page.evaluate(() => window.audit.setDictation({ error: null }));
      await pill.getByText(failure.enabled ? 'Ready' : 'Off', { exact: true }).waitFor();
      assert.equal((await pill.getByRole('status').textContent()).includes(failure.error), false);
      assert.equal((await pill.getAttribute('title')).includes(failure.error), false);
    }
    assert.equal(await page.evaluate(() => window.audit.calls.some(c => ['start_dictation', 'start_recording', 'stop_recording', 'cancel_dictation'].includes(c.cmd))), false);
  });

  await check('Dictation pill cancellation remains available during pending finalization', async page => {
    await page.setViewportSize({ width: 64, height: 220 });
    await page.goto(origin + '/?window=dictation');
    const pill = page.getByRole('region', { name: 'Dictation pill', exact: true });
    await pill.waitFor({ timeout: 30000 });
    await page.waitForFunction(() => window.audit.hasListener('dictation:state'));
    await page.evaluate(() => { window.audit.setDictation({ phase: 'listening', session_id: 23 }); window.audit.holdDictationStop = true; });
    await pill.getByRole('button', { name: 'Stop dictation', exact: true }).click();
    await page.waitForFunction(() => !!window.audit.releaseDictationStop);
    const cancel = pill.getByRole('button', { name: 'Cancel dictation', exact: true });
    assert.equal(await cancel.isDisabled(), false);
    await cancel.click();
    await eventually(() => pill.getAttribute('data-phase'), phase => phase === 'idle', 'Cancelled state');
    await page.evaluate(() => window.audit.releaseDictationStop());
    await page.waitForTimeout(100);
    assert.equal(await pill.getAttribute('data-phase'), 'idle');
  });

  await check('Dictation settings block unconsented provider saves and lock pending edits', async page => {
    await open(page, '/settings?section=dictation');
    await page.getByLabel('Text cleanup', { exact: true }).selectOption('provider');
    const save = page.getByRole('button', { name: 'Save dictation settings', exact: true });
    assert.equal(await save.isDisabled(), true);
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'set_dictation_settings').length), 0);
    await page.getByRole('checkbox', { name: 'Allow dictation text to be sent to the configured provider', exact: true }).check();
    await page.getByLabel('Activation', { exact: true }).selectOption('toggle');
    await page.getByLabel('Pill side', { exact: true }).selectOption('left');
    await page.getByLabel('Microphone', { exact: true }).selectOption('synthetic-mic');
    await page.getByRole('checkbox', { name: 'Allow guarded paste using the clipboard' }).check();
    await page.evaluate(() => window.audit.holdDictationSave = true);
    await save.click();
    await page.waitForFunction(() => !!window.audit.releaseDictationSave);
    assert.equal(await page.getByLabel('Dictation shortcut', { exact: true }).isDisabled(), true);
    await page.getByRole('tab', { name: 'General', exact: true }).click();
    assert.equal(await page.getByRole('tab', { name: 'Dictation', exact: true }).getAttribute('aria-selected'), 'true');
    await page.evaluate(() => window.audit.releaseDictationSave());
    await page.getByRole('status').filter({ hasText: 'Dictation settings saved.' }).waitFor();
    const input = await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'set_dictation_settings').at(-1).args.input);
    assert.deepEqual(input, { enabled: false, activation: 'toggle', shortcut: 'Ctrl+Shift+Space', side: 'left', cleanup: 'provider', clipboard_consent: true, history_enabled: false, microphone_id: 'synthetic-mic', consent_provider: true });
    await page.getByText('Dictation permission is saved for the current provider endpoint. A changed endpoint needs new permission.', { exact: true }).waitFor();
  });

  await check('Dictation disable never requires renewed provider permission and retains text for exit review', async page => {
    await open(page, '/settings?section=dictation');
    await page.getByRole('switch', { name: 'Enable dictation', exact: true }).waitFor();
    await page.evaluate(() => window.audit.setDictation({ settings: { ...window.audit.dictationState().settings, enabled: true, cleanup: 'provider' }, provider_authorized: false, phase: 'listening', session_id: 32, raw_text: 'Interrupted text to keep.' }));
    const toggle = page.getByRole('switch', { name: 'Enable dictation', exact: true });
    await eventually(() => toggle.getAttribute('aria-checked'), checked => checked === 'true', 'Enabled state loaded');
    await toggle.click();
    const save = page.getByRole('button', { name: 'Save dictation settings', exact: true });
    assert.equal(await save.isDisabled(), false);
    await save.click();
    const recovery = page.getByRole('region', { name: 'Dictation recovery', exact: true });
    await recovery.getByRole('button', { name: 'Copy and disable', exact: true }).waitFor();
    assert.match(await recovery.innerText(), /Interrupted text to keep\./);
    await recovery.getByRole('button', { name: 'Discard and disable', exact: true }).click();
    await recovery.waitFor({ state: 'detached' });
    assert.equal(await page.evaluate(() => window.audit.dictationState().settings.enabled), false);
    await eventually(() => page.getByText('Unsaved changes', { exact: true }).count(), count => count === 0, 'Completed disable acknowledges saved draft');
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'set_dictation_settings').at(-1).args.input.consent_provider), false);
  });

  await check('Dictation dirty section navigation asks once and respects keep editing', async page => {
    await open(page, '/settings?section=dictation');
    await page.getByLabel('Dictation shortcut', { exact: true }).fill('Ctrl+Alt+D');
    const reject = dialog => dialog.dismiss();
    page.on('dialog', reject);
    await page.getByRole('tab', { name: 'General', exact: true }).click();
    assert.equal(await page.getByLabel('Dictation shortcut', { exact: true }).inputValue(), 'Ctrl+Alt+D');
    page.off('dialog', reject);
    let confirmations = 0;
    page.on('dialog', async dialog => { confirmations++; await dialog.accept(); });
    await page.getByRole('tab', { name: 'General', exact: true }).click();
    await eventually(() => page.getByRole('tab', { name: 'General', exact: true }).getAttribute('aria-selected'), selected => selected === 'true', 'General section selected');
    assert.equal(confirmations, 1);
    assert.equal(new URL(page.url()).searchParams.has('section'), false);
  });

  await check('Dictation active controls support stop and cancellation from main settings', async page => {
    await open(page, '/settings?section=dictation');
    await page.getByRole('switch', { name: 'Enable dictation', exact: true }).waitFor();
    await page.evaluate(() => window.audit.setDictation({ phase: 'listening', session_id: 18, level: 0.5 }));
    const stop = page.getByRole('button', { name: 'Stop dictation', exact: true });
    await stop.waitFor();
    await stop.focus();
    await page.keyboard.press('Enter');
    await page.getByRole('region', { name: 'Dictation recovery', exact: true }).waitFor();
    await page.getByRole('button', { name: 'Discard text', exact: true }).click();
    await page.evaluate(() => window.audit.setDictation({ phase: 'processing', session_id: 19 }));
    await page.getByRole('button', { name: 'Cancel dictation', exact: true }).click();
    await eventually(() => page.evaluate(() => window.audit.dictationState().phase), phase => phase === 'idle', 'Cancellation ends processing');
    const calls = await page.evaluate(() => window.audit.calls.map(c => c.cmd));
    assert.equal(calls.filter(cmd => cmd === 'stop_dictation').length, 1);
    assert.equal(calls.filter(cmd => cmd === 'cancel_dictation').length, 1);
    assert.equal(calls.filter(cmd => cmd === 'start_dictation').length, 0);
  });

  await check('Dictation main-window notice leads to keyboard recovery and cancel-exit keeps text', async page => {
    await open(page);
    await page.waitForFunction(() => window.audit.hasListener('dictation:state'));
    await page.evaluate(() => window.audit.setDictation({ phase: 'review', session_id: 9, text: 'Text retained before quit.', exit_intent: 'quit' }));
    const reviewLink = page.getByRole('link', { name: 'Review dictation', exact: true });
    await reviewLink.waitFor();
    await reviewLink.focus();
    await page.keyboard.press('Enter');
    const recovery = page.getByRole('region', { name: 'Dictation recovery', exact: true });
    await recovery.waitFor();
    await recovery.getByRole('button', { name: 'Cancel quit', exact: true }).click();
    await recovery.getByRole('button', { name: 'Copy text', exact: true }).waitFor();
    assert.match(await recovery.innerText(), /Text retained before quit\./);
    assert.equal(await page.evaluate(() => window.audit.dictationState().phase), 'review');
    assert.equal(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'start_dictation')), false);
  });

  await check('Dictation transport failures stay accessible and recover in pill and settings', async page => {
    await page.setViewportSize({ width: 64, height: 220 });
    await page.goto(origin + '/?window=dictation');
    const pill = page.getByRole('region', { name: 'Dictation pill', exact: true });
    await pill.waitFor({ timeout: 30000 });
    await page.waitForFunction(() => window.audit.hasListener('dictation:state'));
    await page.evaluate(() => {
      window.audit.failDictationRead = true;
      window.audit.emit('dictation:state', window.audit.dictationState());
    });
    await pill.getByText('Error', { exact: true }).waitFor();
    assert.match(await pill.getAttribute('title'), /dictation state unavailable/);
    assert.match(await pill.getByRole('status').textContent(), /dictation state unavailable/);
    await page.evaluate(() => { window.audit.failDictationRead = false; window.audit.setDictation({ error: null }); });
    await pill.getByText('Off', { exact: true }).waitFor();
    assert.doesNotMatch(await pill.textContent(), /dictation state unavailable/);
    assert.doesNotMatch(await pill.getAttribute('title'), /dictation state unavailable/);
    assert.equal(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'start_dictation')), false);

    await page.setViewportSize({ width: 1200, height: 800 });
    await open(page, '/settings?section=dictation');
    await page.getByRole('switch', { name: 'Enable dictation', exact: true }).waitFor();
    const settings = page.getByRole('region', { name: 'Dictation', exact: true });
    await page.evaluate(() => {
      window.audit.setDictation({ settings: { ...window.audit.dictationState().settings, enabled: true }, phase: 'idle' });
    });
    await settings.getByText(/^Ready\./).waitFor();
    await page.evaluate(() => {
      window.audit.failDictationRead = true;
      window.audit.emit('dictation:state', window.audit.dictationState());
    });
    const alert = settings.getByRole('alert').filter({ hasText: 'Could not refresh dictation:' });
    await alert.waitFor();
    assert.match(await alert.textContent(), /dictation state unavailable/);
    assert.equal(await settings.getByText(/^Ready\./).count(), 0);
    await page.evaluate(() => window.audit.failDictationRead = false);
    await settings.getByRole('button', { name: 'Retry dictation state', exact: true }).click();
    await alert.waitFor({ state: 'detached' });
    await settings.getByText(/^Ready\./).waitFor();
    assert.equal(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'start_dictation')), false);
  });

  await check('Dictation review visibility follows the main-window route and avoids duplicate errors', async page => {
    await open(page);
    await page.waitForFunction(() => window.audit.hasListener('dictation:state'));
    const notice = page.getByRole('complementary', { name: 'Dictation review available' });
    const recovery = page.getByRole('region', { name: 'Dictation recovery', exact: true });
    const message = 'Cleanup failed. Review the raw transcript.';
    await page.evaluate(error => window.audit.setDictation({ phase: 'review', session_id: 91, text: '', raw_text: 'Synthetic text retained for review.', error }), message);
    await notice.getByRole('link', { name: 'Review dictation', exact: true }).click();
    await recovery.waitFor();
    await notice.waitFor({ state: 'detached' });
    assert.equal(await page.getByRole('alert').filter({ hasText: message }).count(), 1);
    assert.equal(await recovery.getByRole('alert').textContent(), message);
    await page.getByRole('tab', { name: 'General', exact: true }).click();
    await notice.waitFor();
    await recovery.waitFor({ state: 'detached' });
    await page.getByRole('tab', { name: 'Dictation', exact: true }).click();
    await recovery.waitFor();
    await notice.waitFor({ state: 'detached' });
    await page.getByRole('link', { name: 'Meetings', exact: true }).click();
    await notice.waitFor();
    for (const phase of ['idle', 'disabled']) {
      await page.evaluate(phase => window.audit.setDictation({ phase, error: 'The dictation shortcut failed.', raw_text: '', text: '', session_id: null }), phase);
      await notice.waitFor({ state: 'detached' });
      assert.equal(await recovery.count(), 0);
    }
    assert.equal(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'start_dictation')), false);
  });

  await check('Dictation history timestamps include relative days and absolute local time', async page => {
    await page.clock.setFixedTime(new Date('2026-09-23T05:30:00Z'));
    await open(page, '/settings?section=dictation');
    const history = page.getByRole('region', { name: 'Dictation history', exact: true });
    await history.getByText('Synthetic completed dictation.', { exact: true }).waitFor();
    const entries = [
      { id: 41, text: 'Today fixture.', created_at: '2026-09-23T05:15:00Z' },
      { id: 42, text: 'Yesterday across UTC midnight.', created_at: '2026-09-23T04:45:00Z' },
      { id: 43, text: 'Older fixture.', created_at: '2026-09-20T14:00:00Z' },
    ];
    await page.evaluate(entries => window.audit.seedDictationHistory(entries), entries);
    await history.getByRole('button', { name: 'Refresh history', exact: true }).click();
    await history.getByText('Today fixture.', { exact: true }).waitFor();
    const times = history.locator('time');
    assert.deepEqual(await times.allTextContents(), [
      'today · Sep 23, 2026, 12:15 AM',
      'yesterday · Sep 22, 2026, 11:45 PM',
      '3 days ago · Sep 20, 2026, 9:00 AM',
    ]);
    assert.deepEqual(await times.evaluateAll(elements => elements.map(element => element.getAttribute('datetime'))), entries.map(entry => entry.created_at));
    for (const viewport of [{ width: 1200, height: 800 }, { width: 390, height: 844 }]) {
      await page.setViewportSize(viewport);
      await times.first().scrollIntoViewIfNeeded();
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
      if (process.env.DICTATION_EVIDENCE_DIR) {
        await mkdir(process.env.DICTATION_EVIDENCE_DIR, { recursive: true });
        await page.screenshot({ path: join(process.env.DICTATION_EVIDENCE_DIR, `dictation-history-${viewport.width}x${viewport.height}.png`) });
      }
    }
    await page.clock.setFixedTime(new Date('2026-09-24T05:30:00Z'));
    await history.getByRole('button', { name: 'Refresh history', exact: true }).click();
    await eventually(() => times.allTextContents(), labels => labels[0].startsWith('yesterday · '), 'Refresh uses the current local day');
    assert.deepEqual(await times.allTextContents(), [
      'yesterday · Sep 23, 2026, 12:15 AM',
      '2 days ago · Sep 22, 2026, 11:45 PM',
      '4 days ago · Sep 20, 2026, 9:00 AM',
    ]);
  }, { locale: 'en-US', timezoneId: 'America/Chicago' });

  await check('Dictation history uses local calendar days across daylight-saving changes', async page => {
    await open(page, '/settings?section=dictation');
    const history = page.getByRole('region', { name: 'Dictation history', exact: true });
    await history.getByText('Synthetic completed dictation.', { exact: true }).waitFor();
    for (const scenario of [
      { now: '2026-03-09T05:30:00Z', date: '2026-03-08T05:45:00Z', expected: '2 days ago · Mar 7, 2026, 11:45 PM' },
      { now: '2026-11-02T06:30:00Z', date: '2026-11-01T05:15:00Z', expected: 'yesterday · Nov 1, 2026, 12:15 AM' },
    ]) {
      await page.clock.setFixedTime(new Date(scenario.now));
      await page.evaluate(date => window.audit.seedDictationHistory([{ id: 44, text: 'Daylight-saving fixture.', created_at: date }]), scenario.date);
      await history.getByRole('button', { name: 'Refresh history', exact: true }).click();
      await eventually(() => history.locator('time').textContent(), text => text === scenario.expected, 'Calendar-relative timestamp across DST');
      assert.equal(await history.locator('time').getAttribute('datetime'), scenario.date);
    }
  }, { locale: 'en-US', timezoneId: 'America/Chicago' });

  await check('Dictation history deletion reads back state and preserves entries on failure', async page => {
    await open(page, '/settings?section=dictation');
    const history = page.getByRole('region', { name: 'Dictation history', exact: true });
    await history.waitFor();
    await history.getByText('Synthetic completed dictation.', { exact: true }).waitFor();
    await page.evaluate(() => window.audit.failDictationDelete = true);
    await history.getByRole('button', { name: 'Delete history entry', exact: true }).click();
    await history.getByRole('alert').filter({ hasText: 'history delete failed' }).waitFor();
    assert.match(await history.innerText(), /Synthetic completed dictation\./);
    await page.evaluate(() => window.audit.failDictationDelete = false);
    await history.getByRole('button', { name: 'Clear all history', exact: true }).click();
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'delete_dictation_history').length), 1);
    await history.getByRole('button', { name: 'Delete all entries', exact: true }).click();
    await history.getByText('No completed dictation history.', { exact: true }).waitFor();
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'delete_dictation_history').at(-1).args.id), null);
    assert.equal(await page.evaluate(() => window.audit.calls.at(-1).cmd), 'list_dictation_history');
  });

  await check('Dictation recovery retains raw text after failed copy and ignores stale completions', async page => {
    await open(page, '/settings?section=dictation');
    await page.getByRole('switch', { name: 'Enable dictation', exact: true }).waitFor();
    await page.evaluate(() => window.audit.setDictation({ phase: 'review', session_id: 4, text: '', raw_text: 'Raw words to recover.', error: 'Cleanup failed.', delivery: 'none' }));
    const recovery = page.getByRole('region', { name: 'Dictation recovery', exact: true });
    await recovery.waitFor();
    assert.match(await recovery.innerText(), /Raw words to recover\./);
    await page.evaluate(() => window.audit.failDictationResolve = true);
    await recovery.getByRole('button', { name: 'Copy text', exact: true }).click();
    await recovery.getByRole('alert').filter({ hasText: 'clipboard unavailable' }).waitFor();
    assert.match(await recovery.innerText(), /Raw words to recover\./);
    await page.evaluate(() => { window.audit.failDictationResolve = false; window.audit.holdDictationResolve = true; });
    await recovery.getByRole('button', { name: 'Copy text', exact: true }).click();
    await page.waitForFunction(() => !!window.audit.releaseDictationResolve);
    assert.equal(await page.evaluate(() => window.audit.copiedDictation), 'Raw words to recover.');
    await page.evaluate(() => {
      window.audit.setDictation({ phase: 'review', session_id: 5, text: 'New recovery must survive.', raw_text: '', error: null, delivery: 'uncertain' });
      window.audit.releaseDictationResolve();
      window.audit.holdDictationResolve = false;
      window.audit.emit('dictation:state', { ...window.audit.dictationState(), phase: 'idle', text: '', session_id: 4 });
    });
    await eventually(() => recovery.innerText(), value => value.includes('New recovery must survive.'), 'New review retained');
    await recovery.getByRole('button', { name: 'I verified the insertion', exact: true }).click();
    await recovery.waitFor({ state: 'detached' });
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'resolve_dictation').at(-1).args.action), 'confirm');
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'start_dictation').length), 0);
  });
}
