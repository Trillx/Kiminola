# T5 — Voice activity detection with Silero via ort

## Goal

Add Silero VAD so the recording session only emits placeholder transcript events while speech is detected. This makes the app's live transcript feel reactive to real audio before real ASR lands.

## Why this next

The app already captures, resamples, and displays placeholder text. Gating that display on actual speech is the next meaningful testable milestone and a required preprocessing step before ASR.

## Acceptance criteria

- [x] `ort` is added as a dependency.
- [x] The Silero VAD ONNX model is downloaded to `%LOCALAPPDATA%\Kiminola\models` for this tracer bullet.
- [x] A `VadSession` loads the model and maintains LSTM state across chunks.
- [x] 16 kHz mono chunks are scored for speech probability.
- [x] Speech start/end is tracked with simple hysteresis.
- [x] Placeholder transcript events are only emitted while the VAD reports speech.
- [x] `cargo check`, `npm run check`, and `npm run build` all pass.

## Out of scope

- Real transcription (sherpa-onnx) — ticket T6.
- ASR model download/management — ticket T6.
- Persisting meetings to SQLite — ticket T7.

## Blockers

None (T4 is closed).
