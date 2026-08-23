import type { TranscriptEvent, TranscriptLine } from "./tauri";

const MIN_ECHO_WORDS = 3;
const MIN_ECHO_CHARACTERS = 12;
const MIN_TIME_OVERLAP = 0.35;
const MIN_TEXT_SIMILARITY = 0.82;

function normalizedWords(text: string): string[] {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}'\s]/gu, " ")
    .split(/\s+/)
    .filter(Boolean);
}

function levenshteinDistance(left: string, right: string): number {
  if (left.length === 0) return right.length;
  if (right.length === 0) return left.length;

  let previous = Array.from({ length: right.length + 1 }, (_, index) => index);
  for (let leftIndex = 0; leftIndex < left.length; leftIndex += 1) {
    const current = [leftIndex + 1];
    for (let rightIndex = 0; rightIndex < right.length; rightIndex += 1) {
      const substitution = previous[rightIndex] + (left[leftIndex] === right[rightIndex] ? 0 : 1);
      current.push(
        Math.min(
          current[rightIndex] + 1,
          previous[rightIndex + 1] + 1,
          substitution,
        ),
      );
    }
    previous = current;
  }
  return previous[right.length];
}

function textSimilarity(left: string, right: string): number {
  const normalizedLeft = normalizedWords(left).join(" ");
  const normalizedRight = normalizedWords(right).join(" ");
  const longest = Math.max(normalizedLeft.length, normalizedRight.length);
  if (longest === 0) return 1;
  return 1 - levenshteinDistance(normalizedLeft, normalizedRight) / longest;
}

function overlapRatio(left: TranscriptLine, right: TranscriptLine): number {
  if (
    left.start_ms === undefined ||
    left.end_ms === undefined ||
    right.start_ms === undefined ||
    right.end_ms === undefined
  ) {
    return 0;
  }

  const overlap = Math.max(
    0,
    Math.min(left.end_ms, right.end_ms) - Math.max(left.start_ms, right.start_ms),
  );
  const shorterDuration = Math.max(
    1,
    Math.min(left.end_ms - left.start_ms, right.end_ms - right.start_ms),
  );
  return overlap / shorterDuration;
}

function isLikelyEcho(left: TranscriptLine, right: TranscriptLine): boolean {
  if (left.channel === right.channel || left.is_partial || right.is_partial) return false;

  const leftWords = normalizedWords(left.text);
  const rightWords = normalizedWords(right.text);
  if (
    leftWords.length < MIN_ECHO_WORDS ||
    rightWords.length < MIN_ECHO_WORDS ||
    leftWords.join(" ").length < MIN_ECHO_CHARACTERS ||
    rightWords.join(" ").length < MIN_ECHO_CHARACTERS
  ) {
    return false;
  }

  return (
    overlapRatio(left, right) >= MIN_TIME_OVERLAP &&
    textSimilarity(left.text, right.text) >= MIN_TEXT_SIMILARITY
  );
}

function lineFromEvent(event: TranscriptEvent): TranscriptLine {
  return {
    channel: event.channel,
    text: event.text,
    utterance_id: event.utterance_id,
    revision: event.revision,
    start_ms: event.start_ms,
    end_ms: event.end_ms,
    is_partial: event.is_partial,
  };
}

function compareTranscriptLines(left: TranscriptLine, right: TranscriptLine): number {
  const startDifference = (left.start_ms ?? Number.MAX_SAFE_INTEGER) - (right.start_ms ?? Number.MAX_SAFE_INTEGER);
  if (startDifference !== 0) return startDifference;
  return (left.utterance_id ?? left.id ?? 0) - (right.utterance_id ?? right.id ?? 0);
}

/**
 * Applies one backend transcript revision without allowing one audio lane to
 * overwrite the other. Finalized cross-source duplicates are reconciled in
 * favor of system audio (`Others`), which is the clean copy when laptop
 * speaker output leaks back into the microphone.
 */
export function applyTranscriptEvent(
  lines: TranscriptLine[],
  event: TranscriptEvent,
): TranscriptLine[] {
  const incoming = lineFromEvent(event);
  const existingIndex = lines.findIndex((line) => line.utterance_id === event.utterance_id);
  const existing = existingIndex >= 0 ? lines[existingIndex] : undefined;

  if ((existing?.revision ?? -1) >= event.revision) return lines;

  let next = existingIndex >= 0
    ? lines.map((line, index) => (index === existingIndex ? incoming : line))
    : [...lines, incoming];

  if (!incoming.is_partial) {
    const duplicate = next.find(
      (line) => line.utterance_id !== incoming.utterance_id && isLikelyEcho(line, incoming),
    );
    if (duplicate) {
      if (incoming.channel === "you") {
        // The clean system copy already exists, so discard the leaked mic copy.
        next = next.filter((line) => line.utterance_id !== incoming.utterance_id);
      } else {
        // System audio arrived after its leaked mic copy; replace the mic copy.
        next = next.filter((line) => line.utterance_id !== duplicate.utterance_id);
      }
    }
  }

  return next.sort(compareTranscriptLines);
}

export function finalizedTranscript(lines: TranscriptLine[]): TranscriptLine[] {
  return lines
    .filter((line) => !line.is_partial && line.text.trim().length > 0)
    .map(({ channel, text, start_ms, end_ms }) => ({
      channel,
      text,
      start_ms,
      end_ms,
    }));
}
