import { readFileSync } from "node:fs";
import { test } from "node:test";
import assert from "node:assert/strict";
// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { setupCompactWindowSync, type CompactWindowMedia, type CompactWindowState } from "../src/lib/compact-window.ts";

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

  let matches = false;
  let listener: (() => void) | undefined;
  const media: CompactWindowMedia = {
    get matches() {
      return matches;
    },
    addEventListener(type, nextListener) {
      assert.equal(type, "change");
      listener = nextListener;
    },
    removeEventListener(type, nextListener) {
      assert.equal(type, "change");
      if (listener === nextListener) listener = undefined;
    },
  };
  const callbacks = new Map<number, () => void>();
  let nextFrame = 1;
  const states: CompactWindowState[] = [];
  const stop = setupCompactWindowSync({
    media,
    initialCompactWindow: false,
    requestFrame: (callback) => {
      const handle = nextFrame++;
      callbacks.set(handle, callback);
      return handle;
    },
    cancelFrame: (handle) => callbacks.delete(handle),
    onStateChange: (state) => states.push(state),
  });

  assert.deepEqual(states, []);
  matches = true;
  listener?.();
  assert.deepEqual(states, [{ compactWindow: true, compactWindowResizing: true }]);
  const lastState = (): CompactWindowState | undefined => states[states.length - 1];

  const runNextFrame = () => {
    const handle = callbacks.keys().next().value as number | undefined;
    if (handle === undefined) return;
    const callback = callbacks.get(handle);
    callbacks.delete(handle);
    callback?.();
  };
  runNextFrame();
  assert.equal(lastState()?.compactWindowResizing, true);
  runNextFrame();
  assert.deepEqual(lastState(), { compactWindow: true, compactWindowResizing: false });

  matches = false;
  listener?.();
  matches = true;
  listener?.();
  runNextFrame();
  runNextFrame();
  assert.deepEqual(lastState(), { compactWindow: true, compactWindowResizing: false });

  stop();
  matches = false;
  listener?.();
  assert.deepEqual(lastState(), { compactWindow: true, compactWindowResizing: false });
});
