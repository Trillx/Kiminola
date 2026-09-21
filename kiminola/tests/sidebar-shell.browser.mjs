// Isolated browser regression: run against Vite with synthetic IPC only.
// KIMINOLA_BASE_URL defaults to http://127.0.0.1:1420; Playwright can be supplied
// through KIMINOLA_PLAYWRIGHT_MODULE without adding a runtime dependency.
import assert from "node:assert/strict";
import { test } from "node:test";
import { pathToFileURL } from "node:url";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import axe from "axe-core";
const { chromium } = await import(process.env.KIMINOLA_PLAYWRIGHT_MODULE
  ? pathToFileURL(process.env.KIMINOLA_PLAYWRIGHT_MODULE).href : "playwright");

async function fixturePage(viewport, run) {
  const browser = await chromium.launch({ headless: true, channel: process.env.KIMINOLA_BROWSER_CHANNEL ?? "chrome" });
  try {
    const page = await browser.newPage({ viewport });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.addInitScript(() => {
      let serial = 0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
        transformCallback() { return ++serial; },
        unregisterCallback() {},
        async invoke(command, args = {}) {
          if (command === "database_status") return { ready: true, backups: [] };
          if (command === "is_onboarding_complete") return true;
          if (command === "plugin:event|listen") return ++serial;
          if (command === "plugin:event|unlisten") return;
          if (command === "plugin:app|version") return "0.1.4";
          if (["list_meetings", "list_note_drafts", "search_meetings"].includes(command)) return [];
          if (command === "list_library_tree") return [{ kind: "space", id: 1, name: "Personal", children:
            Array.from({ length: 50 }, (_, i) => ({ kind: "meeting", id: i + 1, title: `Meeting ${i + 1}`, children: [], created_at: "2026-09-21T12:00:00Z", duration_seconds: 60 })) }];
          if (command === "get_meeting") return { id: args.id, title: `Meeting ${args.id}`, created_at: "2026-09-21T12:00:00Z", duration_seconds: 60, notepad: "Synthetic notes", transcript: [], enhanced_markdown: null, space_name: "Personal", location_path: "Personal", parent_meeting_id: null };
          if (command === "get_meeting_presence_state") return { enabled: false, paused: false, start_with_windows: false, mode: "off", hint: null, prompt: null };
          if (command === "get_global_shortcut") return "Ctrl+Shift+R";
          if (command === "check_model_pack") return true;
          throw new Error(`Unmocked native command: ${command}`);
        },
      };
    });
    await page.goto(process.env.KIMINOLA_BASE_URL ?? "http://127.0.0.1:1420");
    await page.locator(".main-content").waitFor();
    try { await run(page); }
    catch (error) { console.error({ url: page.url(), errors, body: (await page.locator("body").innerText()).slice(0, 1500) }); throw error; }
    assert.deepEqual(errors, [], "no browser runtime errors");
  } finally { await browser.close(); }
}
const settle = (page) => page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));

test("sidebar text and drawer semantics pass axe in light and dark themes", async () => {
  for (const viewport of [{ width: 1200, height: 800 }, { width: 420, height: 520 }]) {
    await fixturePage(viewport, async (page) => {
      for (const theme of ["light", "dark"]) {
        await page.evaluate((theme) => localStorage.setItem("kiminola-theme", theme), theme);
        await page.reload();
        await page.locator(".main-content").waitFor();
        if (viewport.width <= 760) await page.locator(".sidebar-collapse-btn").click();
        const selector = viewport.width <= 760 ? ".sidebar-drawer" : "#desktop-sidebar";
        await page.locator(selector).getByRole("link", { name: "Meeting 1", exact: true }).waitFor();
        await page.addScriptTag({ content: axe.source });
        const violations = await page.evaluate(async (selector) => {
          const results = await window.axe.run(selector, { runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] } });
          return results.violations.map(({ id, nodes }) => ({ id, nodes: nodes.map(({ target, failureSummary }) => ({ target, failureSummary })) }));
        }, selector);
        assert.deepEqual(violations, [], `${theme} ${viewport.width}px sidebar axe violations`);
        if (process.env.KIMINOLA_SCREENSHOT_DIR) {
          await mkdir(process.env.KIMINOLA_SCREENSHOT_DIR, { recursive: true });
          await page.screenshot({ path: path.join(process.env.KIMINOLA_SCREENSHOT_DIR, `sidebar-fixed-${theme}-${viewport.width}.png`) });
        }
      }
    });
  }
});

test("long libraries scroll while Settings stays inside the viewport", async () => {
  for (const viewport of [{ width: 1200, height: 800 }, { width: 420, height: 520 }]) {
    await fixturePage(viewport, async (page) => {
      if (viewport.width <= 760) await page.locator(".sidebar-collapse-btn").click();
      const sidebar = page.locator(viewport.width <= 760 ? ".sidebar-drawer" : "#desktop-sidebar");
      await sidebar.getByRole("link", { name: "Meeting 50", exact: true }).waitFor();
      const footer = await sidebar.locator(".sidebar-bottom").boundingBox();
      assert.ok(footer && footer.y >= 0 && footer.y + footer.height <= viewport.height, "Settings must not be clipped below the window");
      const nav = sidebar.getByRole("navigation", { name: "Library", exact: true });
      await nav.hover();
      await page.mouse.wheel(0, 2500);
      await page.waitForFunction(() => [...document.querySelectorAll('.sidebar nav')].some((el) => el.scrollTop > 0));
      const last = await sidebar.getByRole("link", { name: "Meeting 50", exact: true }).boundingBox();
      assert.ok(last && last.y + last.height <= footer.y, "last meeting is reachable without covering Settings");
      const after = await sidebar.locator(".sidebar-bottom").boundingBox();
      assert.deepEqual(after, footer, "footer does not move with library scrolling");
    });
  }
});

