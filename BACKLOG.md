# Kimi Nola improvement backlog

Ordered by user impact: data loss and crashes first, then broken flows, test gaps, UX, accessibility, polish, and refactors. Keep each item small enough for one verified commit.

## Queued

- [x] **P1 · Prevent navigation actions from racing pause or resume.** The navigation dialog can open while a capture-control command is in flight; its save/recovery actions now wait for that transition before issuing Stop. Verified 2026-08-23. Commit: da260ae.
- [x] **P2 · Checkpoint recovery on pause.** A successful pause now immediately flushes the newest duration and transcript into the recovery draft. Verified 2026-08-23. Commit: da260ae.
- [x] **P2 · Verify model contents during health checks.** Settings now hashes every model file with a real manifest digest and repairs same-size corruption instead of reporting it ready. Verified 2026-08-23. Commit: 467752e.

## Completed

- [x] **P0 · Block tray quit while a meeting is recording.** Active sessions now keep the process alive, restore the recording window, and explain how to save or intentionally cancel. Verified 2026-08-22. Commit: 6701664.
- [x] **P0 · Continuously save recording notes for recovery.** Every recording now uses a serialized, debounced SQLite note draft; normal saves attach it, intentional cancellation removes only auto-created drafts, and crashes leave a library-visible recovery copy. Verified 2026-08-22. Commit: 372c4af.
- [x] **P0 · Persist in-progress transcript checkpoints.** Recovery drafts now atomically store notes, elapsed duration, and the latest transcript text; interrupted sessions display their transcript and can continue with correctly offset timing. Verified 2026-08-23. Commit: aa0c056.
- [x] **P1 · Surface recording startup failures.** The recording lifecycle now remains in Starting until native capture succeeds; failures stop the timer and waveform, disable invalid controls, preserve recovery notes, and offer retry plus Windows microphone settings. Verified 2026-08-23. Commit: 68e3af7.
- [x] **P1 · Make stop-and-save failures retryable.** Stop is idempotent, finalization warnings return the best transcript snapshot, recovery drafts make meeting saves idempotent, and save failures expose retry/open-recovery actions without duplicating meetings. Verified 2026-08-23. Commit: 460968b.
- [x] **P1 · Surface pause and resume failures.** In-flight capture controls cannot race Stop; pause failure leaves recording active with an explanation, while resume failure keeps the paused dialog open with retry and microphone-settings actions. Verified 2026-08-23. Commit: 4030e94.
- [x] **P1 · Detect audio capture queue pressure.** Microphone and meeting-audio callbacks now count dropped samples without blocking, emit throttled source-specific health events, and warn when transcript gaps may have occurred. Verified 2026-08-23. Commit: 4ca11d9.
- [x] **P1 · Surface loopback capture availability.** Start and resume now report whether Windows meeting audio opened successfully; microphone-only sessions warn that other participants are not being captured and explain how to retry. Verified 2026-08-23. Commit: b22ce8b.
- [x] **P1 · Surface unavailable local transcription.** Start and resume now report whether the on-device ASR engine is present; capture continues safely while the recording view explains that spoken content will not be transcribed and links to the local model folder. Verified 2026-08-23. Commit: 29644f8.
- [x] **P1 · Retry ASR loading after in-app model installation.** Failed launch-time model loads are no longer cached permanently; later starts retry serially, while the first successful engine remains shared for the process lifetime. Verified 2026-08-23. Commit: 28b3a20.
- [x] **P2 · Surface transcript-finalization warnings.** Stop now returns the best transcript plus a non-fatal warning; save retries preserve it, and the saved meeting explains that its ending may be incomplete with a direct transcript-review action. Verified 2026-08-23. Commit: b62b04e.
- [x] **P0 · Make the global stop shortcut save-safe.** The global listener now only starts recordings; while recording, the page routes shortcut presses through the same guarded, retry-safe finalization and durable save path as the Stop button. Verified 2026-08-23. Commit: 9a0560b.
- [x] **P1 · Guard navigation away from active recording.** Internal navigation during capture or after a failed save now pauses for an explicit choice to continue, finish through the durable save path, or checkpoint the final transcript into a recovery copy before leaving. Verified 2026-08-23. Commit: a449ac7.
- [x] **P2 · Base elapsed duration on a monotonic clock.** Active capture segments now use `performance.now()`, catch up after delayed callbacks, exclude pauses, seed from recovered duration, and checkpoint correctly even when timer callbacks skip exact five-second boundaries. Verified 2026-08-23. Commit: a2ef9f0.
- [x] **P2 · Add model repair to Settings.** Settings now has a focused speech-model health view with local file verification, resumable download/repair progress, post-download verification, and a direct route from the unavailable-transcription warning. Verified 2026-08-23. Commit: 2f88919.
- [x] **P2 · Guard navigation during recording startup.** Startup navigation now offers only stay or recovery-only exit; exit waits for draft creation, avoids starting capture when possible, or immediately stops and checkpoints if native startup already completed, without creating an empty meeting. Verified 2026-08-23. Commit: 5bc1d54.
