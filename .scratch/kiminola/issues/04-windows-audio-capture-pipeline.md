# Windows audio capture pipeline for dual-channel streaming transcription

Type: research
Status: resolved

## Question

What is the right audio capture pipeline for Kiminola on Windows (ARM64 included), with a credible cross-platform path?

Cover:

- **System loopback capture**: WASAPI loopback (render endpoint capture) on Windows — per-process audio capture APIs (e.g. `AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS` / Process Loopback Capture introduced in Windows 10 20348+), capturing meeting-app audio even when it plays to headphones; ARM64 support status; Rust/C#/C++ crates and libraries that wrap this well (e.g. cpal, windows-rs, NAudio).
- **Mic capture**: standard capture, echo cancellation considerations (AEC when speakers are used), noise suppression options (RNNoise?).
- **Dual-channel sync**: keeping mic and loopback channels separate and time-aligned for "You"/"Others" labeling.
- **Streaming conditioning**: resampling to model input rates (16 kHz mono), chunking/windowing strategy for streaming ASR, VAD (voice activity detection — silero-vad? energy-based?) to segment and save compute.
- **Cross-platform path**: CoreAudio (macOS) and PipeWire/Pulse (Linux) equivalents, and which abstraction layers (cpal, etc.) cover them.

Output: a recommended pipeline architecture (libraries + data flow), with notes on ARM64 gotchas and links.

## Answer

Full findings: [research/04-windows-audio-capture-pipeline.md](../research/04-windows-audio-capture-pipeline.md)

Recommended pipeline (Rust):
1. **Windows loopback**: custom windows-rs WASAPI module — Process Loopback Capture (`ActivateAudioInterfaceAsync` + `AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS`, Win10 20348+) targeting the meeting app's process tree, with classic render-endpoint loopback (`AUDCLNT_STREAMFLAGS_LOOPBACK`) as fallback. cpal does NOT wrap WASAPI loopback; NAudio wraps only classic loopback.
2. **Mic**: cpal (or the same WASAPI module), 30 ms frames, timestamps on a shared QueryPerformanceCounter clock.
3. **Separation/alignment**: two independent ring buffers, never mixed; per-buffer QPC/device-position timestamps; synthesize silence when loopback is idle (packets simply stop when nothing plays); drift-correct via rubato async resample ratio.
4. **Conditioning**: downmix to mono, rubato → 16 kHz; Silero VAD (ONNX via `ort`, 32 ms windows, energy-VAD fallback) per lane for utterance gating; feed local streaming ASR (sherpa-onnx Zipformer, or Whisper sliding-window + LocalAgreement), labeling "You"/"Others" by lane.
5. **AEC**: optional, only for speaker setups — Rust WebRTC AEC3 (`aec3-rs`) or speexdsp (`aec-rs`, prebuilt win-ARM64) using our own loopback stream as the reference.
6. **macOS**: cpal ≥ 0.16 CoreAudio loopback recording (Core Audio process taps, macOS 14.6+). **Linux**: cpal PipeWire/PulseAudio hosts capturing the default sink's `.monitor` source.
7. **ARM64 risk**: WASAPI loopback is undocumented/unverified on Windows ARM64 — reports of zero-packet streams on Snapdragon where x64 code works. Requires an early hardware spike, a runtime "packets flowing?" health check, and graceful degradation.
