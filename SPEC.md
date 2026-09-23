# Kiminola — Build-Ready Spec

**Status:** Ready for implementation. All wayfinder decision tickets are resolved (see `.scratch/kiminola/map.md` and `issues/`). The library hierarchy decision is also adopted: Spaces and Meetings form a recursive organizational tree with validated reparenting.

## 1. Product definition

Kiminola (display name: **Kimi Nola**) is an open-source, Windows-first (x64 + ARM64) Granola alternative for **local meeting transcription** and **AI-enhanced notes**. It captures microphone and system audio as separate channels, transcribes on-device with a streaming ASR model, and lets the user optionally enhance their handwritten notes with a cloud LLM.

**Brand identity:** Oatwave — see `kiminola/branding/HANDOFF.md` (Rev A). Paper-cream light-first palette with charcoal dark mode; Gentium Book Plus / Archivo / IBM Plex Mono; one gold element per view.

**Privacy line:** audio never leaves the machine. Transcript text leaves only when the user explicitly chooses a cloud LLM for enhancement.

## 2. Core loop

1. **Idle / library** — sidebar shows Spaces tree, Boards, and recent meetings; "New meeting" starts capture.
2. **Recording** — the full screen is a notes-first sketch notepad. A subtle **Live transcript** pill sits bottom-left; clicking it opens a floating square that pushes the notepad right. Mic and loopback channels transcribe in parallel, labeled **You** / **Others**. Concurrent partial utterances remain independent; finalized cross-source copies caused by laptop-speaker bleed are conservatively reconciled in favor of the clean loopback result.
3. **Stop** — "Stop meeting" ends capture. Post-meeting defaults to the **My notes** tab with pills: **My notes → Enhance Notes → Transcript**.
4. **Enhance (optional)** — user clicks **Enhance Notes**, picks a template, and gets an AI artifact generated from their raw notes + transcript. Raw notes are never overwritten; re-enhance overwrites the AI artifact. Action items in enhanced notes can be edited and copied to a selected Board column. Editing an action item updates linked Board cards from that Meeting.

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

- Four primary surfaces: **Library/Idle**, **Recording**, **Post-meeting**, and **Boards**.
- Fully collapsible sidebar via floating edge button; state persists in `localStorage`.
- Top bar: "New meeting" primary action + light/dark theme toggle.
- Recording view: full-screen notepad; live transcript pill bottom-left; stop button reads "Stop meeting".
- Post-meeting view: pill tabs **My notes** / **Enhance Notes** / **Transcript**. Default tab: **My notes**.
- Boards view: user-created Boards with customizable ordered columns and movable action-item cards.
  - The board uses the available workspace width, with a horizontal board switcher above the columns. Board and column creation forms expand on demand. Narrow windows scroll columns inside the board rather than overflowing the page.
- Updates: after the main app launches, Kimi Nola performs one non-blocking
  check against the published stable GitHub Release feed. A visible update
  notice offers release details, but download and installation always require
  explicit user approval. Installation is disabled and re-checked while the
  active recording route is open; a passive Windows installer restarts the app
  after a successful update. Settings also exposes a manual check.

### 4.2 Rust backend

