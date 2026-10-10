import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
// @ts-expect-error Node imports TypeScript directly.
import { createTranscriptRowKey } from "../src/lib/transcript-state.ts";

function transcriptSheet() {
  const effects: Array<() => void> = [];
  const preEffects: Array<() => void> = [];
  const cleanup: Array<() => void> = [];
  const effect = Object.assign((fn: () => void) => effects.push(fn), { pre: (fn: () => void) => preEffects.push(fn) });
  const adapters = {
    $props: () => ({ lines: [], open: false }), $bindable: (value: unknown) => value,
    $state: (value: unknown) => value, $derived: (value: unknown) => value, $effect: effect,
    tick: () => Promise.resolve(), onDestroy: (fn: () => void) => cleanup.push(fn),
    createTranscriptRowKey,
    window: { addEventListener() {}, removeEventListener() {} },
    setTimeout: () => 1, clearTimeout: () => {},
  };
  const component = readFileSync(new URL("../src/lib/components/LiveTranscript.svelte", import.meta.url), "utf8");
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText.replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "").replace(/^export \{\};?\s*$/gm, "");
  const api = new Function(...Object.keys(adapters), `${source}\nreturn {
    show(value) { open = value; }, bind(body) { bodyEl = body; },
    update(value) { lines = value; }, onScroll
  };`)(...Object.values(adapters));
  let top = 0;
  const body = {
    scrollHeight: 2454, clientHeight: 330,
    get scrollTop() { return top; },
    set scrollTop(value: number) { top = Math.max(0, Math.min(value, this.scrollHeight - this.clientHeight)); },
    classList: { add() {}, remove() {} },
  };
  const render = async (mutate: () => void = () => {}) => {
    preEffects.forEach((fn) => fn());
    mutate();
    effects.forEach((fn) => fn());
    await Promise.resolve();
  };
  return { ...api, body, render,
    dispose: () => cleanup.forEach((fn) => fn()),
    async open() { api.show(true); await render(() => api.bind(body)); },
  };
}

test("opening an accumulated transcript starts at the latest speech", async (t) => {
  const sheet = transcriptSheet();
  t.after(sheet.dispose);
  await sheet.open();
  assert.equal(sheet.body.scrollTop, 2124);
});

test("tall lines and partial growth follow only while the user is at the bottom", async (t) => {
  const sheet = transcriptSheet();
  t.after(sheet.dispose);
  await sheet.open();
  sheet.body.scrollTop = sheet.body.scrollHeight;
  sheet.onScroll();
  sheet.update([{ utterance_id: 1, channel: "you", text: "partial" }]);
  await sheet.render(() => { sheet.body.scrollHeight = 3000; });
  assert.equal(sheet.body.scrollTop, 2670);
  sheet.update([{ utterance_id: 1, channel: "you", text: "partial growing much taller" }]);
  await sheet.render(() => { sheet.body.scrollHeight = 3500; });
  assert.equal(sheet.body.scrollTop, 3170);
  sheet.body.scrollTop = 100;
  sheet.onScroll();
  sheet.update([{ utterance_id: 1, channel: "you", text: "still growing" }]);
  await sheet.render(() => { sheet.body.scrollHeight = 4000; });
  assert.equal(sheet.body.scrollTop, 100);
  sheet.body.scrollTop = sheet.body.scrollHeight;
  sheet.onScroll();
  await sheet.render(() => { sheet.body.scrollHeight = 4500; });
  assert.equal(sheet.body.scrollTop, 4170);
});
