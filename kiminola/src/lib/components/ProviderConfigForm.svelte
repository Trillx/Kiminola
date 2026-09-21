<script lang="ts">
  import { onDestroy } from "svelte";
  import {
    getLlmConfig,
    setLlmConfig,
    testLlmConfig,
    type ProviderConfig,
    type ProviderKind,
  } from "$lib/tauri";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import * as Select from "$lib/components/ui/select";
  import CheckIcon from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import { isProviderConfigDirty, providerIdentityChanged, providerIsConfigured } from "$lib/settings-ui";

  interface Props {
    onSaved?: () => void;
  }

  let { onSaved }: Props = $props();

  const PROVIDER_KINDS: { value: ProviderKind; label: string }[] = [
    { value: "open_ai", label: "OpenAI" },
    { value: "open_router", label: "OpenRouter" },
    { value: "ollama", label: "Ollama" },
    { value: "lm_studio", label: "LM Studio" },
  ];

  const DEFAULT_URLS: Record<ProviderKind, string> = {
    open_ai: "https://api.openai.com/v1",
    open_router: "https://openrouter.ai/api/v1",
    ollama: "http://localhost:11434/v1",
    lm_studio: "http://localhost:1234/v1",
  };

  const DEFAULT_MODELS: Record<ProviderKind, string> = {
    open_ai: "gpt-4o-mini",
    open_router: "openai/gpt-4o-mini",
    ollama: "llama3.1",
    lm_studio: "default",
  };

  let config = $state<ProviderConfig | null>(null);
  let savedConfig = $state<ProviderConfig | null>(null);
  let apiKey = $state("");
  let loaded = $state(false);
  let loadError = $state("");
  let saving = $state(false);
  let testing = $state(false);
  let testOutput = $state("");
  let saveSuccess = $state(false);
  let saveError = $state("");

  $effect(() => {
    void loadProviderConfig();
  });

  async function loadProviderConfig() {
    loaded = false;
    loadError = "";
    try {
      const c = await getLlmConfig();
      if (disposed) return;
      config = c;
      savedConfig = { ...c };
    } catch {
      if (disposed) return;
      config = null;
      loadError = "Could not load provider settings. Your saved settings have not been changed. Retry to load them again.";
    } finally {
      if (!disposed) loaded = true;
    }
  }

  function setProviderDefaults(kind: ProviderKind) {
    if (!config || config.kind === kind) return;
    changeIdentity({
      ...config,
      kind,
      base_url: DEFAULT_URLS[kind],
      model: DEFAULT_MODELS[kind],
    });
  }

  function setBaseUrl(base_url: string) {
    if (config) changeIdentity({ ...config, base_url });
  }

  function setModel(model: string) {
    if (!config || saving || testing || config.model === model) return;
    config = { ...config, model };
    testOutput = "";
  }

  function setApiKey(value: string) {
    if (saving || testing || apiKey === value) return;
    apiKey = value;
    testOutput = "";
  }

  function changeIdentity(next: ProviderConfig) {
    if (!config || saving || testing) return;
    if (providerIdentityChanged(config, next)) {
      // Reuse only presence already confirmed for this exact saved identity.
      // Other destinations must be saved/read back before their key is known.
      next = {
        ...next,
        has_api_key: savedConfig != null &&
          !providerIdentityChanged(savedConfig, next) && savedConfig.has_api_key === true,
      };
      apiKey = "";
      testOutput = "";
      saveSuccess = false;
      saveError = "";
    }
    config = next;
  }

  async function save(runTest = false) {
    if (!config || saving || testing || disposed) return;
    saving = true;
    saveSuccess = false;
    saveError = "";
    let persisted = false;
    try {
      const hasReplacementKey = apiKey.trim() !== "";
      await setLlmConfig({ ...config }, hasReplacementKey ? apiKey : undefined);
      persisted = true;
      if (disposed) return;
      apiKey = "";
      const refreshed = await getLlmConfig();
      if (disposed) return;
      config = refreshed;
      savedConfig = { ...refreshed };
      saveSuccess = true;
      setTimeout(() => (saveSuccess = false), 3000);
      onSaved?.();
      if (runTest) await test();
    } catch (err) {
      if (disposed) return;
      if (persisted) {
        config = null;
        loadError = "Provider saved, but its status could not be reloaded. Retry to reload it; no connection test was sent.";
      } else {
        saveError = String(err);
      }
    } finally {
      saving = false;
    }
  }

  let disposed = false;
  onDestroy(() => { disposed = true; });

  async function test() {
    if (!config || testing) return;
    testing = true;
    testOutput = "";
    try {
      await testLlmConfig((event) => {
        if (disposed) return;
        if (event.event === "chunk") testOutput += event.data;
        if (event.event === "done") testOutput = testOutput.trim() || "Connection succeeded.";
        if (event.event === "error") testOutput = `Connection failed: ${event.data}`;
      });
    } catch (err) {
      if (!disposed) testOutput = `Connection failed: ${err}`;
    } finally {
      testing = false;
    }
  }

  const providerLabel = $derived(
    PROVIDER_KINDS.find((o) => o.value === config?.kind)?.label ?? "Provider",
  );

  const isConfigured = $derived(loaded && config != null && providerIsConfigured(config));

  const usesLocalProvider = $derived(
    config?.kind === "ollama" || config?.kind === "lm_studio",
  );

  const isDirty = $derived(
    config != null && isProviderConfigDirty(savedConfig, config, apiKey),
  );

  const canSave = $derived(
    config != null && config.base_url.trim() !== "" && config.model.trim() !== "",
  );

  const canTestAfterSave = $derived(
    config != null &&
      providerIsConfigured({
        ...config,
        has_api_key: config.has_api_key === true || apiKey.trim() !== "",
      }),
  );
