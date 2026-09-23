<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Switch } from "$lib/components/ui/switch";
  import DictationRecovery from "./DictationRecovery.svelte";
  import DictationActiveControls from "./DictationActiveControls.svelte";
  import { createDictationController } from "$lib/dictation-controller";
  import { getDictationState, onDictationState, setDictationSettings, listDictationMicrophones } from "$lib/dictation-ipc";
  import type { DictationSettings, DictationSnapshot, DictationMicrophone } from "$lib/dictation-types";
  import { sameDictationSettings } from "$lib/dictation-types";

  let { dirty = $bindable(false), busy = $bindable(false) }: { dirty?: boolean; busy?: boolean } = $props();
  let snapshot = $state<DictationSnapshot | null>(null);
  let draft = $state<DictationSettings | null>(null);
  let baseline = $state<DictationSettings | null>(null);
  let consentProvider = $state(false);
  let error = $state("");
  let readError = $state("");
  let status = $state("");
  let microphones = $state<DictationMicrophone[]>([]);
  let microphoneError = $state("");
  let disposed = false;
  let controller: ReturnType<typeof createDictationController>;
  let hasEdits = $derived(!!draft && (!sameDictationSettings(draft, baseline) || consentProvider));
  let disabling = $derived(!!snapshot?.settings.enabled && draft?.enabled === false);
  $effect(() => { dirty = hasEdits; });

  async function loadMicrophones() {
    microphoneError = "";
    try { const next = await listDictationMicrophones(); if (!disposed) microphones = next; }
    catch (err) { if (!disposed) microphoneError = String(err); }
  }
  onMount(() => {
    controller = createDictationController({ get: getDictationState, listen: onDictationState }, view => {
      snapshot = view.snapshot;
      readError = view.error;
      if (view.snapshot && (!draft || (!hasEdits && !busy))) {
        draft = { ...view.snapshot.settings };
        baseline = { ...draft };
      } else if (view.snapshot && draft && sameDictationSettings(view.snapshot.settings, draft)) {
        // A pending Disable is committed only after recovery is resolved.
        // Acknowledge matching persisted values without replacing other edits.
        baseline = { ...draft };
        if (status && !view.snapshot.exit_intent) status = "Dictation settings saved.";
      }
    });
    void controller.connect();
    void loadMicrophones();
    return () => { disposed = true; controller.dispose(); dirty = false; busy = false; };
  });
  async function save() {
    if (!draft || busy) return;
    const input = { ...draft, consent_provider: !disabling && draft.cleanup === "provider" && consentProvider };
    busy = true; error = ""; status = "";
    try {
      // Command replies may arrive after a newer event. Only a fresh read can
      // acknowledge the visible settings; never overwrite a failed draft.
      await setDictationSettings(input);
      if (disposed) return;
      await controller.refresh();
      if (disposed) return;
      if (snapshot && !readError && sameDictationSettings(snapshot.settings, draft)) {
        baseline = { ...draft }; consentProvider = false;
        status = "Dictation settings saved.";
      } else {
        status = "Settings submitted. Review the current state before saving again.";
      }
    } catch (err) { if (!disposed) error = String(err); }
    finally { if (!disposed) busy = false; }
  }
</script>

