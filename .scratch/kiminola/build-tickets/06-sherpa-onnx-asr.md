# T6 — Streaming ASR with sherpa-onnx

## Goal

Replace placeholder transcript events with real streaming transcription via sherpa-onnx. While recording, the 16 kHz mono VAD-gated audio is fed to a streaming ASR decoder per channel, and the recognized text is emitted as `transcript:event` events.

## Why this next

This is the core product behavior: real, local, streaming transcription. Once this ticket works, the app is genuinely testable with actual speech-to-text.

## Acceptance criteria

- [x] `sherpa-onnx` Rust binding is added as a dependency (v1.13.5, `shared` feature, prebuilt win-arm64 libs via `SHERPA_ONNX_LIB_DIR`).
- [x] A streaming ASR model is installed at `%LOCALAPPDATA%\Kiminola\models`. **Deviation:** manually installed `zipformer-en-20M` (small, fast to test) instead of the spec's Nemotron 0.6B; first-run download manager with SHA-256 (SPEC §7) and the production model swap are follow-up work, not part of this tracer.
- [x] A per-channel streaming ASR decoder is initialized when recording starts.
- [x] 16 kHz mono chunks are fed to the decoder and text is emitted as `transcript:event`.
- [x] The VAD-gated placeholder events are replaced with real partial/final transcript lines (sherpa-onnx endpointing drives finalization; VAD still runs but no longer gates).
- [x] `cargo check`, `npm run check`, and `npm run build` all pass. `cargo test` includes an ASR smoke test (model load + decode of silence).

## Resolution notes

- Runtime DLLs (`sherpa-onnx-c-api.dll`, `sherpa-onnx-cxx-api.dll`, `onnxruntime.dll`, `onnxruntime_providers_shared.dll`) are copied into `target/debug/` (and `deps/` for tests). ort loads the same `onnxruntime.dll` (1.27.1) by name — backward-compatible with ort's pinned version.
- Prebuilt libs and local models are gitignored (`src-tauri/.gitignore`); nothing large is committed.
- Updater plugin registration is commented out in `src/lib.rs` (panics without `plugins.updater` config); wire it up in the packaging phase.
- **Latency saga (all fixed and verified live):** initial 13–14 s perceived delay decomposed into (a) audio capture starting before model load → stale backlog → fixed by loading first; (b) dead VAD inference in the hot path → parked; (c) mic array signal ~-25 dBFS, too quiet for the bring-up zipformer → fixed ×4 boost (proper AGC is follow-up); (d) the 20M zipformer itself → replaced with the SPEC's **Nemotron 0.6B int8, 160 ms chunk** export (`csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25` in `%LOCALAPPDATA%\Kiminola\models\nemotron`, zipformer kept as fallback layout); (e) model load per recording → engine preloaded in background at app launch, one shared recognizer, cheap per-recording lanes; (f) transcript panel hidden by default → auto-opens on first line.
- **Dual-lane throughput:** separate per-lane decodes measured RTF 1.17–2.38 (unsustainable); batched `decode_multiple_streams` + 4 threads → **RTF 0.78**. Live stress test (speech + video audio simultaneously): backlog oscillates 0–31 and recovers; no drops.
- Verified: first text at ~1.8 s of speech live; offline replay of real mic audio gives first text at 1.1–1.9 s with complete, punctuated transcription.
- Tests in `src/asr.rs`: silence smoke, TTS WAV e2e (`.scratch/speech-test.wav`), mic-dump replay + dual-lane RTF regression (both skip gracefully if `%TEMP%\kiminola-mic-dump.f32` is absent).

## Out of scope

- Speaker diarization beyond the two channel labels.
- ASR model management UI (single default model).
- Persisting the meeting to SQLite — ticket T7.
- First-run model download manager and Nemotron 0.6B production model.

## Blockers

None (T5 is closed).
