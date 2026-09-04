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
  let apiKey = $state("");
  let apiKeyTouched = $state(false);
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

  async function save() {
    if (!config) return;
    saving = true;
    saveSuccess = false;
    saveError = "";
    try {
      await setLlmConfig(config, apiKeyTouched ? apiKey : undefined);
      apiKey = "";
      apiKeyTouched = false;
      saveSuccess = true;
      setTimeout(() => (saveSuccess = false), 3000);
      onSaved?.();
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

  const isConfigured = $derived(
    loaded &&
      config != null &&
      config.base_url.trim() !== "" &&
      config.model.trim() !== "",
  );
</script>

{#if !loaded}
  <div class="empty-state">Loading provider settings…</div>
{:else if config}
  <div class="provider-config">
    {#if isConfigured}
      <div class="provider-status">
        <span class="status-icon"><CheckIcon size={12} strokeWidth={3} /></span>
        <div>
          <div class="status-title">AI provider is configured</div>
          <div class="status-detail">{providerLabel} — {config.model}</div>
        </div>
      </div>
    {:else}
      <div class="enhance-title">Configure AI provider</div>
    {/if}
    <div class="enhance-copy">
      Your API key is stored in the OS keychain. It is never sent anywhere except to the provider
      you choose.
    </div>

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
      <Label for="provider-base-url">Base URL</Label>
      <Input
        id="provider-base-url"
        type="text"
        bind:value={config.base_url}
        placeholder="https://api.openai.com/v1"
      />
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

    <div class="field">
      <Label for="provider-key">API key</Label>
      <Input
        id="provider-key"
        type="password"
        bind:value={apiKey}
        oninput={() => (apiKeyTouched = true)}
        placeholder="sk-..."
      />
    </div>

    <div class="config-actions">
      <Button onclick={save} disabled={saving}>
        {saving ? "Saving…" : "Save provider"}
      </Button>
      <Button variant="outline" onclick={test} disabled={testing}>
        {testing ? "Testing…" : "Test connection"}
      </Button>
    </div>

    {#if saveSuccess}
      <div class="save-success">
        <span class="status-icon"><CheckIcon size={12} strokeWidth={3} /></span>
        <span>Provider saved. You're ready to enhance notes.</span>
      </div>
    {/if}
    {#if saveError}
      <div class="test-output error">{saveError}</div>
    {/if}
    {#if testOutput}
      <div class="test-output" class:error={testOutput.startsWith("Connection failed")}>
        {testOutput}
      </div>
    {/if}
  </div>
{/if}
