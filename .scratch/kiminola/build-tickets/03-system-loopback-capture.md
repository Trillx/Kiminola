# T3 — System loopback capture (WASAPI)

## Goal

Add a second audio channel: system loopback audio captured via `windows-rs` WASAPI. While recording, both the microphone (`"You"`) and the system mix (`"Others"`) flow into the recording session, and placeholder transcript events are emitted for both channels.

## Why this next

T2 proved real mic capture. This ticket de-risks the second, Windows-specific audio integration: activating a WASAPI loopback client, reading capture packets, and mixing them into the same event stream. VAD and ASR operate on these two channels in later tickets.

## Acceptance criteria

- [x] `windows-rs` is added as a dependency with the WASAPI/audio features needed.
- [x] `start_recording` opens a WASAPI loopback capture stream on the default render endpoint.
- [x] Loopback samples flow from the capture thread into the recording session alongside mic samples.
- [x] Audio buffers are tagged by channel (`You` / `Others`).
- [x] While both streams are alive, the backend emits placeholder `transcript:event` events for both `You` and `Others`.
- [x] `stop_recording` stops the loopback capture and cleans up COM/resources.
- [x] `cargo check`, `npm run check`, and `npm run build` all pass.

## Out of scope

- Time-alignment of the two channels beyond same-session start/stop.
- Resampling both channels to a common 16 kHz mono rate — ticket T6.
- VAD (Silero) — ticket T4.
- Real ASR (sherpa-onnx) — ticket T5.
- Persisting the meeting to SQLite — ticket T7.

## Blockers

None (T2 is closed).
