import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
// @ts-expect-error Node imports TypeScript directly.
import { flushPendingWork } from "../src/lib/pending-work.ts";
// @ts-expect-error Node imports TypeScript directly.
import { installWhenSaved } from "../src/lib/update-safety.ts";

let sequence = 0;
const dataUrl = (source: string) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;

async function dictationIpc() {
  // Keep the production IPC wrapper and update drain. Replace only native IPC.
  const mockUrl = dataUrl(`// native boundary ${sequence++}
    export const calls = [];
    export function invoke(command, args) {
      return new Promise((resolve, reject) => calls.push({ command, args, resolve, reject }));
    }
    export async function listen() { return () => {}; }
  `);
  const paths: Record<string, string> = {
    "@tauri-apps/api/core": mockUrl,
    "@tauri-apps/api/event": mockUrl,
    "./pending-work": new URL("../src/lib/pending-work.ts", import.meta.url).href,
    "./pending-work.ts": new URL("../src/lib/pending-work.ts", import.meta.url).href,
  };
  let source = ts.transpileModule(readFileSync(new URL("../src/lib/dictation-ipc.ts", import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText;
  source = source.replace(/from "([^"]+)"/g, (_, name: string) => `from ${JSON.stringify(paths[name] ?? name)}`);
  return { ipc: await import(dataUrl(source)), adapter: await import(mockUrl) };
}

const input = {
  enabled: true, activation: "hold", shortcut: "Ctrl+Alt+D", side: "right", cleanup: "raw",
  clipboard_consent: false, history_enabled: true, microphone_id: null, consent_provider: false,
};
const operations = [
  { name: "settings save", method: "setDictationSettings", args: [input], command: "set_dictation_settings" },
  { name: "history deletion", method: "deleteDictationHistory", args: [7], command: "delete_dictation_history" },
  { name: "history clear", method: "deleteDictationHistory", args: [null], command: "delete_dictation_history" },
  { name: "review resolution", method: "resolveDictation", args: ["copy"], command: "resolve_dictation" },
  { name: "capture startup", method: "startDictation", args: [], command: "start_dictation" },
  { name: "capture stop", method: "stopDictation", args: [], command: "stop_dictation" },
  { name: "capture cancellation", method: "cancelDictation", args: [], command: "cancel_dictation" },
];

for (const operation of operations) {
  test(`update waits for pending dictation ${operation.name}`, async () => {
    const { ipc, adapter } = await dictationIpc();
    const pending = ipc[operation.method](...operation.args);
    const events: string[] = [];
    const update = installWhenSaved({
      flush: flushPendingWork,
      prepare: async () => { events.push("prepared"); },
      install: async () => { events.push("installed"); },
      cancel: async () => { events.push("cancelled"); },
    });
    try {
      await new Promise(resolve => setImmediate(resolve));
      assert.equal(adapter.calls[0].command, operation.command);
      assert.deepEqual(events, [], "the installer cannot overtake a pending dictation operation");
    } finally {
      adapter.calls[0].resolve();
      await pending;
      await update;
    }
    assert.deepEqual(events, ["prepared", "installed"]);
  });

  test(`failed pending dictation ${operation.name} prevents installation`, async () => {
    const { ipc, adapter } = await dictationIpc();
    const pending = ipc[operation.method](...operation.args).catch((error: Error) => error);
    const events: string[] = [];
    const update = installWhenSaved({
      flush: flushPendingWork,
      prepare: async () => { events.push("prepared"); },
      install: async () => { events.push("installed"); },
      cancel: async () => { events.push("cancelled"); },
    }).then(() => null, (error: Error) => error);
    await new Promise(resolve => setImmediate(resolve));
    const failure = new Error("synthetic native operation failed");
    adapter.calls[0].reject(failure);
    assert.equal(await pending, failure);
    assert.equal(await update, failure);
    assert.deepEqual(events, [], "failure must leave the app running before database suspension");
    await flushPendingWork();
  });
}
