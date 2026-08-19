<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  const isDev = import.meta.env.DEV;
  const variantKeys = ["A", "B", "C"] as const;
  type VariantKey = (typeof variantKeys)[number];
  type DemoStage = "detected" | "notes" | "recording" | "dismissed";
  type PromptAction = "jot" | "record" | "dismiss";

  const variantNames: Record<VariantKey, string> = {
    A: "Native-toast feel",
    B: "In-app banner",
    C: "Notes-first rail",
  };

  const stageLabels: Record<DemoStage, string> = {
    detected: "Prompt waiting",
    notes: "Note draft open",
    recording: "Recording explicitly started",
    dismissed: "Prompt dismissed",
  };

  let activeVariant = $state<VariantKey>("A");
  let stage = $state<DemoStage>("detected");
  let noteBody = $state("");
  let updatedAt = $state("just now");

  $effect(() => {
    const requested = page.url.searchParams.get("variant")?.toUpperCase();
    activeVariant = isVariant(requested) ? requested : "A";
  });

  function isVariant(value: string | undefined): value is VariantKey {
    return value === "A" || value === "B" || value === "C";
  }

  function updateTimestamp() {
    updatedAt = new Intl.DateTimeFormat("en-US", {
      hour: "numeric",
      minute: "2-digit",
      second: "2-digit",
    }).format(new Date());
  }

  function handleAction(action: PromptAction) {
    updateTimestamp();
    if (action === "jot") {
      stage = "notes";
      return;
    }
    if (action === "record") {
      stage = "recording";
      return;
    }
    stage = "dismissed";
  }

  function resetDemo() {
    stage = "detected";
    noteBody = "";
    updateTimestamp();
  }

  function cycleVariant(delta: number) {
    const current = variantKeys.indexOf(activeVariant);
    const next = (current + delta + variantKeys.length) % variantKeys.length;
    const key = variantKeys[next];
    void goto(`/prototype/meeting-presence?variant=${key}`, {
      replaceState: true,
      keepFocus: true,
      noScroll: true,
    });
  }

  function handleKeydown(event: KeyboardEvent) {
    const target = event.target;
    if (
      target instanceof HTMLElement &&
      target.matches("input, textarea, select, [contenteditable='true']")
    ) {
      return;
    }
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      cycleVariant(-1);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      cycleVariant(1);
    }
  }
</script>

<svelte:head>
  <title>Prototype — Meeting presence | Kimi Nola</title>
</svelte:head>

<svelte:window onkeydown={handleKeydown} />

