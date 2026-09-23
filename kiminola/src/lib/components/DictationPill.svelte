<script lang="ts">
  import { onMount } from "svelte";
  import Mic from "@lucide/svelte/icons/mic";
  import DictationActiveControls from "./DictationActiveControls.svelte";
  import FileText from "@lucide/svelte/icons/file-text";
  import { createDictationController } from "$lib/dictation-controller";
  import { getDictationState, onDictationState } from "$lib/dictation-ipc";
  import type { DictationSnapshot } from "$lib/dictation-types";

  let snapshot = $state<DictationSnapshot | null>(null);
  let error = $state("");
  let displayError = $derived(error || snapshot?.error || "");

  let controller: ReturnType<typeof createDictationController>;
  const labels = { disabled: "Off", idle: "Ready", starting: "Start", listening: "Talk", processing: "Wait", review: "Review" };
  let phase = $derived(snapshot?.phase ?? "disabled");
  let active = $derived(phase === "starting" || phase === "listening" || phase === "processing");
  let level = $derived(Math.max(0, Math.min(1, snapshot?.level ?? 0)));
  let elapsed = $derived(Math.max(0, Math.floor(snapshot?.elapsed_seconds ?? 0)));
  let time = $derived(`${String(Math.floor(elapsed / 60)).padStart(2, "0")}:${String(elapsed % 60).padStart(2, "0")}`);
  const rhythm = [0.25, 0.46, 0.35, 0.66, 0.87, 0.52, 0.73, 0.38, 0.25, 0.45, 0.61, 0.3];
  onMount(() => {
    const root = document.documentElement;
    const oldRoot = root.style.background;
    const oldBody = document.body.style.background;
    root.style.background = "transparent";
    document.body.style.background = "transparent";
    controller = createDictationController({ get: getDictationState, listen: onDictationState }, view => { snapshot = view.snapshot; error = view.error; });
    void controller.connect();
    return () => { controller.dispose(); root.style.background = oldRoot; document.body.style.background = oldBody; };
  });
</script>

<div class="edge" class:left={snapshot?.settings.side === "left"}>
  <section class="pill" aria-label="Dictation pill" data-phase={phase} title={displayError || (phase === "review" ? "Review text in the main window's Dictation settings." : `Dictation ${phase}. ${snapshot?.settings.shortcut ?? ""}`)}>
    <span class="state" role="status" aria-live="polite" aria-atomic="true"><span>{displayError ? "Error" : labels[phase]}</span>{#if displayError}<span class="sr-only">: {displayError}</span>{/if}</span>
    {#if phase === "listening"}
      <div class="wave" role="meter" aria-label="Microphone activity" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(level * 100)}>
        {#each rhythm as weight, i}<span class:gold={i === 4} style:height={`${3 + weight * level * 25}px`}></span>{/each}
      </div>
    {:else}<div class="mark" aria-hidden="true">{#if phase === "review"}<FileText size={20} />{:else}<Mic size={20} />{/if}</div>{/if}
    {#if active}<span class="timer" aria-label={`Elapsed ${time}`}>{time}</span>{/if}
    {#if active && snapshot}<DictationActiveControls {snapshot} refresh={() => controller.refresh()} compact />{/if}
    <span class="sr-only">{phase === "review" ? "Open the main window and choose Dictation in Settings to copy or discard text." : phase === "idle" ? "Use the configured shortcut to start. The microphone is off." : `Dictation ${phase}.`}</span>
  </section>
</div>

<style>
  .edge { position: fixed; inset: 0; display: flex; align-items: center; justify-content: flex-end; padding: 8px; overflow: hidden; }
  .edge.left { justify-content: flex-start; }
  .pill { width: 48px; max-height: 204px; padding: 14px 5px; display: flex; flex: none; flex-direction: column; align-items: center; gap: 10px; background: var(--canvas); border: 1px solid var(--hairline); border-radius: 24px; color: var(--ink); }
  .state { font: 500 9px/1.2 var(--font-mono); text-transform: uppercase; letter-spacing: .02em; }
  .mark { display: grid; place-items: center; color: var(--brand); height: 30px; }
  .wave { height: 30px; display: flex; gap: 1px; align-items: center; width: 35px; }
  .wave span { width: 2px; flex: none; min-height: 3px; background: var(--ink); border-radius: 2px; }
  .wave .gold { background: var(--brand); }
  .timer { font: 400 9px/1.2 var(--font-mono); color: var(--text-muted); }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
  @media (prefers-reduced-motion: reduce) { * { animation: none !important; transition: none !important; } }
</style>
