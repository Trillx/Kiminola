import assert from "node:assert/strict";
import { test } from "node:test";
import { Channel } from "@tauri-apps/api/core";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
// @ts-expect-error Node strip-types imports the production Tauri command wrappers.
import { enhanceMeeting, testLlmConfig, type LlmStreamEvent } from "../src/lib/tauri.ts";

test("meeting enhancements and provider tests receive only their own IPC stream", async () => {
  Object.defineProperty(globalThis, "window", { value: { crypto: globalThis.crypto }, configurable: true });
  const requests: Array<{ command: string; channel: Channel<LlmStreamEvent>; meetingId: unknown }> = [];
  mockIPC((command, payload) => {
    assert.ok(payload && "onEvent" in payload);
    assert.ok(payload.onEvent instanceof Channel);
    requests.push({ command, channel: payload.onEvent, meetingId: "meetingId" in payload ? payload.meetingId : undefined });
  });
  try {
    const a: LlmStreamEvent[] = [], b: LlmStreamEvent[] = [], provider: LlmStreamEvent[] = [];
    await Promise.all([
      enhanceMeeting(101, 1, (event) => a.push(event)),
      enhanceMeeting(202, 1, (event) => b.push(event)),
      testLlmConfig((event) => provider.push(event)),
    ]);
    assert.equal(new Set(requests.map(({ channel }) => channel.id)).size, 3);
    assert.deepEqual(requests.map(({ command, meetingId }) => [command, meetingId]), [
      ["enhance_meeting", 101], ["enhance_meeting", 202], ["test_llm_config", undefined],
    ]);
    requests[0].channel.onmessage({ event: "chunk", data: "A only" });
    requests[2].channel.onmessage({ event: "chunk", data: "Hello" });
    requests[1].channel.onmessage({ event: "error", data: "B failed" });
    requests[0].channel.onmessage({ event: "done" });
    assert.deepEqual(a, [{ event: "chunk", data: "A only" }, { event: "done" }]);
    assert.deepEqual(b, [{ event: "error", data: "B failed" }]);
    assert.deepEqual(provider, [{ event: "chunk", data: "Hello" }]);
  } finally {
    clearMocks();
    Reflect.deleteProperty(globalThis, "window");
  }
});
