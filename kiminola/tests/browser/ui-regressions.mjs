import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createRequire } from 'node:module';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { build, preview } from 'vite';
import { runOnboardingProviderTests } from './onboarding-provider.mjs';
import { runMeetingPresenceTests } from './meeting-presence.mjs';
import { runBoardsTests } from './boards.mjs';
import { runDictationTests } from './dictation.mjs';

// Real frontend, synthetic IPC only. Never opens the native app or the user's browser profile.
const require = createRequire(import.meta.url);
// Rebuild for standalone runs too. Development dependency discovery can replace
// shared chunks during route changes; exercise the immutable shipped bundles.
process.env.NODE_ENV = 'production';
await build({ logLevel: 'warn' });
const server = await preview({ preview: { host: '127.0.0.1', port: 0, strictPort: false }, logLevel: 'warn' });
const origin = server.resolvedUrls.local[0].replace(/\/$/, '');
let browser;
async function eventually(read, check, message) {
  const deadline = Date.now() + 7000;
  let value;
  do {
    value = await read();
    if (check(value)) return value;
    await new Promise(resolve => setTimeout(resolve, 30));
  } while (Date.now() < deadline);
  assert.fail(`${message}: ${JSON.stringify(value)}`);
}
async function check(name, run, contextOptions = {}) {
  if (process.env.UI_TEST_FILTER && !name.includes(process.env.UI_TEST_FILTER)) return;
  await test(name, async () => {
    const context = await browser.newContext({ viewport: { width: 1200, height: 800 }, colorScheme: 'light', ...contextOptions });
    await context.addInitScript({ path: fileURLToPath(new URL('./fixture.mjs', import.meta.url)) });
    const page = await context.newPage();
    page.setDefaultTimeout(7000);
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    try {
      await run(page);
      assert.deepEqual(errors, [], 'No unhandled JavaScript errors');
    } catch (error) {
      console.error(name, { errors, body: (await page.locator('body').innerText()).slice(0, 6500), calls: await page.evaluate(() => window.audit?.calls.slice(-12)) });
      throw error;
    } finally {
      await context.close();
    }
  });
}
async function open(page, route = '/') {
  await page.goto(origin + route);
  // Leave time for browser startup and font loading on cold Windows runners.
  await page.locator('.main-content').waitFor({ timeout: 30000 });
  await page.evaluate(() => document.fonts.ready);
}
async function record(page, draft = false) {
  await open(page, draft ? '/record?draft=100' : '/record');
  await page.waitForFunction(() => document.querySelector('.recording-badge')?.textContent === 'Recording');
}
const scrollMetrics = page => page.locator('.sheet-body').evaluate(e => ({ top: e.scrollTop, gap: e.scrollHeight - e.scrollTop - e.clientHeight }));

