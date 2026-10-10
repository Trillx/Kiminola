<script lang="ts">
  import { onDestroy } from "svelte";
  import { goto } from "$app/navigation";
  import {
    checkMicrophonePermission,
    checkModelPack,
    downloadModelPack,
    openMicrophonePrivacySettings,
    openModelFolder,
    setLlmConfig,
    setOnboardingComplete,
    testLlmConfig,
    type DownloadEvent,
    type ProviderConfig,
    type ProviderKind,
  } from "$lib/tauri";
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent } from "$lib/components/ui/card";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Progress } from "$lib/components/ui/progress";
  import * as Select from "$lib/components/ui/select";

  type Step = 1 | 2 | 3 | 4;

  const PROVIDER_OPTIONS: { value: ProviderKind; label: string }[] = [
    { value: "open_ai", label: "OpenAI" },
    { value: "open_router", label: "OpenRouter" },
    { value: "ollama", label: "Ollama" },
    { value: "lm_studio", label: "LM Studio" },
  ];

  const PROVIDER_DEFAULTS: Record<ProviderKind, Pick<ProviderConfig, "base_url" | "model">> = {
    open_ai: { base_url: "https://api.openai.com/v1", model: "gpt-4o-mini" },
    open_router: { base_url: "https://openrouter.ai/api/v1", model: "openai/gpt-4o-mini" },
    ollama: { base_url: "http://localhost:11434/v1", model: "llama3.1" },
    lm_studio: { base_url: "http://localhost:1234/v1", model: "default" },
  };

  let step = $state<Step>(1);

  let micState = $state<"idle" | "requesting" | "granted" | "denied">("idle");

  let downloadState = $state<"idle" | "downloading" | "error" | "done">("idle");
  let progress = $state(0);
  let downloadedMB = $state(0);
  let totalMB = $state(663);
  let downloadError = $state("");
  let downloadRate = $state<number | null>(null);
  let lastProgressTime = $state<number | null>(null);
  let lastDownloadedBytes = $state(0);

  let provider: ProviderConfig = $state({
    kind: "open_ai",
    base_url: PROVIDER_DEFAULTS.open_ai.base_url,
    model: PROVIDER_DEFAULTS.open_ai.model,
  });
  let apiKey = $state("");
  let providerSkipped = $state(false);
  let testState = $state<"idle" | "testing" | "success" | "error">("idle");
  let testError = $state("");
  let providerOperation = $state<"saving" | "testing" | null>(null);
  let providerGeneration = 0;
  let disposed = false;

  onDestroy(() => {
    disposed = true;
    providerGeneration++;
  });

  let busy = $state(false);

  async function requestMic() {
    micState = "requesting";
    try {
      const result = await checkMicrophonePermission();
      if (result === "Granted") {
        micState = "granted";
      } else if (result === "Denied") {
        micState = "denied";
      } else {
        micState = "denied";
      }
    } catch (err) {
      micState = "denied";
      console.error("[onboarding] mic permission check failed:", err);
    }
  }

  async function alreadyInstalled() {
    busy = true;
    downloadError = "";
    try {
      const present = await checkModelPack();
      if (present) {
        downloadState = "done";
        step = 3;
      } else {
        downloadError = "No valid model pack found in the model folder.";
        downloadState = "error";
      }
    } catch (err) {
      downloadError = String(err);
      downloadState = "error";
    } finally {
      busy = false;
    }
  }

  async function startDownload() {
    downloadState = "downloading";
    progress = 0;
    downloadedMB = 0;
    totalMB = 663;
    downloadError = "";
    downloadRate = null;
    lastProgressTime = null;
    lastDownloadedBytes = 0;

    try {
      await downloadModelPack((event: DownloadEvent) => {
        const now = performance.now();
        if (lastProgressTime !== null && lastProgressTime !== now) {
          const bytesDelta = event.overall_downloaded - lastDownloadedBytes;
          const seconds = (now - lastProgressTime) / 1000;
          if (seconds > 0) {
            downloadRate = bytesDelta / seconds;
          }
        }
        lastProgressTime = now;
        lastDownloadedBytes = event.overall_downloaded;

        progress = (event.overall_downloaded / event.overall_total) * 100;
        downloadedMB = Math.floor(event.overall_downloaded / 1_048_576);
        totalMB = Math.floor(event.overall_total / 1_048_576);
      });
      downloadState = "done";
    } catch (err) {
      downloadError = String(err);
      downloadState = "error";
    }
  }

  function invalidateProviderTest() {
    providerGeneration++;
    testState = "idle";
    testError = "";
  }

  function applyProviderPreset(kind: ProviderKind) {
    if (disposed || busy || providerOperation !== null) return;
    apiKey = "";
    provider.kind = kind;
    provider.base_url = PROVIDER_DEFAULTS[kind].base_url;
    provider.model = PROVIDER_DEFAULTS[kind].model;
    invalidateProviderTest();
  }

  function saveProvider() {
    return runProviderOperation("saving");
  }

  function skipProvider() {
    if (disposed || busy || providerOperation !== null) return;
    providerSkipped = true;
    nextStep();
  }

  function testProvider() {
    return runProviderOperation("testing");
  }

  async function runProviderOperation(operation: "saving" | "testing") {
    if (disposed || step !== 3 || busy || providerOperation !== null) return;
    providerOperation = operation;
    const generation = ++providerGeneration;
    const submittedConfig = { ...provider };
    const submittedKey = apiKey;
    const isCurrent = () => !disposed && generation === providerGeneration &&
      submittedConfig.kind === provider.kind &&
      submittedConfig.base_url === provider.base_url &&
      submittedConfig.model === provider.model && submittedKey === apiKey;
    testState = operation === "testing" ? "testing" : "idle";
    testError = "";
    try {
      await setLlmConfig(submittedConfig, submittedKey || undefined);
      if (!isCurrent()) return;
      if (operation === "testing") {
        await testLlmConfig();
        if (!isCurrent()) return;
        testState = "success";
      } else {
        providerSkipped = false;
        nextStep();
      }
    } catch (err) {
      if (!isCurrent()) return;
      testState = "error";
      testError = String(err);
    } finally {
      // Invalidating a result never releases this lock while IPC is pending.
      if (!disposed) {
        providerOperation = null;
        if (testState === "testing") testState = "idle";
      }
    }
  }

  function nextStep() {
    if (step < 4) step = ((step + 1) as Step);
  }

  async function finish() {
    busy = true;
    try {
      await setOnboardingComplete(true);
      goto("/");
    } finally {
      busy = false;
    }
  }

  function eta(): string {
    const remainingBytes = (totalMB - downloadedMB) * 1_048_576;
    if (!downloadRate || downloadRate <= 0 || remainingBytes <= 0) return "";
    const seconds = Math.max(1, Math.round(remainingBytes / downloadRate));
    if (seconds < 60) return `${seconds}s remaining`;
    const minutes = Math.floor(seconds / 60);
    const rest = seconds % 60;
    return `${minutes}m ${rest}s remaining`;
  }

  const stepLabel = $derived(step === 4 ? "Step 3 of 3" : `Step ${step} of 3`);
  const stepProgress = $derived(step === 4 ? 100 : ((step - 1) / 3) * 100);
