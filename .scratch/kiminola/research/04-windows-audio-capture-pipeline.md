# Research: Windows audio capture pipeline for dual-channel streaming transcription

Date: 2026-08-12
Ticket: [issues/04-windows-audio-capture-pipeline.md](../issues/04-windows-audio-capture-pipeline.md)
Scope: Windows-first (incl. ARM64) capture of mic + system loopback on two separate, time-aligned channels for local streaming ASR ("You"/"Others" labeling), plus a credible macOS/Linux path.

---

## 1. System loopback capture on Windows (WASAPI)

### 1.1 Classic WASAPI loopback (whole render endpoint)

- WASAPI loopback capture records everything the audio engine plays to a **render endpoint** — including headphones — regardless of volume/mute of the stream mix. Mechanism: get the default `eRender` endpoint via `IMMDeviceEnumerator::GetDefaultAudioEndpoint`, then `IAudioClient::Initialize(..., AUDCLNT_STREAMFLAGS_LOOPBACK, ...)` and read via `IAudioCaptureClient`. Documented in [Loopback Recording — Microsoft Learn (updated 2025-04-16)](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording).
- Properties that matter for us:
  - No permissions/prompts needed on Windows; works on any Windows 8+.
  - Capture format = the device's mix format (typically 44.1/48 kHz, 32-bit float, stereo). You always resample/downmix yourself.
  - **Silent-output gotcha**: when nothing is playing, the loopback stream delivers *no packets* (`GetNextPacketSize` stays 0) rather than explicit silence. A time-aligned pipeline must detect this and synthesize silence to keep the "Others" channel's clock advancing — this is the single most common bug in loopback-based meeting recorders (also noted in [NAudio docs](https://github.com/naudio/NAudio/blob/main/Docs/WasapiLoopbackCapture.md) and [Stack Overflow](https://stackoverflow.com/questions/52345617/how-to-record-audio-with-wasapiloopbackcapture-when-no-voice-is-coming-out-from)).
  - Headphone use is fine: loopback taps the render mix *before* the DAC, so output device type is irrelevant.

### 1.2 Process Loopback Capture (Windows 10 build 20348+)

