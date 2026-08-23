import assert from "node:assert/strict";
import { test } from "node:test";
// @ts-expect-error Node's strip-types test runner imports the TypeScript source directly.
import { applyTranscriptEvent, finalizedTranscript } from "../src/lib/transcript-state.ts";
import type { TranscriptEvent, TranscriptLine } from "../src/lib/tauri.ts";

function event(overrides: Partial<TranscriptEvent> & Pick<TranscriptEvent, "utterance_id" | "channel" | "text">): TranscriptEvent {
  return {
    revision: 1,
    start_ms: 0,
    end_ms: 1_000,
    is_partial: false,
    ...overrides,
  };
}

test("keeps simultaneous partials from You and Others independent", () => {
  let lines: TranscriptLine[] = [];
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 1,
    channel: "you",
    text: "I can take that",
    is_partial: true,
  }));
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 2,
    channel: "others",
    text: "That sounds good",
    is_partial: true,
  }));
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 1,
    revision: 2,
    channel: "you",
    text: "I can take that action",
    is_partial: false,
  }));

  assert.deepEqual(lines.map(({ channel, text, is_partial }) => ({ channel, text, is_partial })), [
    { channel: "you", text: "I can take that action", is_partial: false },
    { channel: "others", text: "That sounds good", is_partial: true },
  ]);
});

test("ignores stale revisions of an utterance", () => {
  let lines: TranscriptLine[] = [];
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 7,
    revision: 2,
    channel: "others",
    text: "newer text",
  }));
  const unchanged = applyTranscriptEvent(lines, event({
    utterance_id: 7,
    revision: 1,
    channel: "others",
    text: "stale text",
  }));

  assert.equal(unchanged, lines);
  assert.equal(unchanged[0].text, "newer text");
});

test("prefers overlapping system audio when the microphone contains the same speech", () => {
  let lines: TranscriptLine[] = [];
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 10,
    channel: "you",
    text: "we should ship the update tomorrow",
    start_ms: 1_000,
    end_ms: 4_000,
  }));
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 11,
    channel: "others",
    text: "We should ship the update tomorrow.",
    start_ms: 1_200,
    end_ms: 4_100,
  }));

  assert.equal(lines.length, 1);
  assert.equal(lines[0].channel, "others");
});

test("does not suppress short acknowledgements or non-overlapping repeated phrases", () => {
  let lines: TranscriptLine[] = [];
  lines = applyTranscriptEvent(lines, event({ utterance_id: 20, channel: "you", text: "okay" }));
  lines = applyTranscriptEvent(lines, event({ utterance_id: 21, channel: "others", text: "okay" }));
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 22,
    channel: "you",
    text: "we should ship the update tomorrow",
    start_ms: 10_000,
    end_ms: 12_000,
  }));
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 23,
    channel: "others",
    text: "we should ship the update tomorrow",
    start_ms: 15_000,
    end_ms: 17_000,
  }));

  assert.equal(lines.length, 4);
});

test("retains both speakers during genuine double-talk", () => {
  let lines: TranscriptLine[] = [];
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 30,
    channel: "you",
    text: "I will send the revised proposal",
    start_ms: 2_000,
    end_ms: 5_000,
  }));
  lines = applyTranscriptEvent(lines, event({
    utterance_id: 31,
    channel: "others",
    text: "Can we review the budget first",
    start_ms: 2_100,
    end_ms: 4_800,
  }));

  assert.deepEqual(lines.map((line) => line.channel), ["you", "others"]);
});

test("saves only finalized text and retains audio-relative timing", () => {
  const saved = finalizedTranscript([
    {
      utterance_id: 1,
      revision: 2,
      channel: "you",
      text: "final words",
      start_ms: 250,
      end_ms: 1_500,
      is_partial: false,
    },
    {
      utterance_id: 2,
      revision: 1,
      channel: "others",
      text: "still changing",
      start_ms: 500,
      end_ms: 1_250,
      is_partial: true,
    },
  ]);

  assert.deepEqual(saved, [{
    channel: "you",
    text: "final words",
    start_ms: 250,
    end_ms: 1_500,
  }]);
});