</script>

<div class="onboarding-shell">
  <Card class="wizard">
    <CardContent class="wizard-inner">
      <header class="wizard-header">
        <div class="step-label">{stepLabel}</div>
        <Progress value={stepProgress} max={100} class="h-1" />
      </header>

      {#if step === 1}
        <section class="step">
          <h1 class="display">Microphone access</h1>
          <p class="subtitle">
            Kimi Nola needs permission to listen while you record. Audio stays on your machine; nothing is uploaded.
          </p>

          {#if micState === "idle" || micState === "requesting"}
            <div class="action-row">
              <Button onclick={requestMic} disabled={micState === "requesting" || busy}>
                {micState === "requesting" ? "Requesting…" : "Allow microphone"}
              </Button>
            </div>
          {:else if micState === "granted"}
            <div class="status-card success" role="status">
              <span>Microphone access granted.</span>
              <p class="hint">This checks permission only, not microphone volume. Check that your microphone is unmuted before recording.</p>
            </div>
            <Button onclick={nextStep}>Continue</Button>
          {:else if micState === "denied"}
            <div class="status-card error">
              <strong>Microphone access denied</strong>
              <p class="hint">Open Windows Privacy settings to enable the microphone, then try again.</p>
            </div>
            <div class="action-row">
              <Button variant="outline" onclick={() => openMicrophonePrivacySettings()}>
                Open privacy settings
              </Button>
              <Button onclick={requestMic}>Try again</Button>
            </div>
          {/if}
        </section>

      {:else if step === 2}
        <section class="step">
          <h1 class="display">Download the model pack</h1>
          <p class="subtitle">
            The Nemotron speech model runs entirely on your device. The first download is about {totalMB} MB.
          </p>

          {#if downloadState === "idle"}
            <div class="action-row">
              <Button onclick={startDownload} disabled={busy}>Download model pack</Button>
              <Button variant="outline" onclick={alreadyInstalled} disabled={busy}>
                Already installed
              </Button>
            </div>
          {:else if downloadState === "downloading"}
            <div class="progress-card">
              <div class="progress-row">
                <span>{Math.round(progress)}%</span>
                <span class="mono">{downloadedMB} / {totalMB} MB</span>
              </div>
              <Progress value={progress} max={100} class="h-2" />
              <p class="hint mono">
                {#if eta()}{eta()} · {/if}audio never leaves this machine
              </p>
            </div>
          {:else if downloadState === "error"}
            <div class="status-card error">
              <strong>Download failed</strong>
              <p class="hint">{downloadError || "Could not reach Hugging Face after retrying."}</p>
            </div>
            <div class="action-row">
              <Button onclick={startDownload} disabled={busy}>Retry</Button>
              <Button variant="outline" onclick={() => openModelFolder()} disabled={busy}>
                Open model folder
              </Button>
            </div>
          {:else if downloadState === "done"}
            <div class="status-card success">
              <span>Model pack verified and ready.</span>
            </div>
            <Button onclick={nextStep}>Continue</Button>
          {/if}
        </section>

      {:else if step === 3}
        <section class="step">
          <h1 class="display">AI Provider (optional)</h1>
          <p class="subtitle">
            Add a BYOK API key if you want to enhance your notes with a cloud LLM. You can skip this and add it later in Settings.
          </p>

          <div class="provider-form">
            <div class="field">
              <Label for="provider-kind">Provider</Label>
              <Select.Root
                type="single"
                disabled={busy || providerOperation !== null}
                value={provider.kind}
                onValueChange={(value) => applyProviderPreset(value as ProviderKind)}
              >
                <Select.Trigger id="provider-kind" class="w-full">
                  {PROVIDER_OPTIONS.find((o) => o.value === provider.kind)?.label ?? "Provider"}
                </Select.Trigger>
                <Select.Content>
                  {#each PROVIDER_OPTIONS as option (option.value)}
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
                disabled={busy || providerOperation !== null}
                bind:value={provider.base_url}
                oninput={() => { apiKey = ""; invalidateProviderTest(); }}
                placeholder="https://api.openai.com/v1"
              />
            </div>

            <div class="field">
              <Label for="provider-model">Model</Label>
              <Input
                id="provider-model"
                type="text"
                disabled={busy || providerOperation !== null}
                bind:value={provider.model}
                oninput={invalidateProviderTest}
                placeholder="gpt-4o-mini"
              />
            </div>

            <div class="field">
              <Label for="provider-key">API key</Label>
              <Input
                id="provider-key"
                type="password"
                disabled={busy || providerOperation !== null}
                bind:value={apiKey}
                oninput={invalidateProviderTest}
                placeholder="sk-…"
              />
            </div>

            {#if testState === "success"}
              <div class="status-card success">
                <span>Connection successful.</span>
              </div>
            {:else if testState === "error"}
              <div class="status-card error">
                <strong>Connection failed</strong>
                <p class="hint">{testError}</p>
              </div>
            {/if}
          </div>

          <div class="action-row">
            <Button onclick={saveProvider} disabled={busy || providerOperation !== null}>Save provider</Button>
            <Button variant="outline" onclick={testProvider} disabled={busy || providerOperation !== null}>
              {providerOperation === "testing" ? "Testing…" : "Test connection"}
            </Button>
            <Button variant="ghost" onclick={skipProvider} disabled={busy || providerOperation !== null}>Skip for now</Button>
          </div>
        </section>

      {:else if step === 4}
        <section class="step">
          <h1 class="display">You're all set</h1>
          <p class="subtitle">
            Model ready. Start your first meeting whenever you like.
          </p>
          <Button onclick={finish} disabled={busy}>Go to library</Button>
        </section>
      {/if}
    </CardContent>
  </Card>
</div>

<style>
  .onboarding-shell {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--canvas);
    color: var(--ink);
    padding: 40px 24px;
  }

  :global(.wizard) {
    width: min(520px, 100%);
  }

  :global(.wizard-inner) {
    display: flex;
    flex-direction: column;
    gap: 28px;
  }

  .wizard-header {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .step-label {
    font-family: var(--font-mono);
    font-size: 10.5px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .step {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .display {
    font-family: var(--font-display);
    font-size: clamp(28px, 4vw, 40px);
    font-weight: 400;
    line-height: 1.15;
    color: var(--ink-strong);
    margin: 0;
  }

  .subtitle {
    color: var(--text-muted);
    font-size: 15px;
    line-height: 1.65;
    margin: 0;
  }

  .hint {
    margin: 0;
    color: var(--soft);
    font-size: 12.5px;
  }

  .status-card {
    padding: 14px 16px;
    border-radius: var(--radius-card);
    font-size: 14px;
  }
  .status-card.success {
    background: var(--brand-soft);
    color: var(--brand-deep);
  }
  .status-card.error {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .progress-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: var(--surface);
    border: 1px solid var(--hairline-soft);
    border-radius: var(--radius-card);
    padding: 18px;
  }
  .progress-row {
    display: flex;
    justify-content: space-between;
    font-size: 14px;
    color: var(--ink-strong);
  }

  .provider-form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .action-row {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
  }

  .mono {
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-size: 11px;
  }
</style>
