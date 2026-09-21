import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const component = readFileSync(new URL("../src/routes/onboarding/+page.svelte", import.meta.url), "utf8");
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
const initialConfig = () => ({ kind: "open_ai", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini" });

function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

// Handler tests complement, not replace, the rendered Svelte browser regressions.
function onboarding(overrides: Record<string, unknown> = {}) {
  const destroy: Array<() => void> = [];
  const writes: Array<{ config: ReturnType<typeof initialConfig>; apiKey: string | undefined }> = [];
  let tests = 0;
  const adapters = {
    $state: (value: unknown) => value,
    $derived: (value: unknown) => value,
    onDestroy: (fn: () => void) => destroy.push(fn),
    setLlmConfig: async (config: ReturnType<typeof initialConfig>, apiKey?: string) => { writes.push({ config, apiKey }); },
    testLlmConfig: async () => { tests++; },
    ...overrides,
  };
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const source = ts.transpileModule(script, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText.replace(/^import[\s\S]*?from\s+["'][^"']+["'];\s*/gm, "").replace(/^export \{\};?\s*$/gm, "");
  const api = new Function(...Object.keys(adapters), `${source}\nstep = 3; return {
    saveProvider, testProvider, skipProvider, applyProviderPreset,
    replaceKey(value) { apiKey = value; },
    clearVisibleStatus() { testState = 'idle'; },
    mutateConfig(value) { Object.assign(provider, value); },
    state() { return { provider: { ...provider }, apiKey, step, testState, testError }; }
  };`)(...Object.values(adapters));
  return { ...api, writes, testCount: () => tests, dispose: () => destroy.forEach(fn => fn()) };
}

for (const action of ["saveProvider", "testProvider"] as const) {
  test(`onboarding ${action} excludes duplicate save, test and skip even if visible status is reset`, async (t) => {
    const pending = deferred();
    let writes = 0;
    const form = onboarding({ setLlmConfig: () => { writes++; return pending.promise; } });
    t.after(form.dispose);
    const running = form[action]();
    form.clearVisibleStatus();
    void form.saveProvider();
    void form.testProvider();
    form.skipProvider();
    form.applyProviderPreset("open_router");
    assert.equal(writes, 1, "a pending operation owns the provider independently of its visible result");
    assert.equal(form.state().step, 3);
    assert.deepEqual(form.state().provider, initialConfig());
    pending.resolve();
    await running;
  });
}

test("onboarding submits a detached configuration snapshot", async (t) => {
  const pending = deferred();
  let submitted: ReturnType<typeof initialConfig> | undefined;
  const form = onboarding({ setLlmConfig: (config: ReturnType<typeof initialConfig>) => { submitted = config; return pending.promise; } });
  t.after(form.dispose);
  const running = form.saveProvider();
  form.mutateConfig({ base_url: "https://different.example.invalid/v1", model: "different-model" });
  assert.deepEqual(submitted, initialConfig(), "live edits must never retarget a submitted write");
  pending.resolve();
  await running;
  assert.equal(form.state().step, 3, "a stale write cannot advance changed settings");
});

for (const change of [
  { kind: "open_router" },
  { base_url: "https://different.example.invalid/v1" },
  { model: "different-model" },
  { apiKey: "synthetic-replacement" },
]) {
  test(`late onboarding test success cannot certify changed ${Object.keys(change)[0]}`, async (t) => {
    const pending = deferred();
    const form = onboarding({ testLlmConfig: () => pending.promise });
    t.after(form.dispose);
    const running = form.testProvider();
    await tick();
    if ("apiKey" in change) form.replaceKey(change.apiKey);
    else form.mutateConfig(change);
    pending.resolve();
    await running;
    assert.notEqual(form.state().testState, "success");
  });
}

test("an obsolete onboarding write does not launch a connection test", async (t) => {
  const pending = deferred();
  const form = onboarding({ setLlmConfig: () => pending.promise });
  t.after(form.dispose);
  const running = form.testProvider();
  form.mutateConfig({ model: "different-model" });
  pending.resolve();
  await running;
  assert.equal(form.testCount(), 0);
});

test("late onboarding failures cannot replace the current configuration's result", async (t) => {
  const pending = deferred();
  const form = onboarding({ testLlmConfig: () => pending.promise });
  t.after(form.dispose);
  const running = form.testProvider();
  await tick();
  form.mutateConfig({ model: "different-model" });
  form.clearVisibleStatus();
  pending.reject(new Error("obsolete fixture failure"));
  await running;
  assert.equal(form.state().testState, "idle");
  assert.equal(form.state().testError, "");
});

for (const action of ["saveProvider", "testProvider"] as const) {
  test(`disposing onboarding invalidates pending ${action} before its write completes`, async () => {
    const pending = deferred();
    const form = onboarding({ setLlmConfig: () => pending.promise });
    const running = form[action]();
    form.dispose();
    const disposedState = form.state();
    pending.resolve();
    await running;
    assert.deepEqual(form.state(), disposedState, "a disposed wizard must not advance or publish a result");
    assert.equal(form.testCount(), 0, "disposing cancels the unsent connection-test phase");
    await form.testProvider();
    await form.saveProvider();
    form.skipProvider();
    assert.deepEqual(form.state(), disposedState);
    assert.equal(form.testCount(), 0);
  });
}

for (const fail of [false, true]) {
  test(`disposing onboarding ignores late test ${fail ? "failure" : "success"}`, async () => {
    const pending = deferred();
    const form = onboarding({ testLlmConfig: () => pending.promise });
    const running = form.testProvider();
    await tick();
    form.dispose();
    const disposedState = form.state();
    if (fail) pending.reject(new Error("disposed fixture failure"));
    else pending.resolve();
    await running;
    assert.deepEqual(form.state(), disposedState);
  });
}
