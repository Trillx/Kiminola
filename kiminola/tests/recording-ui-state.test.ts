import assert from "node:assert/strict";
import { test } from "node:test";

// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { activateElapsedClock, canHandleStopShortcut, canPauseRecording, canResumeRecording, canRetryFinish, canStopRecording, createElapsedClock, elapsedClockSeconds, freezeElapsedClock, recordingPhaseLabel, shouldAdvanceElapsed, shouldDiscardAutoDraft, shouldGuardRecordingNavigation } from "../src/lib/recording-ui-state.ts";

test("the elapsed clock catches up after delayed callbacks and excludes pauses", () => {
  let clock = createElapsedClock(120);
  clock = activateElapsedClock(clock, 1_000);
  assert.equal(elapsedClockSeconds(clock, 4_900), 123);

  clock = freezeElapsedClock(clock, 5_500);
  assert.equal(elapsedClockSeconds(clock, 50_000), 124);

  clock = activateElapsedClock(clock, 60_000);
  assert.equal(elapsedClockSeconds(clock, 62_900), 127);
  clock = freezeElapsedClock(clock, 63_500);
  assert.equal(elapsedClockSeconds(clock, 90_000), 128);
});

test("only an active recording advances elapsed time", () => {
  assert.equal(shouldAdvanceElapsed("starting"), false);
  assert.equal(shouldAdvanceElapsed("failed"), false);
  assert.equal(shouldAdvanceElapsed("paused"), false);
  assert.equal(shouldAdvanceElapsed("stopping"), false);
  assert.equal(shouldAdvanceElapsed("finish_failed"), false);
  assert.equal(shouldAdvanceElapsed("recording"), true);
});

test("recording controls stay unavailable until startup succeeds", () => {
  assert.equal(canStopRecording("starting"), false);
  assert.equal(canStopRecording("failed"), false);
  assert.equal(canStopRecording("recording"), true);
  assert.equal(canStopRecording("paused"), true);
});

test("pause and resume controls cannot race an in-flight command", () => {
  assert.equal(canPauseRecording("recording", false), true);
  assert.equal(canPauseRecording("recording", true), false);
  assert.equal(canPauseRecording("paused", false), false);
  assert.equal(canResumeRecording("paused", false), true);
  assert.equal(canResumeRecording("paused", true), false);
  assert.equal(canResumeRecording("recording", false), false);
});

test("failure and transition labels never claim the app is recording", () => {
  assert.equal(recordingPhaseLabel("starting"), "Starting");
  assert.equal(recordingPhaseLabel("failed"), "Couldn't start");
  assert.equal(recordingPhaseLabel("stopping"), "Finishing");
  assert.equal(recordingPhaseLabel("finish_failed"), "Save failed");
});

test("only a failed finish exposes the save retry", () => {
  assert.equal(canRetryFinish("finish_failed"), true);
  assert.equal(canRetryFinish("failed"), false);
  assert.equal(canRetryFinish("stopping"), false);
});

test("the stop shortcut only enters a safe save or retry state", () => {
  assert.equal(canHandleStopShortcut("recording", false), true);
  assert.equal(canHandleStopShortcut("paused", false), true);
  assert.equal(canHandleStopShortcut("finish_failed", false), true);
  assert.equal(canHandleStopShortcut("starting", false), false);
  assert.equal(canHandleStopShortcut("failed", false), false);
  assert.equal(canHandleStopShortcut("stopping", false), false);
  assert.equal(canHandleStopShortcut("recording", true), false);
});

test("navigation is guarded while capture or a failed save needs a decision", () => {
  assert.equal(shouldGuardRecordingNavigation("recording"), true);
  assert.equal(shouldGuardRecordingNavigation("paused"), true);
  assert.equal(shouldGuardRecordingNavigation("finish_failed"), true);
  assert.equal(shouldGuardRecordingNavigation("starting"), false);
  assert.equal(shouldGuardRecordingNavigation("failed"), false);
  assert.equal(shouldGuardRecordingNavigation("stopping"), false);
});

test("leaving after startup failure preserves the recovery draft", () => {
  assert.equal(shouldDiscardAutoDraft(true, false), false);
  assert.equal(shouldDiscardAutoDraft(true, true), true);
  assert.equal(shouldDiscardAutoDraft(false, true), false);
});
