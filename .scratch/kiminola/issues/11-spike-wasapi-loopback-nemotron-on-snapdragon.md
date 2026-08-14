# Spike: WASAPI loopback + Nemotron streaming ASR on Snapdragon X Elite

Type: task
Status: resolved

## Question

Validate the two riskiest hardware assumptions on the owner's actual machine (Snapdragon X Elite, 32 GB unified RAM) *before* the stack is locked in ticket 05:

1. **WASAPI loopback on ARM64** — ticket 04's research found Microsoft-acknowledged reports of loopback capture silently delivering zero packets on Snapdragon machines. Spike: minimal Rust windows-rs program that captures classic whole-endpoint loopback AND Process Loopback Capture while audio plays, verifying packets actually flow, for both x64-emulated and native ARM64 builds.
2. **sherpa-onnx Nemotron streaming on ARM64** — ticket 01's research picked `nemotron-speech-streaming-en-0.6b` INT8 via sherpa-onnx (pinned ≥ 1.13.4) with prebuilt Windows ARM64 binaries. Spike: run the streaming recognizer natively on ARM64 against a known audio file, measure real-time factor and RAM, confirm sane output (the < 1.13.4 silent-wrong-decode bug is the thing to rule out).

The spike is throwaway code under `.scratch/kiminola/spike/`. Success criteria: loopback packets flow natively on ARM64, and Nemotron streaming transcribes at RTF well under 1.0 with correct text. The answer records measured numbers; failure reroutes ticket 05.

## Answer

**PASS on the owner's Snapdragon X Elite Windows ARM64 machine (Windows build 26200), measured 2026-08-12.** Both hardware assumptions are cleared; ticket 05 does not need a fallback reroute.

### WASAPI loopback

The [minimal Rust probe](../spike/wasapi-loopback-spike/src/main.rs) was compiled as both a native ARM64 PE (`Machine = 0xAA64`) and an x64 PE (`Machine = 0x8664`) running under Windows emulation. A generated 440 + 660 Hz WAV played from a known PowerShell process while each build captured for five seconds.

| Build | Capture mode | Packets | Frames | Silent packets | Peak amplitude | Result |
|---|---|---:|---:|---:|---:|---|
| Native ARM64 | Classic whole-endpoint | 500 | 220,500 | 0 | 0.6664 | PASS |
| Native ARM64 | Process tree | 498 | 219,618 | 0 | 0.6663 | PASS |
| x64 emulated | Classic whole-endpoint | 500 | 220,500 | 0 | 0.6664 | PASS |
| x64 emulated | Process tree | 498 | 219,618 | 0 | 0.6662 | PASS |

The first process-loopback attempt activated successfully but returned `E_NOTIMPL` when the probe called `IAudioClient::GetMixFormat`. That was a harness error, not an ARM64 failure: Microsoft's virtual process-loopback client requires the caller to supply a capture format. The probe was corrected to match Microsoft's ApplicationLoopback sample (44.1 kHz, stereo, 16-bit PCM plus `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`), after which both architectures passed.

One preliminary run that switched directly from the ARM64 client to the x64 client inside the same playback session returned `AUDCLNT_E_DEVICE_INVALIDATED` once for x64. It did not reproduce when each shipping architecture was run in its own fresh process: both isolated x64 modes passed with full non-silent packet flow.

### Nemotron streaming ASR

The sherpa-onnx v1.13.5 online CLI was verified as a native ARM64 PE (`Machine = 0xAA64`) and run with the CPU provider and four threads against the three model-supplied known WAVs. The INT8 encoder, decoder, joiner, and tokens total **661,919,397 bytes (631.3 MiB)**.

- Two normal runs over 28.4 seconds of audio had weighted RTFs of **0.1405** and **0.1408** (per-file range **0.11-0.21**).
- A third run under active OS memory polling had weighted RTF **0.3063** (per-file range **0.27-0.39**), still comfortably below real time.
- Recognizer creation took **2.36 seconds** in the instrumented run.
- Peak working set was **907,333,632 bytes (865.3 MiB)**.
- Normalized transcript comparison produced **2 edits across 77 reference words (2.6% WER)**. The only differences were `dishonoured` -> `dishonored` and `Prynne` -> `Prynn`; output was otherwise correct and coherent. This rules out the silent wrong-decode failure.

Evidence is preserved in [the instrumented benchmark log](../spike/codex-sherpa-benchmark.err) and the two normal-run logs ([run 1](../spike/sherpa-run1.err), [run 2](../spike/sherpa-run2.err)).

### Architecture consequence

Proceed with the researched Windows pipeline: native ARM64 and x64 builds, custom `windows-rs` classic/process WASAPI loopback, and sherpa-onnx >= 1.13.4 with Nemotron streaming INT8 on CPU. Keep the runtime packet-flow health check and classic fallback from ticket 04 as production defenses, but no Snapdragon-specific architecture fallback is required on this tested machine.
