# Kiminola — Build-Ready Spec

**Status:** Ready for implementation. All wayfinder decision tickets are resolved (see `.scratch/kiminola/map.md` and `issues/`).

## 1. Product definition

Kiminola (display name: **Kimi Nola**) is an open-source, Windows-first (x64 + ARM64) Granola alternative for **local meeting transcription** and **AI-enhanced notes**. It captures microphone and system audio as separate channels, transcribes on-device with a streaming ASR model, and lets the user optionally enhance their handwritten notes with a cloud LLM.

**Brand identity:** Oatwave — see `kiminola/branding/HANDOFF.md` (Rev A). Paper-cream light-first palette with charcoal dark mode; Gentium Book Plus / Archivo / IBM Plex Mono; one gold element per view.

**Privacy line:** audio never leaves the machine. Transcript text leaves only when the user explicitly chooses a cloud LLM for enhancement.

## 2. Core loop

1. **Idle / library** — sidebar shows Spaces tree and recent meetings; "New meeting" starts capture.
2. **Recording** — the full screen is a notes-first sketch notepad. A subtle **Live transcript** pill sits bottom-left; clicking it opens a floating square that pushes the notepad right. Mic and loopback channels transcribe in parallel, labeled **You** / **Others**.
3. **Stop** — "Stop meeting" ends capture. Post-meeting defaults to the **My notes** tab with pills: **My notes → Enhance Notes → Transcript**.
4. **Enhance (optional)** — user clicks **Enhance Notes**, picks a template, and gets a read-only AI artifact generated from their raw notes + transcript. Raw notes are never overwritten; re-enhance overwrites the AI artifact.

## 3. Stack

| Layer | Choice | Notes |
|-------|--------|-------|
| Shell | **Tauri 2** | Rust backend + TypeScript/Svelte frontend via Vite |
| ASR | **sherpa-onnx C API** | In-process Rust; Nemotron streaming 0.6B (cache-aware FastConformer-RNNT), INT8, CPU provider |
| Audio capture | **windows-rs WASAPI** | Process loopback (20348+) with classic loopback fallback; **cpal** for mic |
| Resampling | **rubato** | Dual-channel → 16 kHz mono |
| VAD | **Silero via ort** | ONNX Runtime |
| Database | **sqlx + SQLite (bundled)** | Migrations from day one; FTS5 virtual table for search |
| LLM | **ChatProvider seam** | Streaming SSE; OpenAI-compatible providers (OpenRouter, OpenAI, Ollama, LM Studio); keys in OS keychain via `keyring` |
| Distribution | **NSIS + Tauri updater** | GitHub Releases + winget; SignPath.io OSS code signing |

## 4. Architecture

### 4.1 Frontend (Svelte + TypeScript)

- Three screens: **Library/Idle**, **Recording**, **Post-meeting**.
- Fully collapsible sidebar via floating edge button; state persists in `localStorage`.
- Top bar: "New meeting" primary action + light/dark theme toggle.
- Recording view: full-screen notepad; live transcript pill bottom-left; stop button reads "Stop meeting".
- Post-meeting view: pill tabs **My notes** / **Enhance Notes** / **Transcript**. Default tab: **My notes**.
- Theme: light/dark; dark mode uses warm amber accent on deep charcoal canvas.

### 4.2 Rust backend

- **Audio pipeline**: WASAPI process loopback + cpal mic → dual ring buffers (QPC time-aligned) → rubato → 16 kHz mono → Silero VAD → sherpa-onnx streaming ASR per lane.
- **Persistence**: sqlx + SQLite; tables for meetings, transcript segments, notes, spaces, templates, settings.
- **LLM enhancement**: ChatProvider trait with streaming SSE; sends transcript text + raw notes + template prompt to the configured cloud provider.
- **Model manager**: downloads ASR model on first run from Hugging Face; verifies SHA-256; stores in `%LOCALAPPDATA%\Kiminola\models`.
- **Updater**: Tauri updater plugin with GitHub Releases JSON manifest.

## 5. UI/UX decisions

- Sidebar navigation: **Home** only; Spaces is an expandable tree with meetings nested underneath.
- Removed from prototype: Invite, Shared with me, Chat — out of MVP scope.
- Live transcript auto-scrolls; scrollbar hidden until user scrolls.
- Global hotkey: configurable start/stop (Tauri global-shortcut plugin), sensible default.
- Onboarding: minimal first-run wizard — mic permission → model download with progress → optional BYOK key (skippable).

## 6. Data model & storage

