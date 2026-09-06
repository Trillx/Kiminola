import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
// @ts-expect-error Node imports TypeScript directly.
import { createDraftAutosave } from "../src/lib/draft-autosave.ts";
// @ts-expect-error Node imports TypeScript directly.
import { registerPendingSave, trackOperation } from "../src/lib/pending-work.ts";

let sequence = 0;
const dataUrl = (source: string) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;

async function controller() {
  // Exercise the real update controller, replacing only Tauri/runtime adapters.
  // $state is an ordinary mutable object here; UI reactivity is checked by Svelte.
  const mockUrl = dataUrl(`// adapter ${sequence++}
    export const browser = true;
    export const isTauri = () => true;
    export const calls = [];
    export const state = { candidate: null, downloadError: false, installError: false };
    export const invoke = async (command) => { calls.push(command); };
    export const check = async () => state.candidate;
    state.candidate = {
      version: '2.0.0',
      download: async () => { calls.push('download'); if (state.downloadError) throw new Error('network failed'); },
      install: async () => { calls.push('install'); if (state.installError) throw new Error('installer failed'); }
    };
  `);
  const paths: Record<string, string> = {
    "$app/environment": mockUrl,
    "@tauri-apps/api/core": mockUrl,
    "@tauri-apps/plugin-updater": mockUrl,
    "$lib/update-policy": new URL("../src/lib/update-policy.ts", import.meta.url).href,
    "$lib/pending-work": new URL("../src/lib/pending-work.ts", import.meta.url).href,
    "$lib/update-safety": new URL("../src/lib/update-safety.ts", import.meta.url).href,
  };
  let source = ts.transpileModule(readFileSync(new URL("../src/lib/update.svelte.ts", import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText;
  source = source.replace(/from "([^"]+)"/g, (_, name: string) => `from ${JSON.stringify(paths[name] ?? name)}`);
  const module = await import(dataUrl(`const $state = (value) => value;\n${source}`));
  const adapter = await import(mockUrl);
  await module.checkForUpdates();
  return { module, adapter };
}

test("real controller drains a pending editor and an in-flight write before installing once", async () => {
  const { module, adapter } = await controller();
  let release!: () => void;
  const pending = trackOperation(new Promise<void>((resolve) => { release = resolve; }));
  const saved: string[] = [];
  const editor = createDraftAutosave(async (text: string) => { saved.push(text); }, () => {}, 60_000);
  const dispose = registerPendingSave(() => editor.flushPending());
  editor.schedule("latest note");
  const first = module.installUpdate(() => true);
  const second = module.installUpdate(() => true);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(saved, ["latest note"]);
  assert.deepEqual(adapter.calls, ["download"]);
  release();
  await pending;
  assert.equal(await first, true);
  assert.equal(await second, true);
  assert.deepEqual(adapter.calls, ["download", "prepare_app_update", "install"]);
  dispose();
});

test("real controller keeps the downloaded update retryable after a note-save failure", async () => {
  const { module, adapter } = await controller();
  let fail = true;
  const editor = createDraftAutosave(async () => { if (fail) throw new Error("disk full"); }, () => {}, 60_000);
  const dispose = registerPendingSave(() => editor.flushPending());
  editor.schedule("must survive");
  assert.equal(await module.installUpdate(() => true), false);
  assert.equal(module.updateState.status, "ready");
  assert.match(module.updateState.error, /disk full/);
  assert.deepEqual(adapter.calls, ["download"]);
  fail = false;
  assert.equal(await module.installUpdate(() => true), true);
  assert.deepEqual(adapter.calls, ["download", "prepare_app_update", "install"]);
  dispose();
});

test("recording block never labels an undownloaded candidate ready", async () => {
  const { module, adapter } = await controller();
  assert.equal(await module.installUpdate(() => false), false);
  assert.equal(module.updateState.status, "available");
  assert.deepEqual(adapter.calls, []);
  assert.equal(await module.installUpdate(() => true), true);
  assert.deepEqual(adapter.calls, ["download", "prepare_app_update", "install"]);
});

test("real controller resumes the native app when installation fails", async () => {
  const { module, adapter } = await controller();
  adapter.state.installError = true;
  assert.equal(await module.installUpdate(() => true), false);
  assert.deepEqual(adapter.calls, ["download", "prepare_app_update", "install", "cancel_app_update"]);
  assert.equal(module.updateState.status, "ready");
});