- **Audio pipeline**: WASAPI process-tree loopback when a meeting prompt supplies a PID, with classic default-output fallback, plus cpal mic → independent resampling → 16 kHz mono → sherpa-onnx streaming ASR per lane. The lanes are never mixed before ASR.
- **Transcript events**: each lane emits stable utterance IDs, monotonically increasing revisions, partial/final state, and audio-relative segment timing. On stop, capture queues drain, both ASR lanes flush, and the backend returns an authoritative final snapshot before persistence.
- **Echo reconciliation**: finalized mic/system segments are compared only when their audio windows overlap. High-confidence text matches retain the system (`Others`) copy; short acknowledgements and non-overlapping repetition are never automatically suppressed. This is a local transcript correction, not speaker diarization or audio retention.
- **Persistence**: sqlx + SQLite; tables for meetings, timestamped transcript segments, notes, spaces, templates, settings, boards, board columns, and board cards. Each Meeting has exactly one direct container (`space_id` or `parent_meeting_id`), and recursive location paths are computed for library views and exports.
- **LLM enhancement**: ChatProvider trait with streaming SSE; sends transcript text + raw notes + template prompt to the configured cloud provider.
- **Model manager**: downloads ASR model on first run from Hugging Face; verifies SHA-256; stores in `%LOCALAPPDATA%\Kiminola\models`.
- **Updater**: Tauri updater plugin with one embedded public key and the stable
  `https://github.com/Trillx/Kiminola/releases/latest/download/latest.json`
  endpoint. The release workflow signs the x64 and ARM64 NSIS artifacts and
  publishes one manifest only after both architecture assets are present.
  Drafts and prereleases are never offered by the client; releases move
  forward only, and a bad release is corrected with a higher patch version.

## 5. UI/UX decisions

- Sidebar navigation: **Home** and **Boards**; Spaces is a recursive tree. Spaces may contain nested Spaces and Meetings, and Meetings may contain child Meetings at arbitrary depth. Context menus and drag-and-drop use the same validated move operation; sibling ordering is not user-controlled in this pass.
- At compact widths (760 CSS pixels or less), navigation opens as a modal drawer with keyboard focus containment, Escape dismissal, and focus restoration. Desktop collapse preference remains independent. Long libraries scroll within the navigation area while Settings stays visible.
- Space and parent-Meeting branches expand independently, with consistent indentation and hairline connectors. Open/closed state persists locally; navigating to a Meeting reveals its ancestors. Disclosure arrows point right when closed and down when open. Branch height and arrows transition over 180 ms, with no motion when reduced motion is enabled. Enter/Space activate the focused control; Left/Right close/open branches and move between parent and child controls.
- New meetings inherit an explicit Space or Meeting destination from the action that started them. Global New meeting and the shortcut use the last explicit destination, falling back to Personal. The destination is captured when recording starts.
- Navigating from one recording URL to another requires the current episode's normal exit guard. After successful recovery persistence, the destination starts a fresh recording episode, with separate draft, notes, transcript, timer, and controls; failed persistence leaves the original episode intact.
- Removed from prototype: Invite, Shared with me, Chat — out of MVP scope.
- Live transcript opens at the latest speech and follows new lines and partial revisions only while the reader is near the bottom; scrollbar hidden until user scrolls. Recovered rows and live utterances have disjoint stable renderer identities.
- Transcript corrections save explicitly; Cancel never saves. Failed saves retain the correction and display a retryable error.
- Global hotkey: configurable start/stop (Tauri global-shortcut plugin), sensible default. Invalid or unavailable replacements preserve the active binding; persistence and compensation failures are visible rather than silently disabling it.
- Onboarding: minimal first-run wizard — mic permission → model download with progress → optional BYOK key (skippable). The microphone step checks permission only and does not display simulated audio levels. Provider saves/tests lock conflicting edits and actions until IPC settles; results apply only to the submitted configuration in the current wizard instance, and any provider/model/key edit clears the previous test result.
- AI credentials are scoped to provider kind and normalized full Base URL, excluding model. Provider/endpoint edits clear replacement credentials and stale saved-key status; returning to the unchanged saved identity restores only its previously confirmed presence. Other destinations can be saved without a connection test to read back their own credential status. Legacy unscoped credentials are not reused or migrated because their destination cannot be proven; users re-enter their key. Requests do not follow redirects outside the configured endpoint.
- AI provider settings: when OpenRouter is selected, an explicit catalog refresh fetches the current model list from OpenRouter and exposes a searchable model selector. A failed refresh leaves manual model-ID entry available.
- Companion layout: when the user chooses **Start recording** from a Meeting prompt, Kimi Nola arranges the detected meeting window on the left at roughly two-thirds of the active display and its own Notepad window on the right at roughly one-third. Visible windows move and resize together in a 200 ms ease-out transition; reduced motion uses immediate placement. Hidden or minimized notes open at the destination. Starting a user move/resize, closing either window, or requesting another layout cancels the transition. The arrangement is a starting point: normal windows may be resized, true full-screen/presentation windows are not forced to change, and user movement/resizing is never overridden. During manual resizing, content follows the viewport immediately; only an explicit sidebar toggle animates the shell geometry.
- Meeting-prompt recording also uses the detected meeting process tree as the preferred loopback target. Manual recording and unsupported/failed process activation use classic default-output loopback.
- Active recordings checkpoint notes, elapsed duration, and the latest transcript revisions into a SQLite note draft. An interrupted session remains visible in the library and can be continued into a normal saved meeting; intentional cancellation removes only drafts created automatically for that recording.
- Continuous edits cannot postpone the checkpoint attempt beyond five seconds; recovery snapshots are constructed when the serialized write executes. Recovery-only exits wait for successful persistence and retain the editor on failure, with retry or explicit discard. A separate successful meeting save may provide durability when checkpointing fails.