- `ActivateAudioInterfaceAsync` with `AUDIOCLIENT_ACTIVATION_PARAMS` → `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK` + `AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS` lets you capture **only a specific process (tree)** (`PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`) or **everything except** one process tree (`...EXCLUDE_TARGET_PROCESS_TREE`). Docs: [AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params), [AUDIOCLIENT_ACTIVATION_TYPE](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ne-audioclientactivationparams-audioclient_activation_type) — both "Minimum supported client: Windows 10 Build 20348".
- Official C++ sample: [Application loopback audio capture sample](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/).
- Community reports ([win-capture-audio #14](https://github.com/bozbez/win-capture-audio/issues/14), 2021-08) say it also works on Windows 10 2004+ with just the newer SDK headers, but **Microsoft documents 20348** — treat 20348 as the floor.
- Value for Kiminola: targeting the meeting app's PID (Teams/Zoom/Chrome) avoids capturing notification sounds, music, etc. in the "Others" channel. Caveat: some meeting apps render audio from a helper process or move PIDs (reported for Teams: [Stack Overflow 2025-11](https://stackoverflow.com/questions/79832526/wasapi-application-loopback-is-unable-to-record-ms-teams)), so PID targeting needs a robust "find the audio-rendering process" heuristic (walk `IAudioSessionManager2` sessions) plus a fallback to whole-endpoint loopback.

### 1.3 ARM64 status (important gotcha)

- There is **no Microsoft documentation confirming WASAPI loopback parity on Windows ARM64**. A [Microsoft Q&A answer (2026-01-06)](https://learn.microsoft.com/en-us/answers/questions/5694431/coreaudio-wasapi-loopback-on-windows-11-arm-iaudio) states the APIs are present and callable on ARM64 but documents no support guarantees, and acknowledges **multiple developer reports of `IAudioCaptureClient::GetNextPacketSize()` returning 0 forever during active playback on Snapdragon devices**, with the same code working on x64. Not formally classified as a bug; no ARM-specific flags documented.
- Practical implication: Kiminola must **smoke-test loopback on real ARM64 hardware early** (a capture spike ticket), ship with runtime verification (capture a test tone / verify packets flow, fall back gracefully), and not assume x64-validated code works on ARM64. Some ARM devices reportedly need vendor-specific paths.
- Note the zero-packet failure mode is *indistinguishable in shape* from the normal "silence" behavior in §1.1 — another reason the pipeline must synthesize silence from the clock rather than trusting packet flow, and why we need an explicit "is loopback actually delivering audio while audio is playing" health check.

### 1.4 Libraries that wrap this

**Rust:**
- **[`windows` (windows-rs)](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Media/Audio/struct.AUDIOCLIENT_ACTIVATION_PARAMS.html)** — full raw bindings: `AUDIOCLIENT_ACTIVATION_PARAMS`, `ActivateAudioInterfaceAsync`, all WASAPI COM interfaces. This is the flexible, zero-black-box route. COM capture objects are `!Send`/`!Sync`, so the standard pattern is a dedicated capture thread per stream pushing samples over a channel (as done by [flexaudio-os-windows](https://lib.rs/crates/flexaudio-os-windows), MIT, windows-rs 0.54 — a small, readable reference implementation of both system loopback and process-tree loopback).
- **[`wasapi` crate](https://crates.io/crates/wasapi)** (Henrik Enquist, author of CamillaDSP) — safe-ish Rust wrapper over WASAPI incl. loopback capture and event-driven capture; does not wrap the process-loopback activation itself, but can be combined with windows-rs for the activation call. Good mic-capture wrapper too.
- **[cpal](https://github.com/RustAudio/cpal)** — cross-platform host/device/stream abstraction; WASAPI backend on Windows. **cpal does not expose WASAPI loopback capture** (checked CHANGELOG through 0.17, 2026-08: no WASAPI loopback entry; CoreAudio loopback was added but not WASAPI). Use cpal for the mic and for macOS/Linux, not for Windows loopback. Historical issues show WASAPI loopback-via-cpal attempts failing (e.g. [cpal #516](https://github.com/RustAudio/cpal/issues/516)).

**C#:**
- **[NAudio](https://github.com/naudio/NAudio)** — `WasapiLoopbackCapture` (legacy) / `WasapiRecorder.WithLoopbackCapture()` (NAudio 3, zero-copy + MMCSS) wraps classic whole-endpoint loopback well. Per-process loopback is **not** wrapped ([NAudio #878](https://github.com/naudio/NAudio/issues/878), open since 2022); C# projects do it via CsWin32 P/Invoke of `ActivateAudioInterfaceAsync` (pattern documented in [LocalScribe's capture spike plan](https://github.com/imnotwallace/LocalScribe/blob/master/docs/plans/2026-06-30-stage-1-capture-spike.md)).

**C++:** the Microsoft sample above; OBS's `win-wasapi` plugin and Firefox's [cubeb_wasapi.cpp](https://searchfox.org/mozilla-central/source/media/libcubeb/src/cubeb_wasapi.cpp) are battle-tested references (`AUDCLNT_STREAMFLAGS_LOOPBACK`; note event callbacks don't work with loopback in cubeb's experience — polling/timer-driven capture is safer).

**Recommendation:** Rust + windows-rs (own thin WASAPI module, modeled on the Microsoft sample / flexaudio-os-windows) with `wasapi` crate as an acceptable shortcut for classic loopback. This avoids NAudio's missing process-loopback support and cpal's missing WASAPI-loopback support while keeping one language across the app.

---

## 2. Mic capture & echo cancellation

- Mic capture is a standard WASAPI capture stream (`eCapture` endpoint, shared mode, event-driven). Via cpal (fine for mic) or the same custom WASAPI module — using one custom module for both channels keeps timestamp handling uniform, which matters for §3.
- **AEC**: needed only when the user is on speakers; on headphones loopback audio can't leak into the mic. Even on speakers, because Kiminola captures the *exact* render stream digitally, we have the ideal AEC reference signal — no need for the OS's AEC. Options:
  - WebRTC AEC3 ports in Rust: [aec3-rs](https://github.com/RubyBit/aec3-rs) (pure-Rust WebRTC AEC3 port, 2025-11), [sonora](https://github.com/dignifiedquire/sonora) (pure-Rust WebRTC audio processing: AEC + NS + AGC, 2026-02).
  - SpeexDSP-based: [aec-rs](https://github.com/thewh1teagle/aec) — explicitly supports Windows/Linux/macOS **ARM64 and x64** with precompiled libs.
  - Windows also offers `AUDCLNT_STREAMFLAGS_*` + the Voice Capture DSP / Communications mode with built-in AEC, but that entangles us with device-mode quirks; an in-app AEC against our own loopback reference is more controllable. Microsoft itself notes WASAPI loopback "is provided primarily to support acoustic echo cancellation" ([Loopback Recording doc](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)).
- **Noise suppression**: DeepFilterNet (pure-Rust via `tract`, used in production by [flow orchestrator](https://github.com/native-logic-technologies/flow), 2026-03) is the current best-in-class open option; RNNoise (C, trivially bindable) is the lighter fallback. NS is optional v1 — transcription models tolerate noise better than humans do.

---

## 3. Dual-channel separation & time alignment

- Keep mic and loopback as **two independent capture streams into two independent ring buffers** — never mix them pre-ASR. Separation is what enables "You"/"Others" labeling without diarization.
- Time alignment strategy:
  1. Stamp every captured buffer with a **common monotonic clock**: `QueryPerformanceCounter`. WASAPI gives you `pu64QPCPosition` (and device position) per buffer from `IAudioCaptureClient::GetBuffer`, so each channel's samples can be mapped onto the shared QPC timeline; cpal exposes equivalent `InputStreamTimestamp`s.
  2. Both channels run at the same resampled rate (16 kHz mono, §4), so after resampling, alignment = sample-count arithmetic from each stream's start timestamp; correct for **clock drift** between the two devices' hardware clocks using the WASAPI device-position/QPC pairs (or an adaptive resampler ratio — rubato's async resamplers accept a dynamic ratio).
  3. **Synthesize silence** on the loopback channel whenever the endpoint is idle (§1.1) so the "Others" timeline never stalls. Same for mic gaps after device re-plug.
- The ASR/VAD consumers then read *aligned windows* per channel, and transcript segments are labeled by channel — "You" = mic, "Others" = loopback — with timestamps from the shared clock.

---

## 4. Streaming conditioning (16 kHz mono, chunking, VAD)

- **Resample/downmix**: WASAPI gives float32 stereo at device rate. Downmix to mono (mean of channels; loopback meeting audio is effectively mono-compatible) and resample to 16 kHz with [rubato](https://github.com/HEnquist/rubato) (pure Rust, sinc/FFT resamplers, async variant supports drift-correcting dynamic ratios; no C deps so ARM64-safe). NAudio's equivalent (WdlResamplingSampleProvider/MediaFoundationResampler) if the app were C#.
- **Chunking/windowing**: standard real-time stack pattern — 30 ms frames (480 samples @16k) as the atomic unit; VAD per frame; ASR consumes VAD-gated utterance segments with ~200–300 ms pre-roll (pre-activation buffer) and ~500–700 ms trailing-silence hangover. This is the proven pattern from e.g. [GLaDOS](https://github.com/dnhkng/GLaDOS) (Silero VAD 32 ms chunks, 800 ms pre-buffer, 640 ms pause cutoff → ASR) and sherpa-onnx integrations ([streaming Zipformer at 100 ms chunks](https://github.com/voiceping-ai/ios-mac-offline-transcribe)).
- For Whisper-family (non-streaming) models: sliding-window batch decoding with overlap + LocalAgreement-2 merging (only commit text agreed by two consecutive windows) — as implemented in practice by [omni-voice ADR-0033/0034](https://github.com/rust-works/omni-voice/issues/7) and catalogued in [rift-transcription's provider comparison](https://github.com/Leftium/rift-transcription/blob/main/reference/whispering-io-analysis.md). For true streaming ASR, [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) streaming Zipformer accepts raw 16 kHz mono PCM in ~100 ms chunks — simplest contract for our pipeline.
- **VAD**: [Silero VAD](https://github.com/snakers4/silero-vad) v5/v6 (ONNX, CPU-only, <1 ms/frame, 512-sample/32 ms windows @16 kHz) is the de-facto choice; Rust integrations: [silero-vad-rs](https://crates.io/crates/silero-vad-rs) (`ort` ONNX runtime), [vad-rs](https://github.com/lackmannicholas/vad-rs) (multi-model: silero, ten_vad, energy fallback). **ARM64 note**: ONNX Runtime ships Windows ARM64 builds and `ort` supports them, but verify the `ort` download strategy on win-arm64 in a spike. Keep an RMS-energy VAD as zero-dependency fallback. VAD saves ASR compute and (equally important) gives natural utterance boundaries for transcript segmentation on both channels.

---

## 5. Cross-platform path

| Platform | System-audio capture | Mic | Abstraction coverage |
|---|---|---|---|
| Windows 10 20348+ | WASAPI process loopback (`AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS`) or classic `AUDCLNT_STREAMFLAGS_LOOPBACK` | WASAPI capture | Custom windows-rs module (cpal covers mic only) |
| macOS 14.2+ (taps) / **14.6+ (cpal)** | Core Audio **process taps** (`CATapDescription` on an aggregate device — captures per-process or system mix even to headphones; needs one-time "system audio recording" consent) — [Apple doc](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps), deep dive: [recall.ai](https://www.recall.ai/blog/core-audio-taps) (2026-07) | CoreAudio | **cpal ≥ 0.16 supports CoreAudio loopback recording on macOS 14.6+** ([CHANGELOG](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md); caveat: fresh API with several loopback bug fixes landing through 0.17 — tap auto-start, aggregate-UID collisions, silence bugs — pin a recent cpal and test) |
| Linux | PipeWire/PulseAudio **monitor source** of the default sink (every sink exposes a `.monitor` source); per-app capture via PipeWire stream targeting or `pw-loopback` virtual sink | PipeWire/Pulse source | **cpal 0.16+ has native PipeWire and PulseAudio hosts** (PipeWire 0.3.53+, default host priority PipeWire > PulseAudio > ALSA). Gotcha: monitor-source naming lives in the Pulse/PipeWire namespace — open via the PulseAudio host (device name `*.monitor`) or `PULSE_SOURCE` env, not ALSA ([minutes #69](https://github.com/silverstein/minutes/issues/69)). Fallback: `parec` subprocess. |

Precedent that this exact trio works: [persona](https://github.com/xikhar/persona) (2026-07) ships per-app playback capture on all three: "Windows: WASAPI process-loopback capture (requires 20348+) / Linux: PipeWire playback-stream capture / macOS 14.2+: Core Audio process tap". ScreenCaptureKit is the alternative macOS path (used by [huxinhai/audio-capture](https://github.com/huxinhai/audiotee-wasapi)) but process taps are lighter and don't involve screen-capture permission UX.

**Abstraction verdict:** cpal is the right *common* layer for mic everywhere + macOS loopback + Linux monitor capture; Windows loopback needs a small custom windows-rs module behind the same internal `AudioSource` trait (samples + QPC-based timestamps out). C# alternative: NAudio + CsWin32 on Windows, but then macOS/Linux need entirely separate code — another point for Rust.

---

## 6. Recommended pipeline architecture

```
 Windows                          macOS 14.6+                     Linux
 ┌─────────────────────┐         ┌───────────────────┐          ┌────────────────────────┐
 │ windows-rs WASAPI   │         │ cpal CoreAudio    │          │ cpal Pulse/PipeWire    │
 │ process loopback    │         │ loopback (tap)    │          │ *.monitor source       │
 │ (fallback: classic  │         └─────────┬─────────┘          └───────────┬────────────┘
 │  render loopback)   │                   │                                │
 └─────────┬───────────┘                   │                                │
           │ f32 stereo @ device rate + QPC/dev-position timestamps         │
           ▼                                                                
 ┌──────────────────────────────────────────────────────────────────────┐
 │ "Others" lane: ring buffer → silence synth on idle → mono downmix    │
 │   → rubato resample → 16 kHz mono                                    │
 ├──────────────────────────────────────────────────────────────────────┤
 │ "You" lane: mic (cpal/WASAPI) → same timestamps → [optional AEC3 vs  │
 │   loopback reference when on speakers] → ring buffer → downmix       │
 │   → rubato → 16 kHz mono                                             │
 ├──────────────────────────────────────────────────────────────────────┤
 │ Aligner: map both lanes to shared monotonic clock, drift-correct via │
 │   resample-ratio nudging, emit aligned 30 ms frame pairs             │
 ├──────────────────────────────────────────────────────────────────────┤
 │ Per-lane Silero VAD (32 ms) → utterance segments (±pre-roll/hangover)│
 ├──────────────────────────────────────────────────────────────────────┤
 │ Local ASR per lane (sherpa-onnx streaming Zipformer, or Whisper      │
 │   sliding-window + LocalAgreement) → labeled "You"/"Others" segments │
 │   with shared-clock timestamps                                       │
 └──────────────────────────────────────────────────────────────────────┘
```

Library shortlist: `windows` (windows-rs, WASAPI incl. process loopback), `cpal` 0.17 (mic everywhere; macOS/Linux system audio), `rubato` (resample/drift), `ort` + Silero VAD ONNX (VAD), `aec3-rs` or `aec-rs` (optional speaker-mode AEC), sherpa-onnx or whisper.cpp/candle (ASR — out of scope here but shapes the 16 kHz mono contract).

---

## 7. ARM64 gotchas (consolidated)

1. **WASAPI loopback reliability unverified on ARM64** — multiple reports of zero-packet loopback streams on Snapdragon with x64-working code; no Microsoft parity statement ([Q&A 2026-01-06](https://learn.microsoft.com/en-us/answers/questions/5694431/coreaudio-wasapi-loopback-on-windows-11-arm-iaudio)). → Early hardware spike + runtime health check + clear degradation UX. Process-loopback vs classic loopback behavior on ARM64 both need testing.
2. Zero-packet failure looks identical to normal idle-silence — the pipeline's silence-synthesis design must include a "packets should be flowing but aren't" detector.
3. `ort` (ONNX Runtime for Silero VAD) — win-arm64 binaries exist but confirm the crate's download/linking path on ARM64; keep energy-VAD fallback.
4. SpeexDSP AEC (`aec-rs`) ships precompiled Windows ARM64; WebRTC AEC3 Rust ports are pure Rust (compile anywhere) but are young — validate quality before relying on them.
5. Everything else recommended (`windows-rs`, `rubato`, `cpal`) is pure Rust or first-party bindings with no known win-arm64 issues; cpal added native ARM64 Linux CI ([CHANGELOG](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md)) and windows-rs targets aarch64-pc-windows-msvc officially.

## Sources (all accessed 2026-08-12)

- [Loopback Recording — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording) (doc updated 2025-04-16)
- [AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params)
- [AUDIOCLIENT_ACTIVATION_TYPE — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ne-audioclientactivationparams-audioclient_activation_type)
- [Application loopback audio capture sample — Microsoft Learn](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/)
- [CoreAudio WASAPI Loopback on Windows 11 ARM — Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/5694431/coreaudio-wasapi-loopback-on-windows-11-arm-iaudio) (2026-01-06)
- [RustAudio/cpal README + CHANGELOG](https://github.com/RustAudio/cpal)
- [wasapi crate](https://crates.io/crates/wasapi) (2025-04)
- [flexaudio-os-windows](https://lib.rs/crates/flexaudio-os-windows) (2026-07)
- [NAudio WasapiLoopbackCapture docs](https://github.com/naudio/NAudio/blob/main/Docs/WasapiLoopbackCapture.md), [NAudio #878 per-process request](https://github.com/naudio/NAudio/issues/878)
- [Capturing system audio with Core Audio taps — Apple Developer](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps), [CoreAudioTaps deep dive — recall.ai](https://www.recall.ai/blog/core-audio-taps) (2026-07)
- [cpal PipeWire/PulseAudio hosts + monitor-source gotcha — silverstein/minutes #69](https://github.com/silverstein/minutes/issues/69) (2026-04)
- [persona — three-platform process capture precedent](https://github.com/xikhar/persona) (2026-07)
- [aec3-rs](https://github.com/RubyBit/aec3-rs), [sonora](https://github.com/dignifiedquire/sonora), [aec-rs (speexdsp, ARM64 prebuilt)](https://github.com/thewh1teagle/aec)
- [rubato](https://github.com/HEnquist/rubato)
- [silero-vad-rs](https://crates.io/crates/silero-vad-rs), [vad-rs](https://github.com/lackmannicholas/vad-rs), [GLaDOS pipeline pattern](https://github.com/dnhkng/GLaDOS)
- [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), [omni-voice streaming ASR issue](https://github.com/rust-works/omni-voice/issues/7), [rift-transcription provider comparison](https://github.com/Leftium/rift-transcription/blob/main/reference/whispering-io-analysis.md)
- [bozbez/win-capture-audio #14 — process loopback notes](https://github.com/bozbez/win-capture-audio/issues/14)
- [Stack Overflow: process loopback can't record Teams](https://stackoverflow.com/questions/79832526/wasapi-application-loopback-is-unable-to-record-ms-teams) (2025-11)
