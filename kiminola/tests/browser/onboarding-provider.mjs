import assert from 'node:assert/strict';

async function providerStep(page) {
  await page.getByRole('button', { name: 'Allow microphone', exact: true }).click();
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByRole('button', { name: 'Already installed', exact: true }).click();
  await page.getByLabel('Base URL', { exact: true }).waitFor();
}

async function holdProviderRequests(page) {
  await page.evaluate(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
    window.audit.providerCalls = [];
    window.audit.providerPending = [];
    window.__TAURI_INTERNALS__.invoke = (cmd, args = {}) => {
      if (cmd !== 'set_llm_config' && cmd !== 'test_llm_config') return invoke(cmd, args);
      window.audit.providerCalls.push({ cmd, config: args.config ? { ...args.config } : undefined });
      return new Promise((resolve, reject) => {
        window.audit.providerPending.push({
          cmd,
          finish: async (fail = false) => {
            if (fail) { reject(new Error('Fixture: provider operation failed')); return; }
            try { resolve(await invoke(cmd, args)); } catch (error) { reject(error); }
          },
        });
      });
    };
  });
}

async function release(page, cmd, fail = false) {
  await page.evaluate(async ({ cmd, fail }) => {
    const index = window.audit.providerPending.findIndex(item => item.cmd === cmd);
    if (index < 0) throw new Error(`No pending fixture operation: ${cmd}`);
    const [pending] = window.audit.providerPending.splice(index, 1);
    await pending.finish(fail);
  }, { cmd, fail });
}

async function assertLocked(page) {
  for (const id of ['provider-kind', 'provider-base-url', 'provider-model', 'provider-key']) {
    assert.equal(await page.locator(`#${id}`).isDisabled(), true, `${id} must be locked during provider work`);
  }
  for (const name of ['Save provider', 'Skip for now']) {
    assert.equal(await page.getByRole('button', { name, exact: true }).isDisabled(), true, `${name} conflicts with provider work`);
  }
  assert.equal(await page.getByRole('button', { name: /^(Test connection|Testing…)$/ }).isDisabled(), true);
  // Even programmatic DOM clicks must not start another operation or skip the step.
  await page.getByRole('button', { name: 'Save provider', exact: true }).evaluate(button => button.click());
  await page.getByRole('button', { name: /^(Test connection|Testing…)$/ }).evaluate(button => button.click());
  await page.getByRole('button', { name: 'Skip for now', exact: true }).evaluate(button => button.click());
  assert.equal(await page.getByRole('heading', { name: 'AI Provider (optional)', exact: true }).count(), 1);
}

