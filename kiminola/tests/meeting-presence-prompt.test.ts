import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

function meetingPresencePrompt() {
  let presenceState = (_state: unknown) => {};
  let presenceError = (_error: unknown) => {};
  let recordingStarted = () => {};
  const idleState = {
    enabled: true,
    paused: false,
    start_with_windows: false,
    mode: "detecting",
    hint: null,
    prompt: null,
  };
  const adapters = {
    $props: () => ({}),
    $state: (value: unknown) => value,
    onMount: (callback: () => void) => callback(),
    onMeetingPresencePrompt: () => Promise.resolve(() => {}),
    onMeetingPresenceState: (callback: typeof presenceState) => {
      presenceState = callback;
      return Promise.resolve(() => {});
    },
    onMeetingPresenceAction: () => Promise.resolve(() => {}),
    onMeetingPresenceError: (callback: typeof presenceError) => {
      presenceError = callback;
      return Promise.resolve(() => {});
    },
    onRecordingStarted: (callback: typeof recordingStarted) => {
      recordingStarted = callback;
      return Promise.resolve(() => {});
    },
    getMeetingPresenceState: async () => idleState,
    getCurrentWebviewWindow: () => ({ hide: async () => {} }),
    WebviewWindow: { getByLabel: async () => null },
    goto: async () => {},
    console: { error: () => {} },
  };
  const component = readFileSync(
    new URL("../src/lib/components/MeetingPresencePrompt.svelte", import.meta.url),
    "utf8",
  );
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts
    .transpileModule(script, {
      compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
    })
    .outputText.replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "")
    .replace(/^export \{\};?\s*$/gm, "");
  const api = new Function(
    ...Object.keys(adapters),
    `${source}\nreturn { state: () => ({ prompt, error }) };`,
  )(...Object.values(adapters));
  return {
    ...api,
    consumePromptWithError() {
      presenceState(idleState);
      presenceError({ prompt_id: "prompt-1", message: "The meeting target is no longer available." });
    },
    recordingStarted: () => recordingStarted(),
  };
}

test("successful manual recording clears a consumed meeting prompt failure", () => {
  const prompt = meetingPresencePrompt();
  prompt.consumePromptWithError();
  assert.equal(prompt.state().prompt, null);
  assert.match(prompt.state().error, /no longer available/);

  prompt.recordingStarted();

  assert.equal(prompt.state().error, "");
});
