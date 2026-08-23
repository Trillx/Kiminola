import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { compactReleaseNotes, isRecordingPath, updateProgress } from "../src/lib/update-policy.ts";

test("updates are blocked only on the active recording route", () => {
  assert.equal(isRecordingPath("/record"), true);
  assert.equal(isRecordingPath("/"), false);
  assert.equal(isRecordingPath("/settings"), false);
});

test("download progress stays bounded and handles unknown lengths", () => {
  assert.equal(updateProgress(0, 100), 0);
  assert.equal(updateProgress(25, 100), 25);
  assert.equal(updateProgress(125, 100), 100);
  assert.equal(updateProgress(25, null), 0);
});

test("release notes are normalized and compacted for the update banner", () => {
  assert.equal(compactReleaseNotes("  Fix\n\n  audio   startup. "), "Fix audio startup.");
  assert.equal(compactReleaseNotes("abcdefgh", 5), "ab...");
  assert.equal(compactReleaseNotes(null), "");
});