## 6. Data model & storage

- **SQLite single store** via sqlx + migrations.
- **Update durability**: installation waits for pending note saves and app commands, blocks new recording starts, and closes the database pool before handing off to the installer. Save failures keep the app open.
- **Migration recovery**: before changing an existing schema, create and verify a SQLite snapshot under the data folder's `backups/` directory. Backup or migration failure blocks normal app use and presents retry and restore options. Restore validates and migrates a staged copy first, preserves the original database and sidecars, and records an interruption marker so a failed restore cannot create an empty library. Backups are retained; an older app must not open a newer schema.
- **Location**: `%LOCALAPPDATA%\Kiminola\data` for user data; `%LOCALAPPDATA%\Kiminola\models` for ASR models.
- **Schema** (initial):
  - `meetings` — id, title, nullable `space_id`, nullable `parent_meeting_id`, created_at, duration_seconds. Exactly one location column is set for saved Meetings.
  - `transcript_segments` — id, meeting_id, channel ('you'|'others'), start_ms, end_ms, text
  - `notes` — id, meeting_id, raw_markdown, updated_at
  - `note_drafts` — id, title, created_at, updated_at, raw_markdown, optional meeting_id, recovery_duration_seconds, recovery_transcript_json
  - `spaces` — id, name, parent_id, created_at (adjacency list). Spaces can contain Spaces; Meetings can be direct children of Spaces or Meetings.
  - `templates` — id, name, prompt, is_builtin
  - `settings` — key, value
  - `search_index` — FTS5 virtual table over meeting titles, notes, transcript segments
  - `boards` — id, name, created_at
  - `board_columns` — id, board_id, name, position
  - `board_cards` — id, column_id, nullable meeting_id, title, position, created_at
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
- Recursive Spaces/Meetings organization, context menus, validated drag-and-drop reparenting, and computed location paths in Home, detail, and Markdown export
- Export: clipboard copy + `.md` notes / `.txt` transcript
- Summary templates: general default + built-in library (1:1, hiring, weekly team, customer discovery, VC pitch) + user-created custom templates
- Inline transcript editing
- Configurable global hotkey
- User-created Boards with customizable columns and action-item cards linked back to Meetings
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
- Detection stays local and advisory, inspecting session metadata without
  opening capture streams. A recognized native meeting app requires its process
  or visible window plus an active Core Audio session. Other apps use a balanced
  fallback: a visible app window and both active input and output sessions
  associated with the same app family for two consecutive successful detector
  polls. Playback alone or input alone is not enough for an unknown app to
  prompt. Activity means running streams, not audible speech; this is never
  proof of a meeting. Partial evidence remains a quiet possible hint with
  coarse evidence labels; detection never starts recording. Prompts defer
  while Windows reports presentation mode or a full-screen foreground app.
