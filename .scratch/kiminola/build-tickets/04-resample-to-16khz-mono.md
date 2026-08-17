# T4 — Resample both channels to 16 kHz mono

## Goal

Add `rubato` resampling so that every audio buffer arriving from the microphone and the system loopback is converted to 16 kHz mono before it reaches the rest of the pipeline. The event task still emits placeholder transcript text, but now it operates on resampled buffers.

## Why this next

VAD (Silero) and ASR (sherpa-onnx) both expect 16 kHz mono input. Resampling is a prerequisite for every downstream audio ticket, so it gets its own tracer bullet.

## Acceptance criteria

- [x] `rubato` is added as a dependency.
- [x] Each channel has its own real-time resampler (`FastFixedIn`).
- [x] Mic buffers are resampled to 16 kHz mono and tagged as `AudioBuffer::Mic(Vec<f32>)`.
- [x] Loopback buffers are resampled to 16 kHz mono and tagged as `AudioBuffer::Loopback(Vec<f32>)`.
- [x] The resampler handles arbitrary input chunk sizes by buffering.
- [x] The event task receives 16 kHz buffers and emits placeholder events as before.
- [x] `cargo check`, `npm run check`, and `npm run build` all pass.

## Out of scope

- Voice activity detection — ticket T5.
- Real ASR (sherpa-onnx) — ticket T6.
- Dual-channel time alignment beyond same-session start/stop.
- Persisting the meeting to SQLite — ticket T7.

## Blockers

None (T3 is closed).
