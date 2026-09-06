// Run after npm run build. Uses Playwright from the environment or from the
// module file specified by KIMINOLA_PLAYWRIGHT_MODULE. No real recording/data.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

const { chromium } = await import(process.env.KIMINOLA_PLAYWRIGHT_MODULE
  ? pathToFileURL(process.env.KIMINOLA_PLAYWRIGHT_MODULE).href : "playwright");
const build = fileURLToPath(new URL("../build/", import.meta.url));
const types = { ".js": "text/javascript", ".css": "text/css", ".html": "text/html", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".json": "application/json" };
const server = createServer(async (request, response) => {
  try {
    const pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const file = path.resolve(build, `.${pathname}`);
    if (file !== path.resolve(build) && !file.startsWith(path.resolve(build) + path.sep)) { response.writeHead(403).end(); return; }
    let content;
    let extension = path.extname(file);
    try { content = await readFile(file); }
    catch { content = await readFile(path.join(build, "index.html")); extension = ".html"; }
    response.setHeader("Content-Type", types[extension] ?? "application/octet-stream");
    response.end(content);
  } catch (error) { response.writeHead(500).end(String(error)); }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  browser = await chromium.launch({ headless: true, channel: process.env.KIMINOLA_BROWSER_CHANNEL ?? (process.platform === "win32" ? "msedge" : undefined) });
  const page = await browser.newPage({ viewport: { width: 1200, height: 800 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.addInitScript(() => {
    let callbackId = 0;
    const callbacks = new Map();
    const listeners = new Map();
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback(fn) { callbacks.set(++callbackId, fn); return callbackId; },
      async invoke(command, args) {
        if (command === "plugin:event|listen") { listeners.set(args.event, args.handler); return args.handler; }
        if (command === "plugin:event|unlisten") return;
        if (command === "is_onboarding_complete") return true;
        if (command === "database_status") return { ready: true, backups: [] };
        if (["list_meetings", "list_note_drafts", "list_library_tree"].includes(command)) return [];
        if (command === "get_meeting_presence_state") return { enabled: false, paused: false, start_with_windows: false, mode: "disabled", hint: null, prompt: null };
        if (command === "get_note_draft") return {
          id: 1, title: "Resize test notes", created_at: "2026-09-06T12:00:00Z", updated_at: "2026-09-06T12:00:00Z",
          raw_markdown: Array.from({ length: 100 }, (_, i) => `Line ${i}: notes stay editable during resizing.`).join("\n"),
          meeting_id: null, recovery_duration_seconds: 0, recovery_transcript: [], recovery_location: null,
        };
        if (command === "create_note_draft") return 1;
        if (["start_recording", "resume_recording"].includes(command)) return { meeting_audio_available: true, transcription_available: true };
        if (["update_note_draft", "update_note_draft_recovery"].includes(command)) return;
        throw new Error(`Unmocked native command: ${command}`);
      },
    };
    window.emitTestTranscript = () => {
      const handler = callbacks.get(listeners.get("transcript:event"));
      handler?.({ event: "transcript:event", payload: { utterance_id: 1, revision: 1, channel: "you", text: "Resize test transcript", is_partial: false, start_ms: 0, end_ms: 500 } });
    };
  });

  const origin = `http://127.0.0.1:${server.address().port}`;
  const settleFrame = () => page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  for (const route of ["/", "/note/1", "/record"]) {
    await page.setViewportSize({ width: 1200, height: 800 });
    await page.goto(`${origin}${route}`);
    await page.locator(".sidebar-collapse-btn").waitFor();
    await page.waitForFunction(() => document.documentElement.dataset.sidebarCollapsed !== undefined);
    if (route === "/record") {
      await page.waitForFunction(() => document.querySelector(".recording-badge")?.textContent?.includes("Recording"));
      await page.evaluate(() => window.emitTestTranscript());
    }
    const editor = page.locator("textarea").first();
    const hasEditor = route !== "/";
    let selection;
    if (hasEditor) {
      await editor.waitFor();
      if (route === "/record") await editor.fill("Recording notes\n".repeat(100));
      selection = await editor.evaluate((element) => {
        element.focus(); element.setSelectionRange(2, 8); element.scrollTop = 150;
        return { start: element.selectionStart, end: element.selectionEnd, scroll: element.scrollTop, value: element.value };
      });
    }
    for (const width of [1200, 761, 760, 759, 420, 760, 761, 1200]) {
      await page.setViewportSize({ width, height: 800 });
      await settleFrame();
      const state = await page.evaluate(() => {
        const main = document.querySelector(".main");
        const sidebar = document.querySelector(".sidebar");
        const animatedGeometry = document.getAnimations().filter((animation) =>
          ["width", "padding-left", "padding-right", "margin-left", "left"].includes(animation.transitionProperty)).length;
        return { margin: parseFloat(getComputedStyle(main).marginLeft), sidebar: sidebar.getBoundingClientRect().width,
          duration: getComputedStyle(main).transitionDuration, overflow: document.documentElement.scrollWidth > innerWidth,
          animatedGeometry, saved: localStorage.getItem("kiminola-sidebar-collapsed") };
      });
      const expected = width <= 760 ? 0 : 240;
      assert.equal(state.margin, expected, `${route} at ${width}: content offset`);
      assert.equal(state.sidebar, expected, `${route} at ${width}: sidebar width`);
      assert.equal(state.duration, "0s", `${route} at ${width}: resize should not animate geometry`);
      assert.equal(state.animatedGeometry, 0, `${route} at ${width}: trailing geometry animation`);
      assert.equal(state.overflow, false, `${route} at ${width}: horizontal overflow`);
      assert.equal(state.saved, null, "viewport resizing must not save sidebar state");
      if (hasEditor) {
        const actual = await editor.evaluate((element) => ({ start: element.selectionStart, end: element.selectionEnd,
          scroll: element.scrollTop, value: element.value, focused: document.activeElement === element }));
        assert.deepEqual(actual, { ...selection, focused: true }, `${route} at ${width}: editor state changed`);
      }
    }
    console.log(`PASS ${route}: eight widths, breakpoint reversals, no geometry lag/overflow${hasEditor ? ", editor focus/selection/scroll preserved" : ""}`);
  }

  await page.locator(".sidebar-collapse-btn").evaluate((button) => button.click());
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector(".main")).transitionDuration), "0.2s");
  await page.setViewportSize({ width: 1100, height: 800 });
  await settleFrame();
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector(".main")).transitionDuration), "0s");
  assert.equal(await page.evaluate(() => localStorage.getItem("kiminola-sidebar-collapsed")), "true");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.locator(".sidebar-collapse-btn").evaluate((button) => button.click());
  await settleFrame();
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector(".main")).transitionDuration), "0s");
  assert.equal(await page.evaluate(() => parseFloat(getComputedStyle(document.querySelector(".main")).marginLeft)), 240);
  console.log("PASS sidebar toggle: 200 ms motion, resize cancellation, saved preference, reduced motion");
  assert.deepEqual(errors, [], "browser runtime errors");
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
