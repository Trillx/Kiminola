# Kimi Nola improvement backlog

Ordered by user impact: data loss and crashes first, then broken flows, test gaps, UX, accessibility, polish, and refactors. Keep each item small enough for one verified commit.

## Queued

- [ ] **P0 · Make the global stop shortcut save-safe.** Pressing the shortcut while `/record` is open stops the native session and navigates home without creating a meeting, leaving only the recovery draft and no explanation.
- [ ] **P2 · Base elapsed duration on a monotonic clock.** The one-second UI interval can drift while the app is suspended or the window is hidden.
- [ ] **P2 · Add model repair to Settings.** Model installation and repair only exist inside first-run onboarding, so an unavailable-transcription warning cannot route to a focused in-app recovery flow.

## Completed

- [x] **P0 · Block tray quit while a meeting is recording.** Active sessions now keep the process alive, restore the recording window, and explain how to save or intentionally cancel. Verified 2026-08-22.
- [x] **P0 · Continuously save recording notes for recovery.** Every recording now uses a serialized, debounced SQLite note draft; normal saves attach it, intentional cancellation removes only auto-created drafts, and crashes leave a library-visible recovery copy. Verified 2026-08-22.
- [x] **P0 · Persist in-progress transcript checkpoints.** Recovery drafts now atomically store notes, elapsed duration, and the latest transcript text; interrupted sessions display their transcript and can continue with correctly offset timing. Verified 2026-08-23.
- [x] **P1 · Surface recording startup failures.** The recording lifecycle now remains in Starting until native capture succeeds; failures stop the timer and waveform, disable invalid controls, preserve recovery notes, and offer retry plus Windows microphone settings. Verified 2026-08-23.
- [x] **P1 · Make stop-and-save failures retryable.** Stop is idempotent, finalization warnings return the best transcript snapshot, recovery drafts make meeting saves idempotent, and save failures expose retry/open-recovery actions without duplicating meetings. Verified 2026-08-23.
- [x] **P1 · Surface pause and resume failures.** In-flight capture controls cannot race Stop; pause failure leaves recording active with an explanation, while resume failure keeps the paused dialog open with retry and microphone-settings actions. Verified 2026-08-23.
- [x] **P1 · Detect audio capture queue pressure.** Microphone and meeting-audio callbacks now count dropped samples without blocking, emit throttled source-specific health events, and warn when transcript gaps may have occurred. Verified 2026-08-23.
- [x] **P1 · Surface loopback capture availability.** Start and resume now report whether Windows meeting audio opened successfully; microphone-only sessions warn that other participants are not being captured and explain how to retry. Verified 2026-08-23.
- [x] **P1 · Surface unavailable local transcription.** Start and resume now report whether the on-device ASR engine is present; capture continues safely while the recording view explains that spoken content will not be transcribed and links to the local model folder. Verified 2026-08-23.
- [x] **P1 · Retry ASR loading after in-app model installation.** Failed launch-time model loads are no longer cached permanently; later starts retry serially, while the first successful engine remains shared for the process lifetime. Verified 2026-08-23.
- [x] **P2 · Surface transcript-finalization warnings.** Stop now returns the best transcript plus a non-fatal warning; save retries preserve it, and the saved meeting explains that its ending may be incomplete with a direct transcript-review action. Verified 2026-08-23.