try {
  browser = await chromium.launch(process.env.PLAYWRIGHT_EXECUTABLE_PATH
    ? { executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH, headless: true }
    : { channel: process.env.PLAYWRIGHT_CHANNEL || 'chrome', headless: true });


  await check('Browser suite loads production assets across route changes', async page => {
    const scripts = [];
    page.on('request', request => {
      if (request.resourceType() === 'script') scripts.push(new URL(request.url()).pathname);
    });
    for (const route of ['/', '/meeting/1', '/boards', '/settings']) await open(page, route);
    assert.ok(scripts.some(path => path.startsWith('/_app/immutable/')), 'Exercise the compiled app');
    assert.deepEqual(scripts.filter(path => /\/@vite\/|\/\.svelte-kit\/|\/node_modules\/\.vite\//.test(path)), [],
      'Route changes must not depend on mutable Vite development chunks');
  });

  await runOnboardingProviderTests({ check, open, eventually, origin });
  await runMeetingPresenceTests({ check, open, eventually, origin });
  await runBoardsTests({ check, open, origin });
  await runDictationTests({ check, open, eventually, origin });

  await check('UI-02 Cancel never writes a transcript correction', async page => {
    await open(page, '/meeting/1');
    await page.getByRole('tab', { name: 'Transcript', exact: true }).click();
    await page.locator('.raw-line').click();
    await page.locator('.segment-edit-textarea').fill('Unwanted replacement.');
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await page.waitForTimeout(150);
    const state = await page.evaluate(async () => ({
      writes: window.audit.calls.filter(c => c.cmd === 'update_segment_text'),
      stored: (await window.__TAURI_INTERNALS__.invoke('get_meeting', { id: 1 })).transcript[0].text,
    }));
    assert.deepEqual(state, { writes: [], stored: 'Original transcript sentence.' });
    assert.match(await page.locator('.raw-transcript').innerText(), /Original transcript sentence/);
  });

  await check('UI-09 A rejected transcript correction remains editable and retryable', async page => {
    await open(page, '/meeting/1');
    await page.evaluate(() => window.audit.failSegment = true);
    await page.getByRole('tab', { name: 'Transcript', exact: true }).click();
    await page.locator('.raw-line').click();
    const editor = page.locator('.segment-edit-textarea');
    await editor.fill('Correction to keep.');
    await editor.press('Control+Enter');
    await page.getByRole('alert').filter({ hasText: 'Could not save this correction' }).waitFor();
    assert.equal(await editor.inputValue(), 'Correction to keep.');
    await page.evaluate(() => window.audit.failSegment = false);
    await editor.press('Control+Enter');
    await editor.waitFor({ state: 'detached' });
    assert.equal(await page.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke('get_meeting', { id: 1 })).transcript[0].text), 'Correction to keep.');
  });

  await check('UI-08 Search rejects late results and invalidates cleared queries', async page => {
    await open(page);
    await page.evaluate(() => window.audit.slowSearch = true);
    await page.getByRole('button', { name: 'Search meetings', exact: true }).click();
    const input = page.getByPlaceholder('Search titles, notes, transcripts…');
    await input.fill('alpha');
    await page.waitForFunction(() => window.audit.calls.some(c => c.cmd === 'search_meetings' && c.args.query === 'alpha'));
    await input.fill('beta');
    await eventually(() => page.locator('.search-result-title').allTextContents(), v => v.join() === 'Beta launch', 'New query result');
    await page.waitForTimeout(900);
    assert.deepEqual(await page.locator('.search-result-title').allTextContents(), ['Beta launch']);
    await input.fill('alpha');
    await page.waitForFunction(() => window.audit.calls.filter(c => c.cmd === 'search_meetings' && c.args.query === 'alpha').length === 2);
    await input.fill('');
    await page.waitForTimeout(900);
    assert.equal(await page.locator('.search-result-title').count(), 0);
  });

  await check('UI-08 Search failure is actionable and retries the current query', async page => {
    await open(page);
    await page.evaluate(() => window.audit.failSearch = true);
    await page.getByRole('button', { name: 'Search meetings', exact: true }).click();
    await page.getByPlaceholder('Search titles, notes, transcripts…').fill('beta');
    await page.getByRole('alert').filter({ hasText: 'search unavailable' }).waitFor();
    await page.evaluate(() => window.audit.failSearch = false);
    await page.getByRole('button', { name: 'Try again', exact: true }).click();
    await eventually(() => page.locator('.search-result-title').allTextContents(), v => v.join() === 'Beta launch', 'Retried result');
  });

  await check('UI-10 Provider-load failure displays an alert and working Retry', async page => {
    await open(page, '/settings');
    await page.evaluate(() => window.audit.failConfig = true);
    await page.getByRole('tab', { name: 'AI provider', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Could not load provider settings' }).waitFor();
    await page.evaluate(() => window.audit.failConfig = false);
    await page.getByRole('button', { name: 'Retry', exact: true }).click();
    await page.getByLabel('Model', { exact: true }).waitFor();
    assert.equal(await page.getByLabel('Model', { exact: true }).inputValue(), 'gpt-4o-mini');
  });

  await check('UI-01 Changed endpoint loses saved-key status and cannot silently test', async page => {
    await open(page, '/settings?section=ai');
    const key = page.getByLabel('API key', { exact: true });
    await key.waitFor();
    assert.match(await key.getAttribute('placeholder'), /Saved/);
    await page.locator('.provider-advanced summary').click();
    await page.getByLabel('Base URL', { exact: true }).fill('https://example.invalid/v1');
    assert.equal(await key.getAttribute('placeholder'), 'Enter API key');
    assert.equal(await page.getByRole('button', { name: 'Save provider', exact: true }).isDisabled(), false);
    assert.equal(await page.getByRole('button', { name: 'Test saved connection', exact: true }).isDisabled(), true);
    await page.getByRole('button', { name: 'Save provider', exact: true }).click();
    await page.getByRole('status').filter({ hasText: 'Provider saved' }).waitFor();
    assert.equal(await page.getByRole('button', { name: 'Test saved connection', exact: true }).isDisabled(), true);
    assert.equal(await key.getAttribute('placeholder'), 'Enter API key');
    assert.equal(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'test_llm_config')), false);
  });

  await check('UI-01 Reverting to the saved identity restores usable connection controls', async page => {
    await open(page, '/settings?section=ai');
    const testSaved = page.getByRole('button', { name: 'Test saved connection', exact: true });
    await testSaved.waitFor();
    assert.equal(await testSaved.isDisabled(), false);
    await page.locator('.provider-advanced summary').click();
    const endpoint = page.getByLabel('Base URL', { exact: true });
    await endpoint.fill('https://other.example.invalid/v1');
    assert.equal(await testSaved.isDisabled(), true);
    await endpoint.fill('https://api.openai.com/v1');
    assert.equal(await testSaved.isDisabled(), false);
    assert.match(await page.getByLabel('API key', { exact: true }).getAttribute('placeholder'), /Saved/);
    await page.getByLabel('Model', { exact: true }).fill('different-model');
    assert.equal(await page.getByRole('button', { name: 'Save and test', exact: true }).isDisabled(), false);
    assert.equal(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'test_llm_config')), false);
  });

  await check('UI-01 Switching to a saved endpoint discovers only its scoped key without sending a test', async page => {
    await open(page, '/settings?section=ai');
    for (const provider of ['OpenRouter', 'OpenAI']) {
      await page.locator('#provider-kind').click();
      await page.getByRole('option', { name: provider, exact: true }).click();
      const key = page.getByLabel('API key', { exact: true });
      assert.equal(await key.getAttribute('placeholder'), 'Enter API key');
      assert.equal(await page.getByRole('button', { name: 'Test saved connection', exact: true }).isDisabled(), true);
      await page.getByRole('button', { name: 'Save provider', exact: true }).click();
      await eventually(() => key.getAttribute('placeholder'), v => v.startsWith('Saved'), 'Authoritative destination key presence');
      assert.equal(await page.getByRole('button', { name: 'Test saved connection', exact: true }).isDisabled(), false);
      assert.equal(await page.evaluate(() => window.audit.calls.some(c => c.cmd === 'test_llm_config')), false);
    }
    const writes = await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'set_llm_config'));
    assert.deepEqual(writes.map(c => c.args.config.kind), ['open_router', 'open_ai']);
    assert.ok(writes.every(c => c.args.apiKey === undefined));
  });

  await check('UI-11 Invalid shortcut shows an error that clears after retry', async page => {
    await open(page, '/settings?section=shortcut');
    await page.evaluate(() => window.audit.failShortcut = true);
    await page.getByPlaceholder('Ctrl+Shift+R').fill('not-a-shortcut');
    await page.getByRole('button', { name: 'Save shortcut', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'invalid shortcut' }).waitFor();
    await page.evaluate(() => window.audit.failShortcut = false);
    await page.getByPlaceholder('Ctrl+Shift+R').fill('Ctrl+Shift+R');
    await page.getByRole('button', { name: 'Save shortcut', exact: true }).click();
    await page.getByRole('status').filter({ hasText: 'Saved' }).waitFor();
    assert.equal(await page.getByRole('alert').count(), 0);
  });

  await check('UI-12 Onboarding claims permission only, not simulated audio activity', async page => {
    await page.goto(origin + '/onboarding');
    await page.getByRole('button', { name: 'Allow microphone', exact: true }).click();
    await page.getByRole('status').filter({ hasText: 'Microphone access granted' }).waitFor();
    assert.match(await page.locator('.step').innerText(), /permission only, not microphone volume/);
    assert.equal(await page.locator('.mic-meter').count(), 0);
  });

  await check('UI-15 Manage templates selects Templates directly', async page => {
    await open(page, '/meeting/1');
    await page.getByRole('tab', { name: 'Enhance Notes', exact: true }).click();
    await page.getByRole('link', { name: 'Manage templates', exact: true }).click();
    await page.waitForURL('**/settings?section=templates');
    assert.equal(await page.locator('[role=tab][aria-selected=true]').innerText(), 'Templates');
  });

  await check('UI-16 Speech-model progress text is valid Unicode', async page => {
    await open(page, '/settings');
    await page.evaluate(() => window.audit.modelDelay = 1500);
    await page.getByRole('tab', { name: 'Speech model', exact: true }).click();
    await page.getByText('Checking the model pack…', { exact: true }).waitFor();
    assert.doesNotMatch(await page.locator('.model-status').innerText(), /â€¦/);
  });

  await check('Settings meeting switches persist and visibly reflect their state', async page => {
    await open(page, '/settings');
    const detection = page.getByRole('switch', { name: 'Meeting detection', exact: true });
    const startup = page.getByRole('switch', { name: 'Start with Windows', exact: true });
    const visualState = switchControl => switchControl.evaluate(element => ({
      dataState: element.getAttribute('data-state'),
      background: getComputedStyle(element).backgroundColor,
      thumbTranslate: getComputedStyle(element.querySelector('[data-slot="switch-thumb"]')).translate,
    }));

    const detectionOff = await visualState(detection);
    await detection.click();
    await eventually(() => detection.getAttribute('aria-checked'), value => value === 'true', 'Detection switch checked state');
    await eventually(() => visualState(detection), value =>
      value.background !== detectionOff.background
        && value.thumbTranslate !== detectionOff.thumbTranslate,
    'Detection switch track and thumb visibly change');
    await page.getByRole('button', { name: 'Pause detection', exact: true }).waitFor();

    const startupOff = await visualState(startup);
    await startup.click();
    await eventually(() => startup.getAttribute('aria-checked'), value => value === 'true', 'Startup switch checked state');
    await eventually(() => visualState(startup), value =>
      value.background !== startupOff.background
        && value.thumbTranslate !== startupOff.thumbTranslate,
    'Startup switch track and thumb visibly change');

    const writes = await page.evaluate(() => window.audit.calls.filter(call => call.cmd.startsWith('set_meeting_presence_')));
    assert.deepEqual(writes, [
      { cmd: 'set_meeting_presence_enabled', args: { enabled: true } },
      { cmd: 'set_meeting_presence_start_with_windows', args: { enabled: true } },
    ]);
  });

  await check('UI-03 Failed recovery blocks leaving and retains work until durable retry', async page => {
    await record(page);
    await page.evaluate(() => window.audit.failRecovery = true);
    await page.locator('.notepad-textarea').fill('Unsaved recovery notes.');
    await page.locator('a.nav-item[href="/"]').click();
    await page.getByRole('button', { name: 'Keep recovery copy and leave', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'recovery write rejected' }).waitFor();
    assert.equal(new URL(page.url()).pathname, '/record');
    assert.equal(await page.locator('.notepad-textarea').inputValue(), 'Unsaved recovery notes.');
    await page.evaluate(() => window.audit.failRecovery = false);
    await page.getByRole('button', { name: 'Retry saving', exact: true }).click();
    await page.waitForURL(origin + '/');
    assert.equal(await page.evaluate(() => window.audit.recovery.rawMarkdown), 'Unsaved recovery notes.');
  });

  await check('R1 New meeting here starts a fresh episode after keeping recovery and can leave again', async page => {
    await record(page);
    await page.locator('.notepad-textarea').fill('Durable previous recording.');
    await page.getByRole('button', { name: 'Actions for Personal', exact: true }).click();
    await page.getByRole('menuitem', { name: 'New meeting here', exact: true }).click();
    await page.getByRole('button', { name: 'Keep recovery copy and leave', exact: true }).click();
    await page.waitForURL('**/record?spaceId=1');
    await eventually(() => page.locator('.recording-badge').textContent(), v => v === 'Recording', 'Next recording episode starts');
    assert.equal(await page.locator('.notepad-textarea').inputValue(), '');
    assert.equal(await page.locator('.notepad-textarea').isDisabled(), false);
    const state = await page.evaluate(() => ({ recovery: window.audit.recovery, calls: window.audit.calls }));
    assert.equal(state.recovery.rawMarkdown, 'Durable previous recording.');
    assert.equal(state.calls.filter(c => c.cmd === 'start_recording').length, 2);
    assert.equal(state.calls.filter(c => c.cmd === 'stop_recording').length, 1);
    const drafts = state.calls.filter(c => c.cmd === 'create_note_draft');
    assert.equal(drafts.length, 2);
    assert.deepEqual(drafts[1].args.location, { kind: 'space', id: 1 });
    const nextStart = state.calls.findLastIndex(c => c.cmd === 'start_recording');
    assert.ok(state.calls.findLastIndex(c => c.cmd === 'update_note_draft_recovery') < nextStart);
    await page.locator('.notepad-textarea').fill('Second recording notes.');
    await page.locator('a.nav-item[href="/"]').click();
    await page.getByRole('button', { name: 'Keep recovery copy and leave', exact: true }).click();
    await page.waitForURL(origin + '/');
    assert.equal(await page.evaluate(() => window.audit.recovery.rawMarkdown), 'Second recording notes.');
    assert.notEqual(await page.evaluate(() => window.audit.recovery.id), state.recovery.id);
  });

  await check('R1 Rejected same-route recovery retains its episode until retry succeeds', async page => {
    await record(page, true);
    await page.evaluate(() => window.audit.failRecovery = true);
    await page.locator('.notepad-textarea').fill('Recovered work plus unsaved edits.');
    await page.getByRole('button', { name: 'Actions for Personal', exact: true }).click();
    await page.getByRole('menuitem', { name: 'New meeting here', exact: true }).click();
    await page.getByRole('button', { name: 'Keep recovery copy and leave', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'recovery write rejected' }).waitFor();
    assert.equal(new URL(page.url()).search, '?draft=100');
    assert.equal(await page.locator('.notepad-textarea').inputValue(), 'Recovered work plus unsaved edits.');
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'start_recording').length), 1);
    await page.evaluate(() => window.audit.failRecovery = false);
    await page.getByRole('button', { name: 'Retry saving', exact: true }).click();
    await page.waitForURL('**/record?spaceId=1');
    await eventually(() => page.locator('.recording-badge').textContent(), v => v === 'Recording', 'New episode after retry');
    assert.equal(await page.locator('.notepad-textarea').inputValue(), '');
    assert.equal(await page.evaluate(() => window.audit.recovery.id), 100);
    assert.equal(await page.evaluate(() => window.audit.recovery.rawMarkdown), 'Recovered work plus unsaved edits.');
    await page.getByRole('button', { name: 'Live transcript', exact: true }).click();
    assert.equal(await page.locator('.transcript-line').count(), 0);
    await page.evaluate(() => window.audit.emit('transcript:event', { utterance_id: 1, revision: 1, channel: 'you', text: 'Only the new episode.', is_partial: false, start_ms: 0, end_ms: 1000 }));
    await eventually(() => page.locator('.transcript-line').count(), v => v === 1, 'Fresh transcript listener');
  });

  await check('R1 Pending same-route recovery cannot remount or bypass durability', async page => {
    await record(page);
    await page.evaluate(() => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      const gate = new Promise(resolve => window.audit.releaseRecovery = resolve);
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'update_note_draft_recovery') {
          window.audit.writePending = true;
          await gate;
        }
        return invoke(cmd, args);
      };
    });
    await page.locator('.notepad-textarea').fill('Await durability before remount.');
    await page.getByRole('button', { name: 'Actions for Personal', exact: true }).click();
    await page.getByRole('menuitem', { name: 'New meeting here', exact: true }).click();
    await page.getByRole('button', { name: 'Keep recovery copy and leave', exact: true }).click();
    await page.waitForFunction(() => window.audit.writePending);
    await page.locator('a.nav-item[href="/"]').click();
    assert.equal(new URL(page.url()).search, '');
    assert.equal(await page.locator('.recording-badge').textContent(), 'Finishing');
    assert.equal(await page.evaluate(() => window.audit.calls.filter(c => c.cmd === 'start_recording').length), 1);
    await page.evaluate(() => window.audit.releaseRecovery());
    await page.waitForURL('**/record?spaceId=1');
    await eventually(() => page.locator('.recording-badge').textContent(), v => v === 'Recording', 'Durable next episode');
    assert.equal(await page.evaluate(() => window.audit.recovery.rawMarkdown), 'Await durability before remount.');
  });

  await check('UI-04 Recovered lines and new utterance 1 coexist without duplicate keys', async page => {
    await record(page, true);
    await page.getByRole('button', { name: 'Open live transcript', exact: true }).click();
    await page.evaluate(() => window.audit.emit('transcript:event', { utterance_id: 1, revision: 1, channel: 'you', text: 'First new sentence.', is_partial: true, start_ms: 0, end_ms: 1000 }));
    await eventually(() => page.locator('.transcript-line').count(), v => v === 4, 'Recovered and new line count');
    assert.match(await page.locator('.sheet-body').innerText(), /First new sentence/);
  });

  await check('UI-13 Transcript follows growth but preserves a user reading history', async page => {
    await record(page);
    await page.evaluate(() => { for (let i = 0; i < 40; i++) window.audit.emit('transcript:event', { utterance_id: i + 1, revision: 1, channel: 'you', text: `Line ${i + 1}. A complete sentence that occupies space in the transcript sheet.`, is_partial: false, start_ms: i * 2000, end_ms: i * 2000 + 1000 }); });
    await page.getByRole('button', { name: 'Open live transcript', exact: true }).click();
    await eventually(() => scrollMetrics(page), v => v.top > 0 && v.gap < 2, 'Initial latest line');
    await page.evaluate(() => window.audit.emit('transcript:event', { utterance_id: 41, revision: 1, channel: 'you', text: 'Growing partial line. '.repeat(80), is_partial: true, start_ms: 81000, end_ms: 82000 }));
    await eventually(() => scrollMetrics(page), v => v.gap < 2, 'Follow tall appended line');
    await page.evaluate(() => window.audit.emit('transcript:event', { utterance_id: 41, revision: 2, channel: 'you', text: 'Growing partial line. '.repeat(160), is_partial: true, start_ms: 81000, end_ms: 83000 }));
    await eventually(() => scrollMetrics(page), v => v.gap < 2, 'Follow same-row revision');
    await page.locator('.sheet-body').evaluate(e => { e.scrollTop = 0; e.dispatchEvent(new Event('scroll')); });
    await page.evaluate(() => window.audit.emit('transcript:event', { utterance_id: 42, revision: 1, channel: 'you', text: 'Do not pull reader to the bottom.', is_partial: false, start_ms: 84000, end_ms: 85000 }));
    await page.waitForTimeout(120);
    assert.equal((await scrollMetrics(page)).top, 0);
  });

  await check('UI-06 Compact drawer opens, closes with Escape, and navigates', async page => {
    await page.setViewportSize({ width: 420, height: 520 });
    await open(page);
    const toggle = page.locator('.sidebar-collapse-btn');
    await toggle.click();
    const drawer = page.getByRole('dialog', { name: 'Library navigation', exact: true });
    await drawer.waitFor();
    assert.ok((await drawer.boundingBox()).width > 100);
    await page.keyboard.press('Escape');
    await drawer.waitFor({ state: 'detached' });
    await toggle.click();
    await drawer.locator('a[href="/settings"]').click();
    await page.waitForURL('**/settings');
    await drawer.waitFor({ state: 'detached' });
  });

  await check('UI-07 Long library scrolls without hiding Settings', async page => {
    await open(page);
    await page.evaluate(() => { window.audit.treeCount = 50; window.dispatchEvent(new Event('focus')); });
    await eventually(() => page.locator('.library-tree').innerText(), v => v.includes('Meeting 50'), 'Expanded fixture tree');
    const geometry = await page.locator('.sidebar nav').evaluate(e => ({ client: e.clientHeight, scroll: e.scrollHeight, overflow: getComputedStyle(e).overflowY }));
    assert.ok(geometry.scroll > geometry.client);
    assert.equal(geometry.overflow, 'auto');
    await page.locator('.sidebar nav').evaluate(e => e.scrollTop = e.scrollHeight);
    assert.ok(await page.locator('.sidebar nav').evaluate(e => e.scrollTop > 0));
    const settings = await page.locator('a[href="/settings"]').boundingBox();
    assert.ok(settings.y >= 0 && settings.y + settings.height <= 800);
  });

  for (const theme of ['light', 'dark']) {
    await check(`UI-14 Sidebar passes axe contrast scan in ${theme} mode`, async page => {
      await open(page);
      if (theme === 'dark') await page.getByRole('button', { name: 'Switch to dark mode', exact: true }).click();
      await page.waitForTimeout(400);
      await page.addScriptTag({ path: require.resolve('axe-core/axe.min.js') });
      const violations = await page.evaluate(async () => (await axe.run('.sidebar', { runOnly: ['color-contrast'] })).violations);
      assert.deepEqual(violations.map(v => ({ id: v.id, nodes: v.nodes.map(n => n.failureSummary) })), []);
    });
  }
  // Keep the preview server available while the detailed drawer suite runs.
  if (!process.env.UI_TEST_FILTER) {
    const sidebar = await promisify(execFile)(process.execPath,
      ['--test', 'tests/sidebar-shell.browser.mjs'],
      { env: { ...process.env, KIMINOLA_BASE_URL: origin, KIMINOLA_BROWSER_CHANNEL: process.env.PLAYWRIGHT_CHANNEL || 'chrome' }, maxBuffer: 1024 * 1024 });
    console.log(sidebar.stdout);
    if (sidebar.stderr) console.error(sidebar.stderr);
  }
} finally {
  await browser?.close();
  await server.close();
}
