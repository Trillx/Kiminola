import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { canStopRecording, recordingPhaseLabel, shouldAdvanceElapsed, shouldDiscardAutoDraft } from "../src/lib/recording-ui-state.ts";

test("only an active recording advances elapsed time", () => {
  assert.equal(shouldAdvanceElapsed("starting"), false);
  assert.equal(shouldAdvanceElapsed("failed"), false);
  assert.equal(shouldAdvanceElapsed("paused"), false);
  assert.equal(shouldAdvanceElapsed("stopping"), false);
  assert.equal(shouldAdvanceElapsed("recording"), true);
});

test("recording controls stay unavailable until startup succeeds", () => {
  assert.equal(canStopRecording("starting"), false);
  assert.equal(canStopRecording("failed"), false);
  assert.equal(canStopRecording("recording"), true);
  assert.equal(canStopRecording("paused"), true);
});

test("failure and transition labels never claim the app is recording", () => {
  assert.equal(recordingPhaseLabel("starting"), "Starting");
  assert.equal(recordingPhaseLabel("failed"), "Couldn't start");
  assert.equal(recordingPhaseLabel("stopping"), "Finishing");
});

test("leaving after startup failure preserves the recovery draft", () => {
  assert.equal(shouldDiscardAutoDraft(true, false), false);
  assert.equal(shouldDiscardAutoDraft(true, true), true);
  assert.equal(shouldDiscardAutoDraft(false, true), false);
});