- Detection associates helper-process audio with its meeting-app process family
  and checks all active input/output audio endpoints, including non-default
  headsets. Episode suppression and process-loopback capture use the family
  root; Companion layout targets the family's visible application window.
  Unrelated processes are never grouped solely by matching executable names.
  Unknown apps may associate same-executable descendants and embedded WebView
  helpers with their visible owner, but never use an arbitrary shell ancestor
  as the owner. Browser audio is only app-family evidence, not proof about a
  particular tab. Browsers use the balanced input/output rule even when a
  meeting title supplies a friendly label. Unattributable activity must not
  be presented as certain.
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
- Recording startup, active capture, and finalization suppress meeting hints and
  prompts. Starting recording clears pending prompts and retracts their visible
  notifications. Episodes observed during recording remain suppressed until the
  normal inactivity reset; a later meeting may prompt again.
- At most one prompt is shown per active meeting-presence episode for an app.
  An eligible recognized native meeting app may replace an unanswered generic-app
  prompt; the superseded episode stays consumed and its old actions are stale.
  Any prompt action suppresses only that episode; after two consecutive
  detector polls without active meeting audio, the next meeting may prompt
  again even if the meeting app process remains open. Prompt actions are
  accepted only while their prompt ID is current; stale or unknown actions
  are rejected without recording.
- One inactive poll does not discard an unanswered prompt. Two inactive polls
  clear it and rearm the episode; observed process exit or PID reuse clears it
  immediately. Creation times distinguish successive processes sharing a PID.
  Explicit recording actions revalidate the process instance before queuing
  and consuming a capture target; a changed target fails rather than recording
  a replacement app. Hidden windows and incomplete audio scans do not establish
  inactivity. Failed scans break onset/inactivity streaks without clearing
  explicit dismissals. Activity continues to update while presentation/full-screen
  mode defers visible prompts.
- Pausing suppresses prompts while local metadata observation continues, so
  meeting endings still rearm episodes. Unanswered prompts may return on resume
  with fresh action IDs; explicitly dismissed episodes remain suppressed until
  their normal inactivity reset. Disabling detection stops observation and
  clears runtime state.
- Settings expose Meeting detection and Start with Windows. Tray status is
  “Detecting locally · not recording”, “Paused”, or “Off”.

## 9. Packaging & distribution

- **Installer**: NSIS `.exe` for x64 + ARM64; portable `.zip` secondary artifact.
- **Signing**: SignPath.io OSS Authenticode signing is separate from the Tauri
  updater signing key. The updater public key is committed; its private key and
  password exist only in GitHub Actions secrets and an offline maintainer
  backup. Tag jobs submit the release executable and final installer to SignPath,
  require a valid Authenticode chain, then generate the updater signature over the
  final Authenticode-signed installer bytes. Final verification downloads the
  exact assets and re-checks both signature systems.
- **Auto-update**: Tauri built-in updater + GitHub Releases JSON manifest. Tag
  pushes first validate the tag and synchronized application versions, then
  use a GitHub-hosted gate to dispatch the physical x64 + ARM64 checks in a
  separate workflow and verify success for the exact tag commit before creating a
  draft. This keeps self-hosted jobs outside SignPath's trusted-build dependency
  graph. Native release jobs bundle the already architecture-checked executable,
  pass complete portable archives and SHA-256 provenance through short-lived
  workflow artifacts, and keep repository-write access in minimal publishing
  jobs. Signed installers use the canonical
  `Kimi.Nola_<version>_<arch>-setup.exe` asset name. Read-only final verification
  checks downloaded hashes, installer and extracted portable-executable
  Authenticode, updater signatures, and the actual PE architecture of every
  portable executable and native DLL. It generates `latest.json`; a publishing job uploads and
  downloads it again for byte-for-byte and exact-asset verification before manual
  publication.
