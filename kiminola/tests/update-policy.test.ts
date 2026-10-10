import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { compactReleaseNotes, updateProgress } from "../src/lib/update-policy.ts";

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
