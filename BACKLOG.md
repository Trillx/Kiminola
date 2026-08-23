# Kimi Nola improvement backlog

Ordered by user impact: data loss and crashes first, then broken flows, test gaps, UX, accessibility, polish, and refactors. Keep each item small enough for one verified commit.

## Queued

- [ ] **P1 · Detect microphone queue pressure.** The capture callback silently drops microphone buffers when the bounded queue is full (`recording_session.rs` TODO), which can create unexplained transcript gaps.
- [ ] **P2 · Surface transcript-finalization warnings.** A timed-out final ASR flush now preserves and saves the latest snapshot, but the warning is only written to the local log.
- [ ] **P2 · Base elapsed duration on a monotonic clock.** The one-second UI interval can drift while the app is suspended or the window is hidden.

## Completed

- [x] **P0 · Block tray quit while a meeting is recording.** Active sessions now keep the process alive, restore the recording window, and explain how to save or intentionally cancel. Verified 2026-08-22.
- [x] **P0 · Continuously save recording notes for recovery.** Every recording now uses a serialized, debounced SQLite note draft; normal saves attach it, intentional cancellation removes only auto-created drafts, and crashes leave a library-visible recovery copy. Verified 2026-08-22.
- [x] **P0 · Persist in-progress transcript checkpoints.** Recovery drafts now atomically store notes, elapsed duration, and the latest transcript text; interrupted sessions display their transcript and can continue with correctly offset timing. Verified 2026-08-23.
- [x] **P1 · Surface recording startup failures.** The recording lifecycle now remains in Starting until native capture succeeds; failures stop the timer and waveform, disable invalid controls, preserve recovery notes, and offer retry plus Windows microphone settings. Verified 2026-08-23.
- [x] **P1 · Make stop-and-save failures retryable.** Stop is idempotent, finalization warnings return the best transcript snapshot, recovery drafts make meeting saves idempotent, and save failures expose retry/open-recovery actions without duplicating meetings. Verified 2026-08-23.
- [x] **P1 · Surface pause and resume failures.** In-flight capture controls cannot race Stop; pause failure leaves recording active with an explanation, while resume failure keeps the paused dialog open with retry and microphone-settings actions. Verified 2026-08-23.
