// Run after npm run build. Native calls use fixtures, never the user's database.
import assert from "node:assert/strict";
import { readFile, mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

const { chromium } = await import(process.env.KIMINOLA_PLAYWRIGHT_MODULE
  ? pathToFileURL(process.env.KIMINOLA_PLAYWRIGHT_MODULE).href : "playwright");
const build = fileURLToPath(new URL("../build/", import.meta.url));
const types = { ".js": "text/javascript", ".css": "text/css", ".html": "text/html", ".svg": "image/svg+xml", ".woff2": "font/woff2" };
const server = createServer(async (request, response) => {
  try {
    const pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const file = path.resolve(build, `.${pathname}`);
    if (file !== path.resolve(build) && !file.startsWith(path.resolve(build) + path.sep)) { response.writeHead(403).end(); return; }
    let content, extension = path.extname(file);
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
    const meeting = (id, title, children = []) => ({ kind: "meeting", id, title, children, created_at: "2026-09-06T12:00:00Z", duration_seconds: 60 });
    const tree = [{ kind: "space", id: 1, name: "Projects", children: [
      { kind: "space", id: 2, name: "Design", children: [meeting(10, "Weekly review", [meeting(11, "Follow-up")])] },
      { kind: "space", id: 3, name: "Empty folder", children: [] },
      meeting(12, "Project kickoff"),
    ] }, { kind: "space", id: 4, name: "Personal", children: [] }];
    let callbackId = 0;
    window.sidebarMoves = [];
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback() { return ++callbackId; },
      async invoke(command, args) {
        if (command === "plugin:event|listen") return ++callbackId;
        if (command === "plugin:event|unlisten") return;
        if (command === "is_onboarding_complete") return true;
        if (command === "database_status") return { ready: true, backups: [] };
        if (["list_meetings", "list_note_drafts"].includes(command)) return [];
        if (command === "list_library_tree") return structuredClone(tree);
        if (command === "move_library_node") {
          // Native IPC serializes reactive values before crossing the boundary.
          window.sidebarMoves.push(JSON.parse(JSON.stringify(args)));
          const find = (nodes, ref) => {
            for (const node of nodes) {
              if (node.kind === ref.kind && node.id === ref.id) return { node, siblings: nodes };
              const found = find(node.children, ref);
              if (found) return found;
            }
          };
          const source = find(tree, args.node);
          const destination = find(tree, args.destination);
          source.siblings.splice(source.siblings.indexOf(source.node), 1);
          destination.node.children.push(source.node);
          return;
        }
        if (command === "get_meeting_presence_state") return { enabled: false, paused: false, start_with_windows: false, mode: "disabled", hint: null, prompt: null };
        if (command === "get_meeting") return { ...meeting(args.id, "Follow-up"), notepad: "", enhanced_markdown: null, transcript: [], space_name: "Design", location_path: "Projects / Design", parent_meeting_id: 10 };
        throw new Error(`Unmocked native command: ${command}`);
      },
    };
  });
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  const projects = page.getByRole("button", { name: "Projects", exact: true });
  const design = page.getByRole("button", { name: "Design", exact: true });
  await design.waitFor();
  // Enter on a nested folder must affect that folder only. The original nested
  // context triggers also handled the same bubbling event on every ancestor.
  await design.focus();
  await design.press("Enter");
  assert.equal(await projects.getAttribute("aria-expanded"), "true", "Enter on Design must not collapse Projects");
  assert.equal(await design.getAttribute("aria-expanded"), "false", "Enter must collapse Design once");
  console.log("PASS nested keyboard toggle affects only the target folder");
  await design.click();
  await page.getByRole("link", { name: "Follow-up", exact: true }).waitFor();
  const meetingToggle = page.getByRole("button", { name: "Collapse Weekly review", exact: true });
  assert.equal(await meetingToggle.count(), 1, "Parent meetings need their own disclosure control");
  await meetingToggle.click();
  await page.getByRole("link", { name: "Follow-up", exact: true }).waitFor({ state: "hidden" });
  await page.getByRole("button", { name: "Expand Weekly review", exact: true }).click();
  await page.getByRole("link", { name: "Follow-up", exact: true }).waitFor();
  console.log("PASS parent meetings expand and collapse independently of navigation");

  const settle = () => page.waitForFunction(() => document.getAnimations().length === 0);
  await settle();
  const arrow = design.locator(".library-disclosure svg");
  assert.equal(await arrow.evaluate((el) => getComputedStyle(el).transform), "matrix(0, 1, -1, 0, 0, 0)", "Expanded arrow points down");
  const motion = await design.evaluate(async (el) => {
    const branch = el.closest(".library-node").querySelector(".library-node-children");
    const before = branch.getBoundingClientRect().height;
    el.click();
    await new Promise((resolve) => setTimeout(resolve, 70));
    return { before, during: branch.getBoundingClientRect().height, inert: branch.inert };
  });
  assert.ok(motion.during > 0 && motion.during < motion.before, "Branch height animates instead of disappearing instantly");
  assert.equal(motion.inert, true, "Closing children cannot receive focus");
  await settle();
  assert.equal(await arrow.evaluate((el) => getComputedStyle(el).transform), "none", "Collapsed arrow points right");
  await page.reload();
  await design.waitFor();
  assert.equal(await design.getAttribute("aria-expanded"), "false", "Folder state survives reload");
  await design.evaluate(async (el) => {
    for (let i = 0; i < 5; i++) { el.click(); await new Promise((resolve) => setTimeout(resolve, 35)); }
  });
  await settle();
  assert.equal(await design.getAttribute("aria-expanded"), "true");
  await page.getByRole("link", { name: "Follow-up", exact: true }).waitFor();
  console.log("PASS arrow direction, animated branch height, rapid reversal, focus safety, saved state");

  await design.focus();
  await design.press("ArrowRight");
  assert.equal((await page.locator(":focus").textContent()).trim(), "Weekly review");
  await page.locator(":focus").press("ArrowLeft");
  assert.equal(await page.getByRole("button", { name: "Expand Weekly review", exact: true }).count(), 1);
  await page.locator(":focus").press("ArrowLeft");
  assert.equal((await page.locator(":focus").textContent()).trim(), "Design");
  await page.getByRole("button", { name: "Expand Weekly review", exact: true }).click();
  await settle();
  // A child context menu must not open an ancestor's menu.
  await page.getByRole("link", { name: "Follow-up", exact: true }).click({ button: "right" });
  assert.equal(await page.getByRole("menuitem", { name: "New child meeting", exact: true }).count(), 1);
  await page.keyboard.press("Escape");
  await design.focus();
  await design.press("Space");
  assert.equal(await projects.getAttribute("aria-expanded"), "true");
  assert.equal(await design.getAttribute("aria-expanded"), "false");
  await settle();
  await page.goto(`http://127.0.0.1:${server.address().port}/meeting/11`);
  await page.getByRole("link", { name: "Follow-up", exact: true }).waitFor();
  assert.equal(await design.getAttribute("aria-expanded"), "true", "Navigating to a nested meeting reveals ancestors");
  assert.equal(await page.getByRole("link", { name: "Follow-up", exact: true }).getAttribute("aria-current"), "page");
  console.log("PASS arrow keys, Space, child context menu, reveal active meeting");

  await page.emulateMedia({ reducedMotion: "reduce" });
  await design.click();
  await page.getByRole("link", { name: "Follow-up", exact: true }).waitFor({ state: "hidden" });
  assert.equal(await page.evaluate(() => document.getAnimations().length), 0, "Reduced motion has no animations");
  await design.click();
  await page.getByRole("link", { name: "Follow-up", exact: true }).waitFor();
  const alignment = await page.evaluate(() => {
    const rows = [...document.querySelectorAll(".library-node-row")];
    const rects = rows.map((row) => ({ name: row.querySelector(".library-node-label").textContent,
      x: row.querySelector(".library-node-icon").getBoundingClientRect().x }));
    return rects;
  });
  const x = (name) => alignment.find((row) => row.name === name).x;
  assert.equal(x("Design"), x("Project kickoff"), "Space and meeting icons align at the same depth");
  assert.equal(x("Weekly review") - x("Design"), 18, "Every level uses consistent indentation");
  assert.equal(await page.locator(".sidebar").evaluate((el) => el.scrollWidth <= el.clientWidth), true, "Sidebar has no horizontal overflow");
  console.log("PASS reduced motion and consistent parent/child alignment");
  if (process.env.KIMINOLA_SCREENSHOT_DIR) {
    await mkdir(process.env.KIMINOLA_SCREENSHOT_DIR, { recursive: true });
    await page.mouse.move(600, 100);
    await settle();
    await page.locator(".sidebar").screenshot({ path: path.join(process.env.KIMINOLA_SCREENSHOT_DIR, "sidebar-light.png") });
    await page.evaluate(() => localStorage.setItem("kiminola-theme", "dark"));
    await page.reload();
    await page.getByRole("link", { name: "Follow-up", exact: true }).waitFor();
    await settle();
    await page.locator(".sidebar").screenshot({ path: path.join(process.env.KIMINOLA_SCREENSHOT_DIR, "sidebar-dark.png") });
  }
  const transfer = await page.evaluateHandle(() => new DataTransfer());
  const projectRow = projects.locator("..");
  const designRow = design.locator("..");
  await projectRow.dispatchEvent("dragstart", { dataTransfer: transfer });
  await designRow.dispatchEvent("dragover", { dataTransfer: transfer });
  await designRow.dispatchEvent("drop", { dataTransfer: transfer });
  assert.deepEqual(await page.evaluate(() => window.sidebarMoves), [], "Cannot drop an ancestor into a descendant");
  await projectRow.dispatchEvent("dragend", { dataTransfer: transfer });
  const personal = page.getByRole("button", { name: "Personal", exact: true });
  await personal.click();
  await designRow.dispatchEvent("dragstart", { dataTransfer: transfer });
  await personal.locator("..").dispatchEvent("dragover", { dataTransfer: transfer });
  await personal.locator("..").dispatchEvent("drop", { dataTransfer: transfer });
  await page.waitForFunction(() => window.sidebarMoves.length === 1);
  await page.waitForFunction(() => document.querySelector("#library-children-space-4 #library-children-space-2"));
  assert.equal(await personal.getAttribute("aria-expanded"), "true", "Dropping into a closed folder reveals the moved branch");
  assert.deepEqual(await page.evaluate(() => window.sidebarMoves), [{ node: { kind: "space", id: 2 }, destination: { kind: "space", id: 4 } }]);
  assert.equal(await page.getByRole("link", { name: "Follow-up", exact: true }).count(), 1, "Move preserves descendants without duplication");
  console.log("PASS drag target ownership, cycle prevention, and revealing moved branches");
  assert.deepEqual(errors, [], "browser runtime errors");
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
