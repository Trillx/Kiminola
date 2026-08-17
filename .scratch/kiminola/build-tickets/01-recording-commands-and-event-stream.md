# T1 — Recording commands + transcript event stream

## Goal

Replace the frontend's mock transcript simulation with real Tauri command/event plumbing. After this ticket, clicking **New meeting** → **Stop meeting** exercises the actual backend invoke flow, even though the backend still emits placeholder transcript events.

## Why this first

It establishes the frontend↔backend contract that every audio/ASR slice will use: `start_recording`/`stop_recording` commands plus a typed `transcript:event` stream. Real audio capture and ASR plug into this same shape in later tickets.

## Acceptance criteria

- [x] Rust backend exposes `start_recording` and `stop_recording` Tauri commands.
- [x] While recording, the backend emits `transcript:event` events from a background task.
- [x] Events carry channel (`"you" | "others"`), text, and a flag indicating whether the line is partial/final.
- [x] Frontend `record/+page.svelte` calls `start_recording` on mount and `stop_recording` on stop/cancel.
- [x] Frontend listens for `transcript:event` and drives `LiveTranscript` from real events instead of `liveSimulation`.
- [x] `cargo check`, `npm run check`, and `npm run build` all pass.

## Out of scope

- Real audio capture (cpal/WASAPI) — ticket T2.
- Real ASR (sherpa-onnx) — ticket T3.
- Persisting the meeting to SQLite — ticket T4.

## Blockers

None.