</script>

{#if !loaded}
  <div class="empty-state" role="status">Loading provider settings…</div>
{:else if loadError}
  <div class="provider-config provider-settings-form">
    <div class="test-output error" role="alert">{loadError}</div>
    <Button variant="outline" onclick={() => void loadProviderConfig()}>Retry</Button>
  </div>
{:else if config}
  <div class="provider-config provider-settings-form">
    <header class="provider-heading">
      <div>
        <h2>AI provider</h2>
        <p>Configure the provider used to enhance meeting notes.</p>
      </div>
      {#if isConfigured}
        <span class="provider-status"><CheckIcon size={13} strokeWidth={2.5} aria-hidden="true" /> Configured · {providerLabel} · {config.model}</span>
      {/if}
    </header>

    <div class="field">
      <Label for="provider-kind">Provider</Label>
      <Select.Root
        type="single"
        disabled={saving || testing}
        value={config.kind}
        onValueChange={(value) => setProviderDefaults(value as ProviderKind)}
      >
        <Select.Trigger id="provider-kind" class="w-full">
          {providerLabel}
        </Select.Trigger>
        <Select.Content>
          {#each PROVIDER_KINDS as option (option.value)}
            <Select.Item value={option.value} label={option.label}>
              {option.label}
            </Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </div>

    <div class="field">
      <Label for="provider-model">Model</Label>
      <Input
        id="provider-model"
        type="text"
        disabled={saving || testing}
        value={config.model}
        oninput={(event) => setModel(event.currentTarget.value)}
        placeholder="gpt-4o-mini"
      />
    </div>

    <details class="provider-advanced">
      <summary><span>Advanced</span><ChevronDown size={15} aria-hidden="true" /></summary>
      <div class="field">
        <Label for="provider-base-url">Base URL</Label>
        <Input
          id="provider-base-url"
          type="text"
          disabled={saving || testing}
          value={config.base_url}
          oninput={(event) => setBaseUrl(event.currentTarget.value)}
          placeholder="https://api.openai.com/v1"
        />
        <span class="field-help">Change this only for a custom or self-hosted endpoint.</span>
      </div>
    </details>

    <div class="field">
      <Label for="provider-key">API key</Label>
      <Input
        id="provider-key"
        type="password"
        disabled={saving || testing}
        value={apiKey}
        oninput={(event) => setApiKey(event.currentTarget.value)}
        placeholder={config.has_api_key
          ? "Saved — enter a new key to replace it"
          : usesLocalProvider
            ? "Optional for local provider"
            : "Enter API key"}
        autocomplete="off"
      />
      <span class="field-help">
        Stored in Windows Credential Manager for this provider and Base URL only. Changing either
        clears the replacement key. Leave blank to keep only this endpoint’s saved key.
        Older unscoped keys are not reused; re-enter your key if needed.
      </span>
    </div>

    <div class="config-actions">
      <Button onclick={() => void save(canTestAfterSave)} disabled={saving || testing || !isDirty || !canSave}>
        {saving ? "Saving…" : testing ? "Testing…" : canTestAfterSave ? "Save and test" : "Save provider"}
      </Button>
      <Button variant="outline" onclick={test} disabled={saving || testing || isDirty || !isConfigured}>
        {testing ? "Testing…" : "Test saved connection"}
      </Button>
      {#if !isDirty && isConfigured}<span class="saved-indicator">Saved</span>{/if}
    </div>

    {#if isDirty && canSave && !canTestAfterSave}
      <span class="field-help">Save this provider to check for its own stored key. No connection test will be sent.</span>
    {/if}

    {#if saveSuccess}
      <div class="save-success" role="status">
        <CheckIcon size={14} aria-hidden="true" />
        <span>Provider saved.</span>
      </div>
    {/if}
    {#if saveError}
      <div class="test-output error" role="alert">{saveError}</div>
    {/if}
    {#if testOutput}
      <div
        class="test-output"
        class:error={testOutput.startsWith("Connection failed")}
        role={testOutput.startsWith("Connection failed") ? "alert" : "status"}
      >
        {testOutput}
      </div>
    {/if}
  </div>
{/if}