{#if !isDev}
  <section class="prototype-blocked">
    <div class="prototype-kicker">THROWAWAY PROTOTYPE</div>
    <h1>Only available in development</h1>
    <p>This route is intentionally hidden from production builds.</p>
  </section>
{:else}
  <section class="prototype-page">
    <div class="prototype-kicker">THROWAWAY PROTOTYPE · MEETING PRESENCE</div>
    <div class="prototype-heading">
      <div>
        <h1>How should the “meeting detected” moment feel?</h1>
        <p>
          Three interaction shapes. Nothing here touches the detector, microphone, loopback,
          recording pipeline, or database.
        </p>
      </div>
      <button class="reset-button" type="button" onclick={resetDemo}>Reset demo</button>
    </div>

    <div class="demo-frame" class:frame-detected={stage === "detected"}>
      <div class="frame-topline">
        <span class="frame-app-name">Kimi Nola</span>
        <span class="frame-context">Detection preview · local only</span>
      </div>

      {#if stage === "detected"}
        {#if activeVariant === "A"}
          <!-- Variant A: native-toast feel -->
          <div class="desktop-stage variant-a">
            <div class="desktop-lines" aria-hidden="true">
              <span></span><span></span><span></span><span></span>
              <span></span><span></span><span></span>
            </div>
            <div class="toast-card" role="status" aria-live="polite">
              <div class="toast-topline">
                <span class="toast-app">KIMI NOLA</span>
                <span class="toast-time">now</span>
              </div>
              <div class="presence-mark" aria-hidden="true">·</div>
              <div class="toast-copy">
                <div class="toast-kicker">Meeting presence</div>
                <h2>You may be in a meeting.</h2>
                <p>Granola appears open and its audio session is active.</p>
                <p class="privacy-line">Kimi Nola is not recording.</p>
              </div>
              <div class="toast-actions">
                <button class="action action-primary" type="button" onclick={() => handleAction("jot")}>Jot notes</button>
                <button class="action action-secondary" type="button" onclick={() => handleAction("record")}>Start recording</button>
                <button class="action action-quiet" type="button" onclick={() => handleAction("dismiss")}>Not now</button>
              </div>
            </div>
          </div>
        {:else if activeVariant === "B"}
          <!-- Variant B: in-app banner -->
          <div class="notes-stage variant-b">
            <div class="notes-stage-header">
              <div>
                <div class="notes-eyebrow">My notes</div>
                <h2>Untitled note draft</h2>
              </div>
              <span class="draft-status">Autosaves</span>
            </div>
            <div class="notes-paper" aria-hidden="true">
              <span class="paper-line paper-line-long"></span>
              <span class="paper-line"></span>
              <span class="paper-line paper-line-short"></span>
            </div>
            <div class="banner-card" role="status" aria-live="polite">
              <div class="banner-mark" aria-hidden="true">·</div>
              <div class="banner-copy">
                <div class="toast-kicker">Possible meeting detected</div>
                <h2>Want to jot notes?</h2>
                <p>Granola is open. Your microphone and system audio are still untouched.</p>
              </div>
              <div class="banner-actions">
                <button class="action action-primary" type="button" onclick={() => handleAction("jot")}>Jot notes</button>
                <button class="action action-secondary" type="button" onclick={() => handleAction("record")}>Start recording</button>
                <button class="action action-quiet" type="button" onclick={() => handleAction("dismiss")}>Not now</button>
              </div>
            </div>
          </div>
        {:else}
          <!-- Variant C: notes-first rail -->
          <div class="workspace-stage variant-c">
            <div class="workspace-copy">
              <div class="notes-eyebrow">Today</div>
              <h2>Keep the thought while it is fresh.</h2>
              <p>Open a small note now. Decide about recording only when you are ready.</p>
              <div class="workspace-signal">
                <span class="signal-dot" aria-hidden="true"></span>
                <span>Possible meeting · Granola + active audio session</span>
              </div>
            </div>
            <aside class="note-rail" aria-label="Meeting presence prompt">
              <div class="rail-topline">
                <span class="toast-kicker">Kimi Nola</span>
                <span class="rail-pill">not recording</span>
              </div>
              <h2>Jot something down?</h2>
              <p>You can start a full Meeting later from this note.</p>
              <button class="action action-primary rail-action" type="button" onclick={() => handleAction("jot")}>Jot notes</button>
              <button class="action action-secondary rail-action" type="button" onclick={() => handleAction("record")}>Start recording</button>
              <button class="action action-quiet rail-action" type="button" onclick={() => handleAction("dismiss")}>Not now</button>
            </aside>
          </div>
        {/if}
      {:else if stage === "notes"}
        <div class="result-stage result-notes">
          <div class="result-icon" aria-hidden="true">✦</div>
          <div class="result-copy">
            <div class="toast-kicker">Note draft · autosaving</div>
            <h2>Jot notes without recording.</h2>
            <p>This draft survives the prototype reset only until you reload the page.</p>
          </div>
          <textarea bind:value={noteBody} aria-label="Prototype note draft" placeholder="Type a quick thought…"></textarea>
          <div class="result-actions">
            <button class="action action-primary" type="button" onclick={() => handleAction("record")}>Start recording from this note</button>
            <button class="action action-quiet" type="button" onclick={resetDemo}>Back to prompt</button>
          </div>
        </div>
      {:else if stage === "recording"}
        <div class="result-stage result-recording">
          <div class="recording-orbit" aria-hidden="true"><span></span></div>
          <div class="result-copy">
            <div class="toast-kicker">Explicit action received</div>
            <h2>Meeting recording would start here.</h2>
            <p>The prototype changes state only. No microphone, loopback, or ASR process was opened.</p>
          </div>
          <div class="result-actions">
            <button class="action action-primary" type="button" onclick={() => (stage = "notes")}>Stop preview</button>
            <button class="action action-quiet" type="button" onclick={resetDemo}>Back to prompt</button>
          </div>
        </div>
      {:else}
        <div class="result-stage result-dismissed">
          <div class="dismiss-mark" aria-hidden="true">—</div>
          <div class="result-copy">
            <div class="toast-kicker">Prompt dismissed</div>
            <h2>No audio was captured.</h2>
            <p>In the real flow, “Not now” would quiet this app session.</p>
          </div>
          <button class="action action-secondary" type="button" onclick={resetDemo}>Re-arm demo</button>
        </div>
      {/if}
    </div>

    <div class="state-panel" aria-live="polite">
      <div class="state-heading">
        <div>
          <div class="toast-kicker">Prototype state</div>
          <h2>{stageLabels[stage]}</h2>
        </div>
        <span class="state-chip">in memory only</span>
      </div>
      <div class="state-grid">
        <div>
          <span>Detector</span>
          <strong>{stage === "detected" ? "possible → likely" : "hint resolved"}</strong>
        </div>
        <div>
          <span>Capture</span>
          <strong>{stage === "recording" ? "explicitly started" : "not started"}</strong>
        </div>
        <div>
          <span>Prompt</span>
          <strong>{stage === "dismissed" ? "suppressed" : stage}</strong>
        </div>
        <div>
          <span>Last action</span>
          <strong>{updatedAt}</strong>
        </div>
      </div>
    </div>

    <nav class="prototype-switcher" aria-label="Prototype variants">
      <button type="button" aria-label="Previous variant" onclick={() => cycleVariant(-1)}>←</button>
      <span><strong>{activeVariant}</strong> · {variantNames[activeVariant]}</span>
      <button type="button" aria-label="Next variant" onclick={() => cycleVariant(1)}>→</button>
    </nav>
  </section>
{/if}

<style>
  .prototype-page {
    width: min(1040px, calc(100% - 72px));
    margin: 0 auto;
    padding: 32px 0 132px;
  }

  .prototype-kicker,
  .toast-kicker,
  .notes-eyebrow {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 500;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .prototype-heading {
    align-items: flex-start;
    display: flex;
    gap: 24px;
    justify-content: space-between;
    margin: 10px 0 24px;
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  h1 {
    color: var(--ink-strong);
    font-family: var(--font-display);
    font-size: clamp(28px, 4vw, 42px);
    font-weight: 400;
    letter-spacing: -0.02em;
    line-height: 1.08;
    max-width: 660px;
  }

  .prototype-heading p {
    color: var(--text-muted);
    font-size: 14px;
    margin-top: 10px;
    max-width: 660px;
  }

  .reset-button {
    background: transparent;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-pill);
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    font: inherit;
    font-size: 13px;
    padding: 8px 14px;
  }

  .reset-button:hover {
    border-color: var(--ink);
    color: var(--ink);
  }

  .demo-frame {
    background: var(--surface-soft);
    border: 1px solid var(--hairline);
    border-radius: 20px;
    box-shadow: 0 14px 34px var(--shadow-paper);
    min-height: 500px;
    overflow: hidden;
  }

  .frame-topline {
    align-items: center;
    border-bottom: 1px solid var(--hairline-soft);
    display: flex;
    justify-content: space-between;
    padding: 13px 18px;
  }

  .frame-app-name {
    color: var(--ink-strong);
    font-family: var(--font-display);
    font-size: 18px;
  }

  .frame-context {
    color: var(--soft);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .desktop-stage {
    align-items: flex-end;
    background: var(--canvas);
    display: flex;
    min-height: 458px;
    overflow: hidden;
    padding: 32px;
    position: relative;
  }

  .desktop-lines {
    display: flex;
    flex-direction: column;
    gap: 16px;
    left: 10%;
    opacity: 0.38;
    position: absolute;
    right: 30%;
    top: 17%;
  }

  .desktop-lines span {
    background: var(--surface);
    border: 1px solid var(--hairline-soft);
    border-radius: 8px;
    display: block;
    height: 22px;
  }

  .desktop-lines span:nth-child(2) { width: 75%; }
  .desktop-lines span:nth-child(3) { width: 86%; }
  .desktop-lines span:nth-child(4) { margin-top: 22px; width: 58%; }
  .desktop-lines span:nth-child(5) { width: 80%; }
  .desktop-lines span:nth-child(6) { width: 67%; }
  .desktop-lines span:nth-child(7) { width: 48%; }

  .toast-card {
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: 16px;
    box-shadow: 0 12px 30px var(--shadow-paper);
    margin-left: auto;
    max-width: 390px;
    padding: 18px;
    position: relative;
    width: 100%;
    z-index: 1;
  }

  .toast-topline,
  .rail-topline,
  .state-heading,
  .notes-stage-header {
    align-items: center;
    display: flex;
    justify-content: space-between;
  }

  .toast-app,
  .toast-time {
    color: var(--soft);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .toast-copy {
    padding: 14px 0 16px 30px;
  }

  .toast-copy h2,
  .banner-copy h2,
  .note-rail h2,
  .result-copy h2,
  .state-heading h2 {
    color: var(--ink-strong);
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.2;
    margin-top: 5px;
  }

  .toast-copy p,
  .banner-copy p,
  .note-rail p,
  .result-copy p {
    color: var(--text-muted);
    font-size: 14px;
    line-height: 1.45;
    margin-top: 8px;
  }

  .toast-copy .privacy-line {
    color: var(--brand-deep);
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.04em;
  }

  .presence-mark,
  .banner-mark,
  .result-icon {
    align-items: center;
    background: var(--brand);
    border-radius: 50%;
    color: var(--canvas);
    display: flex;
    flex-shrink: 0;
    font-size: 28px;
    height: 20px;
    justify-content: center;
    line-height: 1;
    position: absolute;
    width: 20px;
  }

  .presence-mark { margin-top: 2px; }

  .toast-actions,
  .banner-actions,
  .result-actions {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .action {
    border: 1px solid transparent;
    border-radius: var(--radius-pill);
    cursor: pointer;
    font: inherit;
    font-size: 13px;
    padding: 8px 13px;
    transition: background 180ms ease, border-color 180ms ease, color 180ms ease;
  }

  .action-primary {
    background: var(--ink);
    color: var(--canvas);
  }

  .action-primary:hover {
    background: transparent;
    border-color: var(--ink);
    color: var(--ink);
  }

  .action-secondary {
    background: var(--brand-soft);
    color: var(--brand-deep);
  }

  .action-secondary:hover { background: var(--brand-softer); }

  .action-quiet {
    background: transparent;
    border-color: var(--hairline);
    color: var(--text-muted);
  }

  .action-quiet:hover {
    border-color: var(--ink);
    color: var(--ink);
  }

  .notes-stage {
    background: var(--canvas);
    min-height: 458px;
    padding: 32px;
    position: relative;
  }

  .notes-stage-header {
    align-items: flex-start;
    max-width: 650px;
  }

  .notes-stage-header h2 {
    color: var(--ink-strong);
    font-family: var(--font-display);
    font-size: 30px;
    font-weight: 400;
    line-height: 1.15;
    margin-top: 6px;
  }

  .draft-status,
  .rail-pill,
  .state-chip {
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-pill);
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.07em;
    padding: 6px 9px;
    text-transform: uppercase;
  }

  .notes-paper {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-top: 40px;
    max-width: 570px;
    opacity: 0.5;
  }

  .paper-line {
    background: var(--surface);
    border-radius: 4px;
    display: block;
    height: 10px;
    width: 92%;
  }

  .paper-line-long { width: 100%; }
  .paper-line-short { width: 62%; }

  .banner-card {
    align-items: flex-start;
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-left: 3px solid var(--brand);
    border-radius: 14px;
    bottom: 30px;
    box-shadow: 0 10px 26px var(--shadow-paper);
    display: grid;
    gap: 16px;
    grid-template-columns: 20px 1fr auto;
    left: 32px;
    padding: 18px 20px;
    position: absolute;
    right: 32px;
  }

  .banner-mark {
    position: static;
  }

  .banner-actions {
    justify-content: flex-end;
    max-width: 250px;
  }

  .workspace-stage {
    background: var(--canvas);
    display: grid;
    gap: 36px;
    grid-template-columns: 1fr 340px;
    min-height: 458px;
    padding: 50px;
  }

  .workspace-copy {
    align-self: center;
    max-width: 430px;
  }

  .workspace-copy h2 {
    color: var(--ink-strong);
    font-family: var(--font-display);
    font-size: clamp(30px, 4vw, 48px);
    font-weight: 400;
    line-height: 1.05;
    margin-top: 10px;
  }

  .workspace-copy p {
    color: var(--text-muted);
    font-size: 16px;
    margin-top: 16px;
    max-width: 350px;
  }

  .workspace-signal {
    align-items: center;
    color: var(--text-muted);
    display: flex;
    font-family: var(--font-mono);
    font-size: 11px;
    gap: 8px;
    margin-top: 32px;
  }

  .signal-dot {
    background: var(--brand);
    border-radius: 50%;
    height: 8px;
    width: 8px;
  }

  .note-rail {
    align-self: center;
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: 18px;
    box-shadow: 0 12px 28px var(--shadow-paper);
    padding: 22px;
  }

  .note-rail h2 { margin-top: 28px; }
  .rail-pill { color: var(--brand-deep); }
  .rail-action { display: flex; justify-content: center; margin-top: 10px; width: 100%; }

  .result-stage {
    align-items: center;
    background: var(--canvas);
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-height: 458px;
    padding: 40px;
    position: relative;
    text-align: center;
  }

  .result-stage .result-icon,
  .result-stage .recording-orbit,
  .result-stage .dismiss-mark {
    margin-bottom: 20px;
    position: static;
  }

  .result-stage .result-icon {
    font-size: 20px;
    height: 38px;
    width: 38px;
  }

  .result-copy { max-width: 500px; }

  .result-stage textarea {
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: 12px;
    color: var(--ink);
    font: inherit;
    line-height: 1.55;
    margin-top: 24px;
    min-height: 120px;
    outline: none;
    padding: 14px;
    resize: vertical;
    text-align: left;
    width: min(520px, 100%);
  }

  .result-stage textarea:focus { border-color: var(--brand); }
  .result-actions { justify-content: center; margin-top: 22px; }

  .recording-orbit {
    align-items: center;
    border: 1px solid var(--brand);
    border-radius: 50%;
    display: flex;
    height: 38px;
    justify-content: center;
    width: 38px;
  }

  .recording-orbit span {
    background: var(--brand);
    border-radius: 50%;
    height: 12px;
    width: 12px;
  }

  .dismiss-mark {
    color: var(--soft);
    font-family: var(--font-display);
    font-size: 44px;
    line-height: 1;
  }

  .state-panel {
    background: var(--surface);
    border: 1px solid var(--hairline);
    border-radius: 16px;
    margin-top: 16px;
    padding: 18px 20px;
  }

  .state-heading h2 {
    font-family: var(--font-mono);
    font-size: 14px;
    letter-spacing: 0.02em;
    margin-top: 3px;
    text-transform: uppercase;
  }

  .state-grid {
    border-top: 1px solid var(--hairline-soft);
    display: grid;
    gap: 14px;
    grid-template-columns: repeat(4, 1fr);
    margin-top: 16px;
    padding-top: 14px;
  }

  .state-grid div { min-width: 0; }

  .state-grid span {
    color: var(--soft);
    display: block;
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .state-grid strong {
    color: var(--ink-strong);
    display: block;
    font-size: 13px;
    font-weight: 500;
    margin-top: 5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .prototype-switcher {
    align-items: center;
    background: var(--ink);
    border-radius: var(--radius-pill);
    bottom: 22px;
    box-shadow: 0 8px 24px var(--shadow-paper);
    color: var(--canvas);
    display: flex;
    gap: 14px;
    left: calc(var(--sidebar-width) + (100vw - var(--sidebar-width)) / 2);
    padding: 7px 9px;
    position: fixed;
    transform: translateX(-50%);
    z-index: 950;
  }

  .prototype-switcher button {
    align-items: center;
    background: transparent;
    border: 1px solid color-mix(in srgb, var(--canvas) 28%, transparent);
    border-radius: 50%;
    color: var(--canvas);
    cursor: pointer;
    display: flex;
    font-size: 17px;
    height: 28px;
    justify-content: center;
    line-height: 1;
    width: 28px;
  }

  .prototype-switcher button:hover { background: color-mix(in srgb, var(--canvas) 14%, transparent); }

  .prototype-switcher span {
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.04em;
    min-width: 160px;
    text-align: center;
  }

  .prototype-switcher strong { color: var(--brand); }

  .prototype-blocked {
    margin: 80px auto;
    max-width: 560px;
    padding: 32px;
    text-align: center;
  }

  .prototype-blocked h1 {
    font-size: 34px;
    margin-top: 10px;
  }

  .prototype-blocked p {
    color: var(--text-muted);
    margin-top: 10px;
  }

  @media (max-width: 780px) {
    .prototype-page { width: min(100% - 32px, 620px); }
    .prototype-heading { flex-direction: column; }
    .desktop-stage, .notes-stage { padding: 22px; }
    .toast-card { margin-left: 0; }
    .banner-card { bottom: 22px; left: 22px; right: 22px; }
    .banner-card, .workspace-stage { grid-template-columns: 1fr; }
    .banner-actions { justify-content: flex-start; max-width: none; }
    .workspace-stage { gap: 22px; padding: 30px 22px; }
    .state-grid { grid-template-columns: repeat(2, 1fr); }
    .prototype-switcher { left: 50%; }
  }
</style>
