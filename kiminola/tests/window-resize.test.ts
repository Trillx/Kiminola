import { readFileSync } from "node:fs";
import { test } from "node:test";
import assert from "node:assert/strict";

const layoutSource = readFileSync(new URL("../src/routes/+layout.svelte", import.meta.url), "utf8");
const cssSource = readFileSync(new URL("../src/app.css", import.meta.url), "utf8");

test("companion layout suppresses breakpoint transition lag", () => {
  assert.match(layoutSource, /compact-window-resizing/);
  assert.match(cssSource, /transition:\s*none\s*!important/);

  for (const selector of [
    ".sidebar",
    ".main",
    ".sidebar-collapse-btn",
    ".bottom-bar",
    ".ai-disclaimer",
    ".transcript-indicator",
    ".transcript-strip",
    ".transcript-sheet",
  ]) {
    const escapedSelector = selector.replace(".", "\\.");
    assert.match(
      cssSource,
      new RegExp(`html\\[data-compact-window-resizing="true"\\] ${escapedSelector}`),
    );
  }

  assert.doesNotMatch(cssSource, /transition: left 200ms ease, width 200ms ease/);
});
