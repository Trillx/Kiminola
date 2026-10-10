import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));

function controller(searchMeetings: (query: string) => Promise<unknown[]>) {
  const effects: Array<() => void | (() => void)> = [];
  const destroys: Array<() => void> = [];
  const timers = new Map<number, () => void>();
  let timerId = 0;
  const component = readFileSync(new URL("../src/lib/components/SearchDialog.svelte", import.meta.url), "utf8");
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText
    .replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "").replace(/^export \{\};?\s*$/gm, "");
  const adapters = {
    $state: (value: unknown) => value,
    $props: () => ({ open: true }),
    $bindable: (value: unknown) => value,
    $effect: (fn: () => void | (() => void)) => effects.push(fn),
    onDestroy: (fn: () => void) => destroys.push(fn),
    searchMeetings,
    goto: () => {},
    console: { error: () => {} },
    setTimeout: (fn: () => void) => { const id = ++timerId; timers.set(id, fn); return id; },
    clearTimeout: (id: number) => timers.delete(id),
  };
  const api = new Function(...Object.keys(adapters), source + `\nreturn {
    input(value) { query = value; onInput(); },
    setOpen(value) { open = value; },
    state() { return { query, results, searching, error: typeof searchError === "undefined" ? "" : searchError }; }
  };`)(...Object.values(adapters));
  let cleanup = effects[0]?.();
  return {
    ...api,
    timers,
    advance() { const pending = [...timers.values()]; timers.clear(); pending.forEach((fn) => fn()); },
    close() { cleanup?.(); api.setOpen(false); cleanup = effects[0]?.(); },
    dispose() { cleanup?.(); destroys.forEach((fn) => fn()); },
  };
}

test("late search responses cannot overwrite the current query's results", async () => {
  const first = deferred<unknown[]>();
  const second = deferred<unknown[]>();
  const ui = controller((query) => query === "alpha" ? first.promise : second.promise);
  ui.input("alpha"); ui.advance();
  ui.input("beta"); ui.advance();
  second.resolve([{ id: 2, title: "Beta" }]); await tick();
  first.resolve([{ id: 1, title: "Alpha" }]); await tick();
  assert.deepEqual(ui.state().results, [{ id: 2, title: "Beta" }]);
  ui.dispose();
});

test("clearing immediately invalidates an in-flight request", async () => {
  const request = deferred<unknown[]>();
  const ui = controller(() => request.promise);
  ui.input("alpha"); ui.advance();
  ui.input("");
  request.resolve([{ id: 1 }]); await tick();
  assert.deepEqual(ui.state().results, []);
  assert.equal(ui.state().searching, false);
  ui.dispose();
});

test("closing cancels the debounce and ignores outstanding results", async () => {
  const request = deferred<unknown[]>();
  const ui = controller(() => request.promise);
  ui.input("alpha"); ui.advance();
  ui.input("beta"); ui.close();
  assert.equal(ui.timers.size, 0);
  request.resolve([{ id: 1 }]); await tick();
  assert.deepEqual(ui.state().results, []);
  ui.dispose();
});

test("a failed current search exposes an error rather than no matches", async () => {
  const ui = controller(async () => { throw new Error("Database busy"); });
  ui.input("alpha"); ui.advance(); await tick();
  assert.match(ui.state().error, /Database busy/);
  assert.equal(ui.state().searching, false);
  ui.dispose();
});