- **Channels**: GitHub Releases primary; winget secondary; Microsoft Store post-MVP.
- **CI**: GitHub Actions uses explicit Windows runners: x64 on `windows-2025`
  and native ARM64 on `windows-11-arm`. Every pull request and `main` push runs
  frontend checks and browser regressions once, Rust formatting/clippy/full
  tests on x64, security audits, and native x64 + ARM64 unsigned package/startup
  validation. Version tags also run both unsigned package jobs. Unsigned
  installers and complete portable archives (the executable
  plus all four native runtime DLLs) are retained for seven days; updater signing
  is available only to the tag release workflow. Production npm dependencies
  are audited at moderate severity or higher in addition to Cargo and dependency
  review gates.

## 10. Privacy & telemetry

- **No usage analytics, ever.**
- **Crash reporting**: opt-in only (Sentry OSS plan or equivalent); minimal report (stack trace, version, OS/arch); no transcript or note content.
- **Update checks**: the only automatic outbound connection when crash reporting is disabled.

## 11. Hardware targets & testing

- **Targets**: Windows x64 (`x86_64-pc-windows-msvc`) and ARM64 (`aarch64-pc-windows-msvc`).
- **Validation**: GitHub-hosted x64 and ARM64 runners cover native compilation,
  installer creation, PE-architecture checks, and executable startup on every
  pull request and `main` push. Dedicated self-hosted x64 and Snapdragon X
  Elite ARM64 runners, labeled `kiminola-hardware`, run nightly/manual physical
  microphone capture of a known 440 Hz stimulus on an explicitly provisioned
  non-virtual input device, requiring a material increase over a matched
  stimulus-off baseline while the source remains live, WASAPI loopback, deterministic
  installed-model speech transcription, installer, and
  previous-version preservation checks, and the same workflow is a required
  pre-draft tag-release gate. A missing runner configuration fails visibly rather
  than producing a green no-op. The update baseline is the highest updater-signed
  stable release strictly older than the candidate; explicit baselines must meet
  the same rule. Routine and release-correlated hardware runs use distinct
  concurrency keys so a scheduled run cannot replace a pending release gate;
  dedicated single-job runner services serialize destructive work on each
  physical machine. Hardware validation verifies native OS/process/LLVM
  architecture, backs up the pre-existing application directory, complete
  persistent user root, installer registry keys, and desktop and Start Menu
  shortcuts under a same-volume persistent recovery root, and records each
  destructive transition in an atomic journal, including registry export before
  live-key removal. A later run recovers or halts on that journal immediately after
  checkout and before tool setup or fixture access, and recovery data is deleted
  only after complete restoration. The workflow also authenticates the previous
  installer, verifies the installed executable and native-DLL architecture, and
  waits for a semantically complete legacy migration history before seeding upgrade
  data. The pre-publication gate upgrades by executing the
  verified candidate installer directly because the app's stable feed excludes
  drafts; in-app updater discovery and installation are smoke-tested on both
  architectures immediately after publication.
- **Spike results**: native ARM64 WASAPI loopback delivered non-silent packets; sherpa-onnx v1.13.5 Nemotron INT8 ran at 0.14 weighted RTF, 865 MiB peak working set, 2.6% normalized WER.
- **Transcript regression matrix**: remote-only playback is `Others`; local-only mic speech is `You`; double-talk retains both lanes; speaker bleed does not create a duplicate `You` line; short acknowledgements are not over-suppressed; and stop waits for finalized text with monotonic audio-relative timing.
- **Update regression**: on both x64 and ARM64, install a prior stable build,
  insert a complete semantic fixture into its untouched database, preserve the
  downloaded model, install and launch the candidate so it applies any pending
  current migrations, and verify complete migration history plus the exact meeting,
  notes, transcript, template, setting, recovery draft, library destination, and
  model hashes remain intact. When no migration is pending, the same gate verifies
  the unchanged history and data. After publication, also accept the signed update through the app
  on both architectures. A green CI build is not runtime proof of updater safety.