- **SQLite single store** via sqlx + migrations.
- **Location**: `%LOCALAPPDATA%\Kiminola\data` for user data; `%LOCALAPPDATA%\Kiminola\models` for ASR models.
- **Schema** (initial):
  - `meetings` — id, title, space_id, created_at, duration_seconds
  - `transcript_segments` — id, meeting_id, channel ('you'|'others'), start_ms, end_ms, text
  - `notes` — id, meeting_id, raw_markdown, updated_at
  - `note_drafts` — id, title, created_at, updated_at, raw_markdown, optional meeting_id
  - `spaces` — id, name, parent_id, created_at (adjacency list)
  - `templates` — id, name, prompt, is_builtin
  - `settings` — key, value
  - `search_index` — FTS5 virtual table over meeting titles, notes, transcript segments
- **Export**: Markdown rendered from DB with YAML frontmatter (title, date, space, duration).

## 7. Model management

- **First-run download** from Hugging Face: Nemotron streaming 0.6B EN (~632 MB), pinned revision.
- **Verification**: SHA-256 after download; redownload on mismatch.
- **Resume**: HTTP range-request resume.
- **Updates**: model manifest (URL, size, SHA-256) pinned in app binary; new model versions arrive via app updates. Old model kept until new one verifies.
- **No picker at MVP**: single default model; settings shows installed model info.

## 8. Scope (MVP)

**IN:**
- Meeting library + FTS5 search over titles, notes, transcripts
- Export: clipboard copy + `.md` notes / `.txt` transcript
- Summary templates: general default + built-in library (1:1, hiring, weekly team, customer discovery, VC pitch) + user-created custom templates
- Inline transcript editing
- Configurable global hotkey
- Minimal first-run wizard

**OUT:**
- Semantic/AI search
- Audio retention (never keep audio at MVP)
- DOCX/PDF export
- Speaker relabeling (You/Others reassignment)
- Template sharing/marketplace, variables/macros, per-Space defaults
- Onboarding feature tour
- Microsoft Store / MSI / MSIX packaging
- True per-speaker diarization
- Calendar integration
- Fully-offline local-LLM enhancement
- Claude Pro/Max subscription OAuth

## 8.1 Post-MVP: meeting presence prompts

- Meeting presence is an opt-in background companion. Closing the main window
  hides it and leaves the companion running; tray Quit exits fully.
- Detection stays local and advisory. A prompt requires two independent
  signals: a known process or visible app window plus an active Core Audio
  session. A single signal remains a quiet possible hint with coarse evidence
  labels; detection never starts recording. Prompts defer while Windows reports
  presentation mode or a full-screen foreground app.
- Initial friendly labels are Granola, Zoom, Microsoft Teams, Google Meet,
  Webex, and a generic “another app” fallback. Raw executable names, window
  titles, URLs, calendar metadata, and detector history are not persisted or
  shown to the user.
- A prompt says “You may be in a meeting. Want to jot notes?” and “Kimi Nola
  is not recording.” Actions are Jot notes, Start recording, and Not now.
  Start recording is always an explicit user action.
- Jot notes creates a standalone Note draft in the same library. Drafts
  autosave, survive restart, never expire silently, and may be explicitly
  deleted. Starting and saving a meeting can attach the draft's notes.
- At most one prompt is shown per active meeting-presence episode for an app.
  Any prompt action suppresses only that episode; after two consecutive
  detector polls without active meeting audio, the next meeting may prompt
  again even if the meeting app process remains open. Prompt actions are
  accepted only while their prompt ID is current; stale or unknown actions
  are rejected without recording.
- Settings expose Meeting detection and Start with Windows. Tray status is
  “Detecting locally · not recording”, “Paused”, or “Off”.

## 9. Packaging & distribution

- **Installer**: NSIS `.exe` for x64 + ARM64; portable `.zip` secondary artifact.
- **Signing**: SignPath.io OSS program primary; self-signed/unsigned fallback.
- **Auto-update**: Tauri built-in updater + GitHub Releases JSON manifest.
- **Channels**: GitHub Releases primary; winget secondary; Microsoft Store post-MVP.
- **CI**: GitHub Actions — x64 native on `windows-latest`; ARM64 cross-compiled to `aarch64-pc-windows-msvc` from x64 runner. Fallback: manual/self-hosted ARM64 builds on Snapdragon X Elite.

## 10. Privacy & telemetry

- **No usage analytics, ever.**
- **Crash reporting**: opt-in only (Sentry OSS plan or equivalent); minimal report (stack trace, version, OS/arch); no transcript or note content.
- **Update checks**: the only automatic outbound connection when crash reporting is disabled.

## 11. Hardware targets & testing

- **Targets**: Windows x64 (`x86_64-pc-windows-msvc`) and ARM64 (`aarch64-pc-windows-msvc`).
- **Validation**: Snapdragon X Elite Copilot+ PC (32 GB) for ARM64; GitHub Actions for x64 CI.
- **Spike results**: native ARM64 WASAPI loopback delivered non-silent packets; sherpa-onnx v1.13.5 Nemotron INT8 ran at 0.14 weighted RTF, 865 MiB peak working set, 2.6% normalized WER.