<section class="settings-card dictation-settings" aria-labelledby="dictation-heading" aria-busy={busy}>
  <header><h2 id="dictation-heading">Dictation</h2><p>Speak into an editor using the on-device speech model. Microphone audio stays on this machine.</p></header>
  {#if snapshot?.phase === "review"}<DictationRecovery {snapshot} refresh={() => controller.refresh()} />
  {:else if snapshot?.error}<p role="alert">{snapshot.error}</p>{/if}
  {#if snapshot && ["starting", "listening", "processing"].includes(snapshot.phase)}
    <p role="status">{snapshot.phase === "starting" ? "Starting microphone…" : snapshot.phase === "listening" ? "Dictation is listening." : "Processing dictation. You can still cancel."}</p>
    <DictationActiveControls {snapshot} refresh={() => controller.refresh()} />
  {:else if snapshot?.phase === "idle" && !snapshot.error && !readError}<p>Ready. Focus an editor and use <kbd>{snapshot.settings.shortcut}</kbd> to dictate.</p>{/if}
  {#if readError}<div role="alert">Could not refresh dictation: {readError} <Button variant="outline" onclick={() => controller.refresh()}>Retry dictation state</Button></div>{/if}
  {#if draft}
    <form onsubmit={(event) => { event.preventDefault(); void save(); }}>
      <fieldset disabled={busy}>
        <div class="enable-row"><div><strong>Enable dictation</strong><p>Keep the shortcut available when the main window closes. This does not start recording or enable startup with Windows.</p></div><Switch aria-label="Enable dictation" bind:checked={draft.enabled} /></div>
        <div class="field-grid">
          <label>Activation<select aria-label="Activation" bind:value={draft.activation}><option value="hold">Hold to talk</option><option value="toggle">Press to start / stop</option></select></label>
          <label>Pill side<select aria-label="Pill side" bind:value={draft.side}><option value="left">Left edge</option><option value="right">Right edge</option></select></label>
          <label class="full">Dictation shortcut<input bind:value={draft.shortcut} required autocomplete="off" spellcheck="false" aria-describedby="dictation-shortcut-help" /></label>
          <p id="dictation-shortcut-help" class="full">Hold mode stops when any key in the chord is released. The Meeting shortcut must be different.</p>
          <label class="full">Microphone<select aria-label="Microphone" bind:value={draft.microphone_id}><option value={null}>Windows default microphone</option>{#each microphones as mic (mic.id)}<option value={mic.id}>{mic.name}</option>{/each}{#if draft.microphone_id && !microphones.some(mic => mic.id === draft?.microphone_id)}<option value={draft.microphone_id}>Selected microphone unavailable</option>{/if}</select></label>
        </div>
        <p>The selected microphone is fixed for each session. No idle capture or system audio. Sessions stop after ten minutes; interrupted text stays available for review.</p>
        {#if microphoneError}<div role="alert">Microphones unavailable: {microphoneError} <Button type="button" variant="outline" onclick={loadMicrophones}>Refresh microphones</Button></div>{/if}
        <label>Text cleanup<select aria-label="Text cleanup" bind:value={draft.cleanup}><option value="raw">Raw transcript, on device</option><option value="provider">Configured AI provider, text only</option></select></label>
        {#if draft.cleanup === "provider"}
          <div class="consent-note"><p>Only dictated text goes to the provider configured in <a href="/settings?section=ai">AI provider</a>. No audio or surrounding app content. Meeting-note consent does not authorize dictation.</p>
            {#if snapshot?.provider_authorized}<p>Dictation permission is saved for the current provider endpoint. A changed endpoint needs new permission.</p>
            {:else}<label class="check-row"><input type="checkbox" bind:checked={consentProvider} />Allow dictation text to be sent to the configured provider</label>{/if}
          </div>
        {/if}
        <label class="check-row"><input type="checkbox" bind:checked={draft.clipboard_consent} />Allow guarded paste using the clipboard</label>
        <p>Automatic paste is disabled in this build because no application/version is qualified yet. Final text stays in review; choose Copy when ready. Copy replaces your clipboard without restoring its previous contents. Windows history/cloud exclusion is requested, but other clipboard tools may still read it. Password fields, elevated apps, terminals, and uncertain targets never receive automatic paste. Kimi Nola never presses Enter.</p>
        <label class="check-row"><input type="checkbox" bind:checked={draft.history_enabled} />Save completed dictation history for 30 days</label>
        <p>Off by default. Final text only, never audio or failed attempts. Turning this off stops new saves; existing entries remain until expiry or deletion.</p>
        <div class="actions"><Button type="submit" disabled={!dirty || busy || (!disabling && draft.cleanup === "provider" && !snapshot?.provider_authorized && !consentProvider)}>{busy ? "Saving…" : "Save dictation settings"}</Button>{#if dirty}<span>Unsaved changes</span>{/if}</div>
      </fieldset>
    </form>
  {:else if !readError}<p role="status">Loading dictation settings…</p>{/if}
  {#if error}<p role="alert">Could not save dictation settings: {error}</p>{/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>

<style>
  .dictation-settings { display: grid; gap: 24px; color: var(--ink); }
  h2 { font-family: var(--font-display); font-size: 27px; line-height: 1.2; margin: 0 0 8px; }
  p { color: var(--text-muted); font-size: 14px; line-height: 1.65; margin: 0; }
  fieldset { display: grid; gap: 18px; border: 0; padding: 0; margin: 0; min-width: 0; }
  .enable-row { display: flex; align-items: center; gap: 24px; justify-content: space-between; }
  .enable-row p { margin-top: 4px; }
  .field-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
  .full { grid-column: 1 / -1; }
  label { display: grid; gap: 8px; font-size: 14px; font-weight: 500; }
  select, input:not([type="checkbox"]) { min-height: 40px; width: 100%; min-width: 0; border: 1px solid var(--hairline); border-radius: var(--radius-input); background: var(--canvas); color: var(--ink); padding: 8px 12px; font: inherit; }
  .check-row { display: flex; gap: 10px; align-items: flex-start; }
  input[type="checkbox"] { accent-color: var(--ink); width: 18px; height: 18px; flex: none; margin-top: 2px; }
  .consent-note { display: grid; gap: 12px; padding: 16px; background: var(--surface-soft); border-radius: var(--radius-input); }
  a { text-decoration: underline; color: inherit; }
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; }
  .actions span { font-size: 13px; color: var(--text-muted); }
  :focus-visible { outline: 2px solid var(--ink); outline-offset: 3px; }
  @media (max-width: 600px) { .field-grid { grid-template-columns: 1fr; } .enable-row { gap: 12px; } }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; animation: none !important; } }
</style>
