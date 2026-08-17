# T2 — Real microphone capture

## Goal

Replace the placeholder event loop in the backend with real microphone input via `cpal`. While recording, mic audio flows into the recording session; the session still emits placeholder transcript events, but now they are tied to an active audio stream instead of a hard-coded timer loop.

## Why this next

T1 proved the command/event plumbing. This ticket de-risks the first real audio integration: initializing `cpal`, selecting an input device, building a stream, and shutting it down cleanly when recording stops. System loopback (WASAPI) and ASR plug in later.

## Acceptance criteria

- [x] `cpal` is added as a dependency and compiles on the target platform.
- [x] `start_recording` opens the default microphone input stream.
- [x] Mic samples flow from the cpal callback into the recording session via a tokio channel.
- [x] While the stream is alive, the backend emits placeholder `transcript:event` events (still fake text) on a heartbeat once audio has been heard.
- [x] `stop_recording` drops the cpal stream and stops the event task cleanly.
- [x] `cargo check`, `npm run check`, and `npm run build` all pass.

## Out of scope

- System loopback / WASAPI capture — ticket T3.
- VAD (Silero) — ticket T4.
- Real ASR (sherpa-onnx) — ticket T5.
- Resampling to 16 kHz mono — ticket T6.
- Persisting audio or transcript to SQLite — ticket T7.

## Blockers

None (T1 is closed).
