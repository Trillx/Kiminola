# Onboarding experience with Model pack downloader

Type: wayfinder:map
Status: open

## Destination

A mandatory first-run wizard that prepares Kiminola for its first Capture session: it confirms microphone access, downloads and verifies the 160 ms Nemotron Model pack from Hugging Face (or detects a valid manually-staged pack), and optionally configures a cloud LLM Provider. The library is inaccessible until onboarding completes. Download supports resume, per-file SHA-256 verification, and clear error recovery.

## Notes

- Domain terms live in `CONTEXT.md` (first-run wizard, Model pack, Model manifest, Onboarding state, Provider).
- Grounding: `SPEC.md` §7 (model management) and §8 (MVP scope: minimal first-run wizard).
- Existing code already expects models at `%LOCALAPPDATA%\Kiminola\models\nemotron` and falls back to executable-relative paths; see `src-tauri/src/asr.rs`.
- Skills used: `/grilling` (destination and UX decisions), `/domain-modeling` (glossary updates), `/research` (exact HF repo and file list).
- Exact Model pack pinned by the research subagent: `csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25` (chosen for lower latency over the 560 ms primary).

## Decisions so far

- **Mandatory first-run gate** — the library is inaccessible until onboarding completes. Closed in the charting `/grilling` session.
- **Three-step wizard, only AI Provider config skippable** — steps: (1) microphone permission, (2) Model pack download, (3) optional AI Provider config. Closed in charting.
- **Hugging Face direct source** — download individual files from a pinned HF repo/revision with per-file SHA-256 verification against an embedded Model manifest. Closed in charting.
- **Calm progress UX** — progress bar with percentage, "X MB of Y MB", ETA, and a one-line privacy explanation. Closed in charting.
- **Resume + verification** — HTTP Range-request resume for partial files; per-file SHA-256 verification; failing files re-downloaded from scratch. Closed in charting.
- **Manual / offline install supported** — valid local Model pack in the expected folder skips the download; "Open model folder" button offered. Closed in charting.
- **Network failure handling** — exponential-backoff auto-retry, then an error card with Retry / Open model folder / Copy diagnostics. Closed in charting.
- **Mic step includes optional mic check** — permission grant is required; a 3-second input-level check is optional. Closed in charting.
- **160 ms Nemotron variant pinned** — chosen over 560 ms for lower latency. Closed in charting.
- **Full-screen wizard** — takes over the whole window since the library is gated. Closed in charting.
- **Forward-only navigation** — no going back; optional Provider step can be skipped forward. Closed in charting.
- **Completion auto-routes to library** — with a brief "Model ready" confirmation. Closed in charting.
- **Step indicator** — "Step X of 3" header with segmented progress bar. Closed in charting.
- **Wizard UI structure and component boundaries** — isolated `(onboarding)` route with `+layout@.svelte`; full-screen centered wizard card; step indicator as segmented progress bar; Provider step is a compact form with Skip. Closed by [ticket 19](.scratch/kiminola/issues/19-first-run-wizard-ui-prototype.closed.md).
- **Onboarding state and route gating** — SQLite `onboarding_complete` boolean; SvelteKit layout redirects to `/onboarding` until set; missing Model pack post-onboarding shows a library banner. Closed by [ticket 18](.scratch/kiminola/issues/18-onboarding-state-and-route-gating.closed.md).
- **Model manifest format & location** — embed `src-tauri/models/manifest.json`, load via `include_str!`, typed `ModelManifest`/`ModelFile` structs. Closed by ticket 16.
- **HTTP client for download** — keep existing `reqwest` (`native-tls` + `stream`); do not route ~650 MB through IPC. Closed by ticket 16.
- **Download resume strategy** — one `.part` file per model file, request `Range: bytes={existing_len}-` from HF `resolve/` URL, fall back to full download on non-`206`. Closed by ticket 16.
- **Progress streaming to UI** — Tauri 2 `tauri::ipc::Channel`, throttled ~10 Hz. Closed by ticket 16.
- **SHA-256 verification timing** — per-file immediately after download, before atomic `.part` → final rename, streaming from disk with `sha2`. Closed by ticket 16.
- **Mic permission detection** — do not rely on cpal enumeration alone; run a real 3-second capture-stream probe to trigger/verify permission. Distinguish states via `AppCapability::CheckAccess("Microphone")` and probe result. Closed by ticket 17.

## Not yet specified

- None — the route is clear.

## Out of scope

- Multi-model picker or model manager UI (MVP ships one pinned Model pack).
- Bundling the Model pack in the installer (SPEC §7 says first-run download).
- NPU / GPU inference offload for the Model pack.
- Subscription-OAuth Provider setup during onboarding (only BYOK API key config).
- Feature tour or tooltips beyond the wizard steps.