test("drawer search remains usable above navigation and Escape closes one layer", async () => {
  await fixturePage({ width: 420, height: 520 }, async (page) => {
    await page.locator(".sidebar-collapse-btn").click();
    const drawer = page.getByRole("dialog", { name: "Library navigation", exact: true });
    await drawer.getByRole("button", { name: "Search meetings", exact: true }).click();
    const search = page.getByPlaceholder("Search titles, notes, transcripts…");
    await search.waitFor();
    assert.equal(await search.evaluate((el) => {
      const bounds = el.getBoundingClientRect();
      return document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2) === el;
    }), true, "search input must not be covered by drawer");
    await search.fill("synthetic");
    await page.keyboard.press("Escape");
    await search.waitFor({ state: "hidden" });
    assert.equal(await drawer.isVisible(), true, "Escape dismisses search before its parent drawer");
    await page.keyboard.press("Escape");
    await drawer.waitFor({ state: "hidden" });
  });
});

test("desktop collapse persists independently through compact resize and drawer use", async () => {
  await fixturePage({ width: 1200, height: 800 }, async (page) => {
    await page.getByRole("button", { name: "Collapse sidebar", exact: true }).click();
    await page.waitForFunction(() => !document.documentElement.dataset.sidebarMotion);
    assert.equal(await page.evaluate(() => localStorage.getItem("kiminola-sidebar-collapsed")), "true");
    for (const width of [760, 420, 761, 1200]) {
      await page.setViewportSize({ width, height: 800 });
      await settle(page);
      assert.equal(await page.locator(".main").evaluate((el) => getComputedStyle(el).marginLeft), "0px");
      assert.equal(await page.locator(".main").evaluate((el) => getComputedStyle(el).transitionDuration), "0s");
      if (width <= 760 && !(await page.locator(".sidebar-drawer").isVisible())) await page.getByRole("button", { name: "Open sidebar", exact: true }).click();
    }
    assert.equal(await page.getByRole("dialog", { name: "Library navigation", exact: true }).count(), 0);
    assert.equal(await page.evaluate(() => localStorage.getItem("kiminola-sidebar-collapsed")), "true");
    await page.reload();
    await page.getByRole("button", { name: "Expand sidebar", exact: true }).waitFor();
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.getByRole("button", { name: "Expand sidebar", exact: true }).click();
    assert.equal(await page.locator(".main").evaluate((el) => getComputedStyle(el).marginLeft), "240px");
    assert.equal(await page.locator(".main").evaluate((el) => getComputedStyle(el).transitionDuration), "0s");
    for (const width of [761, 760, 420, 760, 761, 1200]) {
      await page.setViewportSize({ width, height: 800 });
      await settle(page);
      assert.equal(await page.locator(".main").evaluate((el) => getComputedStyle(el).marginLeft), width <= 760 ? "0px" : "240px");
      assert.equal(await page.locator(".main").evaluate((el) => getComputedStyle(el).transitionDuration), "0s");
    }
  });
});

test("compact navigation opens as a modal drawer without changing content geometry", async () => {
  await fixturePage({ width: 420, height: 520 }, async (page) => {
    await page.locator(".sidebar-collapse-btn").click();
    assert.equal(await page.getByRole("link", { name: "Settings", exact: true }).isVisible(), true, "compact toggle must reveal Settings");
    const drawer = page.getByRole("dialog", { name: "Library navigation", exact: true });
    await drawer.waitFor();
    assert.equal(await drawer.getAttribute("aria-modal"), "true");
    assert.equal(await page.locator(".sidebar-open-btn").isVisible(), false, "opener must not float above the modal navigation");
    assert.equal(await page.locator(".main").evaluate((el) => getComputedStyle(el).marginLeft), "0px");
    assert.equal(await page.evaluate(() => localStorage.getItem("kiminola-sidebar-collapsed")), null);
    assert.equal(await drawer.evaluate((el) => el.contains(document.activeElement)), true);
    await page.getByRole("button", { name: "Close sidebar", exact: true }).focus();
    await page.keyboard.press("Shift+Tab");
    assert.equal(await drawer.evaluate((el) => el.contains(document.activeElement)), true, "focus stays inside drawer");
    await page.keyboard.press("Escape");
    await drawer.waitFor({ state: "hidden" });
    assert.equal(await page.locator(":focus").getAttribute("aria-label"), "Open sidebar", "Escape restores opener focus");
    await page.getByRole("button", { name: "Open sidebar", exact: true }).click();
    await page.getByRole("button", { name: "Close sidebar", exact: true }).click();
    await drawer.waitFor({ state: "hidden" });
    await page.getByRole("button", { name: "Open sidebar", exact: true }).click();
    await page.getByRole("link", { name: "Meeting 1", exact: true }).click();
    await page.waitForURL("**/meeting/1");
    await drawer.waitFor({ state: "hidden" });
    await page.getByRole("button", { name: "Open sidebar", exact: true }).click();
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.waitForURL("**/settings");
    await drawer.waitFor({ state: "hidden" });
  });
});
