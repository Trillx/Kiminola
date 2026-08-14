# Kiminola — Wayfinder Map

## Destination

A **build-ready spec** for Kiminola: an open-source, cross-platform (Windows-first, including ARM64) Granola alternative. Core loop: manual-start meeting capture (mic + system loopback as separate channels) → local streaming transcription from downloadable on-device models → AI note enhancement (baseline structured summary always generated; optional first-class notepad merged in) via pluggable cloud LLM providers, BYOK first. Done = a spec detailed enough that implementation can start with no open questions.

## Notes

- **Domain**: local-first meeting transcription + AI notes. See `CONTEXT.md` (repo root) for the glossary; maintain it via /domain-modeling.
- **Skills every session should consult**: /grilling (HITL tickets), /domain-modeling, /research (research tickets), /prototype (prototype tickets).
- **Grilling preference**: ask ONE question at a time, wait for the answer, then ask the next. Never multi-question rounds.
- **Dev/test hardware**: the owner's daily driver is a Snapdragon X Elite Copilot+ PC, 32 GB unified RAM — Windows ARM64 is a real, testable target, not a build-and-pray.
- **Settled during charting** (no ticket needed):
  - Destination = build-ready spec (not a PoC, not just a stack call).
  - Product = full Granola loop: local transcription + LLM note enhancement.
  - Capture = mic + system loopback as separate channels; speakers labeled "You"/"Others" from channels only.
  - Recording starts manually (button), not auto-detected.
  - LLM integration = provider-pluggable architecture, BYOK (OpenRouter/direct keys) first; subscription OAuth deferred to ticket 08, not assumed.
  - Privacy line: audio never leaves the machine; transcript *text* goes to the chosen cloud LLM for enhancement. Accepted.
  - Shell stack: no preference at charting — researched in ticket 02, locked by the user in ticket 05.
  - License: MIT.

## Decisions so far

<!-- one line per closed ticket: gist + link; detail lives in the ticket -->

- [Local streaming ASR on Windows ARM64](issues/01-local-streaming-asr-on-windows-arm64.md) — Nemotron streaming 0.6B (cache-aware RNNT) via sherpa-onnx CPU INT8 (prebuilt win-arm64); Parakeet-TDT v2/v3 as offline fallback; Moonshine/whisper.cpp/Vosk/SenseVoice rejected; NPU offload deferred.
- [App shell: Electron vs Tauri vs .NET for native-ASR desktop app](issues/02-app-shell-electron-vs-tauri-vs-dotnet.md) — Tauri 2 chosen: in-process Rust ASR/WASAPI, smallest ARM64 footprint, free OSS signing+updater; Electron fallback via sidecar.
- [LLM provider landscape: BYOK, OpenRouter, and subscription OAuth status](issues/03-llm-provider-landscape-byok-openrouter-oauth.md) — BYOK via OpenRouter + direct keys is the safe MVP; Claude Pro/Max OAuth is forbidden and blocked by Anthropic, ChatGPT Plus/Pro OAuth is OpenAI-tolerated and viable as an optional plugin.
- [Windows audio capture pipeline for dual-channel streaming transcription](issues/04-windows-audio-capture-pipeline.md) — Rust: windows-rs WASAPI process-loopback (20348+, classic loopback fallback) + cpal mic, dual ring buffers time-aligned via QPC timestamps, rubato → 16 kHz mono, Silero VAD, per-lane local ASR; cpal covers macOS taps/PipeWire monitor; the ARM64 risk was cleared by ticket 11.
- [LLM provider architecture and the OAuth call](issues/08-llm-provider-architecture-and-oauth-call.md) — narrow ChatProvider seam with streaming SSE; one OpenAI-compatible provider (OpenRouter/OpenAI/Ollama/LM Studio) + experimental ChatGPT-OAuth plugin; keys in OS keychain via `keyring`; no Claude-subscription OAuth ever.
- [Snapdragon hardware spike: WASAPI loopback + Nemotron streaming](issues/11-spike-wasapi-loopback-nemotron-on-snapdragon.md) — PASS: native ARM64 and x64-emulated classic/process loopback all delivered non-silent packets; native sherpa-onnx v1.13.5 Nemotron INT8 ran at 0.14 weighted RTF normally (0.31 while memory-polled), 865 MiB peak working set, and 2.6% normalized WER on the supplied test set. Ticket 05 is unblocked.
- [Lock the stack and inference architecture](issues/05-lock-stack-and-inference-architecture.md) — Tauri 2 + TypeScript/Svelte frontend; Rust in-process ASR via sherpa-onnx C API; windows-rs WASAPI loopback + cpal mic, rubato resampling, Silero VAD via ort; Windows x64 + ARM64 only, 32-bit x86 dropped.
- [Prototype the core UI loop](issues/06-prototype-core-ui-loop.md) — Sidebar + main layout, fully collapsible via floating edge button; notes-first recording with live transcript tucked into a bottom-left pill that expands into a floating square; post-meeting defaults to My notes with pill tabs My notes → Enhance Notes → Transcript; enhancement is optional via an "Enhance Notes" prompt.
- [MVP spec scope cut](issues/09-mvp-spec-scope-cut.md) — IN: meeting library + FTS5 search, clipboard + Markdown export, built-in + custom summary templates, inline transcript editing, configurable global hotkey, minimal first-run wizard; OUT: semantic search, audio retention, DOCX/PDF, speaker relabeling, template sharing, feature tour.
- [Model distribution and management UX](issues/07-model-distribution-and-management-ux.md) — First-run download (no bundling) of the single default Nemotron EN model; no picker/multi-model management at MVP; HF direct with pinned SHA-256 + range resume, stored in `%LOCALAPPDATA%\Kiminola\models`; model updates pinned per app release.
- [Enhancement output shape and notepad merge](issues/13-enhancement-output-shape.md) — Raw notepad preserved verbatim in My notes; Enhance Notes produces a separate read-only AI artifact that rewrites/expands from notes + transcript; prompt-defined Markdown structure; re-enhance overwrites.
- [Local storage layout](issues/12-local-storage-layout.md) — SQLite + FTS5 single store via sqlx + migrations; user data in `%LOCALAPPDATA%\Kiminola\data`; Spaces as adjacency-list tree; Markdown export rendered with YAML frontmatter.
- [Telemetry and crash-reporting stance](issues/14-telemetry-and-crash-reporting.md) — No usage analytics ever; crash reporting opt-in only (Sentry/OSS plan) with no transcript/note content; update checks accepted as the only other outbound connection.
- [Packaging, signing, and distribution](issues/10-packaging-signing-distribution.md) — NSIS `.exe` primary + portable `.zip`; SignPath.io OSS signing primary; Tauri updater via GitHub Releases; GitHub Releases + winget distribution; x64 native + ARM64 cross-compile CI.

## Not yet specified

<!-- see "Fog of war": in-scope fog you can't ticket yet; graduates as the frontier advances -->

- **Update mechanism & release cadence** — hangs on the packaging ticket (10).

## Out of scope

- True per-speaker diarization at MVP — channel labels ("You"/"Others") only; revisit post-MVP.
- Meeting auto-detection / calendar integration at MVP — manual start/stop chosen at charting.
- Fully-offline local-LLM note enhancement at MVP — cloud BYOK chosen at charting; a local-provider option may return via the pluggable provider architecture later.
- Claude Pro/Max subscription OAuth — forbidden and server-side enforced by Anthropic (ticket 03); ruled out permanently in ticket 08.