export async function runOnboardingProviderTests({ check, open, eventually, origin }) {
  await check('R2 Onboarding test locks fields and conflicting actions through save and test IPC', async page => {
    await page.goto(origin + '/onboarding');
    await providerStep(page);
    await holdProviderRequests(page);
    await page.getByRole('button', { name: 'Test connection', exact: true }).click();
    await page.waitForFunction(() => window.audit.providerPending.some(item => item.cmd === 'set_llm_config'));
    await assertLocked(page);
    assert.deepEqual(await page.evaluate(() => window.audit.providerCalls.map(item => item.cmd)), ['set_llm_config']);
    await release(page, 'set_llm_config');
    await page.waitForFunction(() => window.audit.providerPending.some(item => item.cmd === 'test_llm_config'));
    await assertLocked(page);
    assert.deepEqual(await page.evaluate(() => window.audit.providerCalls.map(item => item.cmd)), ['set_llm_config', 'test_llm_config']);
    await release(page, 'test_llm_config');
    await page.getByText('Connection successful.', { exact: true }).waitFor();
    assert.equal(await page.getByLabel('Base URL', { exact: true }).isDisabled(), false);
    assert.equal(await page.getByRole('button', { name: 'Save provider', exact: true }).isDisabled(), false);
  });

  await check('R2 Onboarding save locks fields and actions until the write completes', async page => {
    await page.goto(origin + '/onboarding');
    await providerStep(page);
    await holdProviderRequests(page);
    await page.getByRole('button', { name: 'Save provider', exact: true }).click();
    await page.waitForFunction(() => window.audit.providerPending.some(item => item.cmd === 'set_llm_config'));
    await assertLocked(page);
    assert.deepEqual(await page.evaluate(() => window.audit.providerCalls.map(item => item.cmd)), ['set_llm_config']);
    await release(page, 'set_llm_config');
    await page.getByRole('heading', { name: "You're all set", exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.audit.providerCalls.map(item => item.cmd)), ['set_llm_config']);
  });

  for (const fail of [false, true]) {
    await check(`R2 Onboarding ignores stale test ${fail ? 'failure' : 'success'} after an edit and revert`, async page => {
      await page.goto(origin + '/onboarding');
      await providerStep(page);
      await holdProviderRequests(page);
      await page.getByRole('button', { name: 'Test connection', exact: true }).click();
      await page.waitForFunction(() => window.audit.providerPending.some(item => item.cmd === 'set_llm_config'));
      await release(page, 'set_llm_config');
      await page.waitForFunction(() => window.audit.providerPending.some(item => item.cmd === 'test_llm_config'));
      const endpoint = page.getByLabel('Base URL', { exact: true });
      const original = await endpoint.inputValue();
      // Defense in depth: synthetic input bypasses the disabled DOM control.
      // Editing then reverting also requires generation invalidation, not just equality.
      for (const value of ['https://unsubmitted.example.invalid/v1', original]) {
        await endpoint.evaluate((input, value) => {
          input.value = value;
          input.dispatchEvent(new Event('input', { bubbles: true }));
        }, value);
      }
      await assertLocked(page);
      await release(page, 'test_llm_config', fail);
      await eventually(() => page.getByRole('button', { name: 'Test connection', exact: true }).isDisabled(), value => !value, 'Settled stale request releases its lock');
      assert.equal(await page.locator('.provider-form .status-card').count(), 0, 'Old result cannot certify even a reverted config');
      assert.deepEqual(await page.evaluate(() => window.audit.providerCalls.map(item => item.cmd)), ['set_llm_config', 'test_llm_config']);
    });
  }

  await check('R2 Onboarding model and key edits clear a completed test result', async page => {
    await page.goto(origin + '/onboarding');
    await providerStep(page);
    for (const [label, value] of [['Model', 'new-synthetic-model'], ['API key', 'synthetic-fixture-value']]) {
      await page.getByRole('button', { name: 'Test connection', exact: true }).click();
      await page.getByText('Connection successful.', { exact: true }).waitFor();
      await page.getByLabel(label, { exact: true }).fill(value);
      assert.equal(await page.getByText('Connection successful.', { exact: true }).count(), 0, `${label} changes invalidate the previous result`);
    }
  });

  await check('R2 Onboarding disposal prevents a pending save from launching its old connection test', async page => {
    await page.goto(origin + '/onboarding');
    await providerStep(page);
    await holdProviderRequests(page);
    await page.getByRole('button', { name: 'Test connection', exact: true }).click();
    await page.waitForFunction(() => window.audit.providerPending.some(item => item.cmd === 'set_llm_config'));
    // Navigate with SvelteKit's real link interception, without unloading the JS realm.
    await page.evaluate(() => {
      const link = document.createElement('a');
      link.href = '/';
      link.id = 'fixture-leave-onboarding';
      link.textContent = 'Fixture: leave onboarding';
      document.body.append(link);
    });
    await page.locator('#fixture-leave-onboarding').click();
    await page.locator('.main-content').waitFor();
    await page.goBack();
    await providerStep(page);
    await page.getByRole('button', { name: 'Test connection', exact: true }).click();
    await page.waitForFunction(() => window.audit.providerPending.filter(item => item.cmd === 'set_llm_config').length === 2);
    await release(page, 'set_llm_config');
    assert.deepEqual(await page.evaluate(() => window.audit.providerCalls.map(item => item.cmd)), ['set_llm_config', 'set_llm_config'], 'Disposed wizard must not send another IPC request');
    await assertLocked(page);
    await release(page, 'set_llm_config');
    await page.waitForFunction(() => window.audit.providerPending.some(item => item.cmd === 'test_llm_config'));
    await release(page, 'test_llm_config');
    await page.getByText('Connection successful.', { exact: true }).waitFor();
  });
}
