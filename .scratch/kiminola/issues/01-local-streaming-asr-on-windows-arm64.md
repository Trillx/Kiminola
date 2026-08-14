# Local streaming ASR on Windows ARM64

Type: research
Status: resolved

## Question

Which local **streaming** speech-to-text models and runtimes actually work well on Windows ARM64 today — specifically on a Snapdragon X Elite with 32 GB unified RAM? The user has named NVIDIA Parakeet as the reference point; evaluate it and the alternatives.

Cover at minimum:

- **Parakeet family** (NeMo parakeet-tdt / ctc / streaming variants): which variants support true streaming, accuracy (WER), and how they run locally — sherpa-onnx, ONNX Runtime, NeMo? License (CC-BY-4.0?) and redistribution terms for model weights.
- **Alternatives**: Moonshine, whisper.cpp (and its streaming limitations), Vosk, SenseVoice, anything else credible for live captions in 2025-26. English-first is fine.
- **Runtimes on Snapdragon X Elite**: ONNX Runtime execution providers (QNN/NPU via Hexagon, CPU, GPU), sherpa-onnx ARM64 Windows support, whisper.cpp ARM64. What actually ships Windows ARM64 binaries or builds cleanly?
- **Expected performance**: real-time factor / latency on this class of hardware for the top 2-3 candidates; RAM footprint per model size.
- **Model distribution**: sizes, Hugging Face availability, on-demand download practicality.

Output: a ranked recommendation of model + runtime for the MVP, with evidence and links.

## Answer

Resolved 2026-08-12. Full findings: [research/01-local-streaming-asr-on-windows-arm64.md](../research/01-local-streaming-asr-on-windows-arm64.md).

1. **MVP pick: NVIDIA Nemotron streaming 0.6B (cache-aware FastConformer-RNNT) via sherpa-onnx, CPU provider, INT8 ONNX.** True streaming (80 ms–1.12 s configurable chunks), 6.93% avg WER at 1.12 s chunks, ~632 MB (EN) / ~650 MB (multilingual 40-locale) download from HF on first run. sherpa-onnx ships prebuilt Windows ARM64 binaries, NuGet, and Python wheels; proven in production by OpenWhispr on desktop CPUs. Use `nemotron-speech-streaming-en-0.6b` (NVIDIA Open Model License) or `nemotron-3.5-asr-streaming-0.6b` (OpenMDW-1.1); both permit commercial use and redistribution.
2. **Fallback/accuracy option:** Parakeet-TDT 0.6B v2/v3 (CC-BY-4.0) via sherpa-onnx — better batch WER (6.05%) but offline only (VAD-chunked "simulated streaming").
3. **Also-rans:** Moonshine Medium Streaming (245M, MIT, 6.65% WER) is excellent but its Windows prebuilt is x86_64-only (ARM64 = untested source build); whisper.cpp has no true streaming; Vosk explicitly does not support Windows ARM64; SenseVoice is offline/pseudo-streaming with a murkier model license.
4. **Runtime:** ONNX Runtime CPU on win-arm64 is the path; Hexagon NPU via QNN EP exists but is unproven for these graphs — post-MVP spike at most. Expected RTF ≪ 0.1 on X Elite; ~1–1.5 GB RAM per loaded engine; mic + loopback = two streams on one loaded model.
5. **Gotchas recorded:** pin sherpa-onnx ≥ 1.13.4 (older silently mis-decodes Nemotron); Nemotron online recognizer is greedy-search-only (no hotwords); streaming partials self-revise (UI must replace-in-place).
