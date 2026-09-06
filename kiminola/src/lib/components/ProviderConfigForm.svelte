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
  import { isProviderConfigDirty, providerIsConfigured } from "$lib/settings-ui";

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
  let saving = $state(false);
  let testing = $state(false);
  let testOutput = $state("");
  let saveSuccess = $state(false);
  let saveError = $state("");

  $effect(() => {
    getLlmConfig()
      .then((c) => {
        config = c;
        savedConfig = { ...c };
      })
      .catch((err) => {
        console.error("Failed to load LLM config:", err);
      })
      .finally(() => {
        loaded = true;
      });
  });

  function setProviderDefaults(kind: ProviderKind) {
    if (!config) return;
    config = {
      ...config,
      kind,
      base_url: DEFAULT_URLS[kind],
      model: DEFAULT_MODELS[kind],
    };
  }

  async function save(runTest = false) {
    if (!config) return;
    saving = true;
    saveSuccess = false;
    saveError = "";
    try {
      const hasReplacementKey = apiKey.trim() !== "";
      await setLlmConfig(config, hasReplacementKey ? apiKey : undefined);
      if (hasReplacementKey) config = { ...config, has_api_key: true };
      apiKey = "";
      savedConfig = { ...config };
      saveSuccess = true;
      setTimeout(() => (saveSuccess = false), 3000);
      onSaved?.();
      if (runTest) await test();
    } catch (err) {
      saveError = String(err);
      console.error("Failed to save LLM config:", err);
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

  const canTestAfterSave = $derived(
    config != null &&
      providerIsConfigured({
        ...config,
        has_api_key: config.has_api_key === true || apiKey.trim() !== "",
      }),
  );
</script>

{#if !loaded}
  <div class="empty-state">Loading provider settings…</div>
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
        bind:value={config.model}
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
          bind:value={config.base_url}
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
        bind:value={apiKey}
        placeholder={config.has_api_key
          ? "Saved — enter a new key to replace it"
          : usesLocalProvider
            ? "Optional for local provider"
            : "Enter API key"}
        autocomplete="off"
      />
      <span class="field-help">
        Stored in Windows Credential Manager. Leave blank to keep the saved key; it is sent only to
        the provider you choose.
      </span>
    </div>

    <div class="config-actions">
      <Button onclick={() => void save(true)} disabled={saving || testing || !isDirty || !canTestAfterSave}>
        {saving ? "Saving…" : testing ? "Testing…" : "Save and test"}
      </Button>
      <Button variant="outline" onclick={test} disabled={saving || testing || !isConfigured}>
        {testing ? "Testing…" : "Test saved connection"}
      </Button>
      {#if !isDirty && isConfigured}<span class="saved-indicator">Saved</span>{/if}
    </div>

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
