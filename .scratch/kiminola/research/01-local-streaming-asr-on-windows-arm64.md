# Research 01 — Local streaming ASR on Windows ARM64 (Snapdragon X Elite)

Date researched: 2026-08-12. All sources are primary (vendor model cards, official docs, runtime repos) unless noted.

## TL;DR — ranked recommendation

1. **NVIDIA Nemotron streaming 0.6B (cache-aware FastConformer-RNNT) via sherpa-onnx, CPU, INT8** — the MVP pick. True streaming, ~6.9% avg WER at 1.12s chunks, ~632 MB INT8 ONNX, runs comfortably in real time on laptop CPUs, and sherpa-onnx ships **prebuilt Windows ARM64 binaries, NuGet packages, and Python wheels**. Proven in production by OpenWhispr (open-source Electron dictation app) doing exactly this on desktop CPUs.
   - English-only: [`nvidia/nemotron-speech-streaming-en-0.6b`](https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b) (NVIDIA Open Model License)
   - Multilingual (40 locales, auto lang-detect): [`nvidia/nemotron-3.5-asr-streaming-0.6b`](https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b) (OpenMDW-1.1)
2. **Moonshine Medium Streaming (245M, MIT) via its own C++/ONNX-Runtime core** — best accuracy-per-parameter streaming model (6.65% WER beats Whisper large-v3), tiny footprint, but Windows prebuilt is x86_64-only today; Windows ARM64 needs a from-source build (feasible — portable C++ core on ONNX Runtime — but unproven).
3. **Parakeet-TDT 0.6B v2/v3 (CC-BY-4.0) via sherpa-onnx** — best batch accuracy (6.05% avg WER) and fully working on Windows ARM64, but **not a streaming model**: live captions come from VAD + chunked re-decode ("simulated streaming"). Good as the high-accuracy offline/final-pass option, or the fallback.
4. whisper.cpp, Vosk, SenseVoice: not recommended for this use case (details below).

## Parakeet family — which variants actually stream

"Parakeet" is now three different things; do not conflate them:

| Model | Streaming? | Avg WER (Open ASR Leaderboard) | License | sherpa-onnx package |
|---|---|---|---|---|
| `parakeet-tdt-0.6b-v2` (EN, 600M) | No (offline; full-attention, up to 24 min/pass) | 6.05% | CC-BY-4.0 | `sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8` (~640 MB total: encoder 622 MB + decoder/joiner) |
| `parakeet-tdt-0.6b-v3` (25 EU langs) | No | similar class | CC-BY-4.0 | `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8` (~640 MB) |
| `parakeet-unified-en-0.6b` | Checkpoint supports both; **sherpa-onnx ONNX export is non-streaming only** | ~5.9% class | NVIDIA Open Model License | `sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming` |
| **`nemotron-speech-streaming-en-0.6b`** (600M) | **Yes — cache-aware FastConformer-RNNT, chunks 80/160/560/1120 ms** | 6.93% @ 1.12s, 7.07% @ 0.56s, 7.67% @ 0.16s, 8.43% @ 0.08s | NVIDIA Open Model License | `sherpa-onnx-nemotron-speech-streaming-en-0.6b-{80ms,560ms}-int8-2026-04-25` (~632 MB) |
| **`nemotron-3.5-asr-streaming-0.6b`** (600M, 40 locales) | **Yes — same cache-aware architecture** | competitive across languages; punctuation/casing built-in | OpenMDW-1.1 | `sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11` (~650 MB) |

Key architecture point: cache-aware streaming keeps encoder caches across chunks — compute per chunk is constant regardless of recording length (unlike buffered/sliding-window approaches), which is what makes CPU-only streaming viable. The latency/accuracy operating point is a runtime knob (`att_context_size` right-context ∈ {0,1,6,13} → 80 ms–1.12 s chunks), no retraining needed.

### How to run them

- **sherpa-onnx** (C++, ONNX Runtime underneath, no Python): supports Nemotron streaming natively as an *online* recognizer (`OnlineRecognizerTransducerNeMoImpl`) — websocket server, microphone binaries, and 12 language bindings. This is the production-proven path. Caveats: greedy search only for the Nemotron online impl today (no modified_beam_search/hotwords — [sherpa-onnx#3572](https://github.com/k2-fsa/sherpa-onnx/issues/3572)), and you need sherpa-onnx ≥ 1.13.4 or the Nemotron models decode *silently wrong* (OpenWhispr hit this).
- **NeMo**: full training/inference framework; PyTorch + Linux-centric. Fine for experimentation, wrong tool for a Windows desktop app.
- **HF Transformers ≥ 5.13.0**: supports the streaming checkpoint (`AutoModelForRNNT` + streaming processor API). Python-only; not a shipping path for us but useful for reference decoding.
- **parakeet.cpp / NeMo-Speech.cpp** ([mudler/parakeet.cpp](https://github.com/mudler/parakeet.cpp), ggml/GGUF-based): converts Parakeet/Nemotron checkpoints to GGUF, runs on CPU with prebuilt binaries for macOS arm64 and Linux x64/arm64. **No Windows prebuilt binaries**; ggml does build on Windows ARM64, but this port's Windows/ARM64 status is untested. Interesting future option (single-binary, no ONNX Runtime dependency), not MVP.

### Licenses and redistribution

- **CC-BY-4.0** (Parakeet TDT v2/v3): redistribution of weights allowed with attribution. Fine for an MIT-licensed app; include attribution + license copy.
- **NVIDIA Open Model License** (Nemotron Speech Streaming EN): "Works are commercially usable… free to create and distribute Derivative Works… no claim on outputs" ([NVIDIA Open Model Agreement, 2026-04-02](https://www.nvidia.com/content/dam/en-zz/Solutions/license-agreements/enterprise-services/nvidia-open-model-agreement-2026-04-02.pdf)).
- **OpenMDW-1.1** (Nemotron 3.5 ASR): Linux-Foundation permissive model license; commercial use, derivatives, and redistribution allowed, no field-of-use restrictions ([openmdw.ai](https://openmdw.ai/about/), [SPDX entry for 1.0](https://spdx.org/licenses/OpenMDW-1.0.html)).
- Practical approach used by comparable OSS apps (Earshot, OpenWhispr): **do not bundle weights; download on demand from Hugging Face at first run**. That sidesteps most redistribution questions and keeps installer size sane. All candidate models are on HF.

## Alternatives

- **Moonshine** ([moonshine-ai/moonshine](https://github.com/moonshine-ai/moonshine), MIT). Gen-2 "Streaming" models are the strongest small-model option: Medium Streaming 245M @ **6.65% WER** (beats Whisper large-v3's 7.44%), Small Streaming 123M @ 7.84%, Tiny Streaming 34M @ 12.0%. Caching encoder+partial decoder state gives very low latency (74 ms on M-series MBP, 269 ms on Linux x86 for Medium). Portable C++ core over ONNX Runtime with Python/Swift/Java/JS bindings; models download on first use from download.moonshine.ai. Gaps: **prebuilt Windows library is x86_64-only** (Windows ARM64 = build from source, untested); no punctuation-rich meeting transcript tuning evidence; sherpa-onnx's Moonshine support is the older non-streaming models only.
- **whisper.cpp**: builds on Windows ARM64 (MSVC + ARM NEON; [benchmarks exist on Snapdragon X Elite X1E78100](https://openbenchmarking.org/test/pts/whisper-cpp)), but Whisper is architecturally offline: fixed 30 s window, zero-padding, no caching. The bundled `stream` example is self-described as "a naive example" — sliding-window re-decode every ~0.5 s with the well-known boundary/hallucination problems. Real-time is achievable for small models on X Elite, but accuracy/latency for live captions is strictly worse than the purpose-built streaming models above. Not recommended for live; fine as a user-installed offline option.
- **Vosk**: [official install docs](https://alphacephei.com/vosk/install) explicitly state **"We do not support: … Windows ARM64"**. Out.
- **SenseVoice (FunAudioLLM)**: sherpa-onnx supports it (Windows x64 and arm64 builds work), non-autoregressive and very fast (5–15× faster than Whisper-Small/Large), but it's an **offline** model — "streaming" means chunked/pseudo-streaming with truncated attention and an accuracy hit. Released checkpoint is zh/yue/en/ja/ko; English WER not class-leading. Licensing is more tangled than the others: code MIT, weights under the FunASR Model Open Source License Agreement v1.1 (commercial use permitted per maintainer clarification, but with attribution/model-name clauses — check the card). Also runs via llama.cpp GGUF (~254 MB q8). Not the MVP pick.
- **sherpa-onnx streaming Zipformer** (icefall-trained): the pre-2026 sherpa streaming workhorse, fully supported on win-arm64, but noticeably worse WER than Nemotron. Viable ultra-light fallback.

## Runtimes on Snapdragon X Elite

- **ONNX Runtime CPU (ARM64)**: official `win-arm64` builds are mature; this is the path everything above uses. The X Elite's Oryon CPU cores are far stronger than the ARM boards these models are routinely demoed on (Raspberry Pi 5, RK3588), so CPU inference has large headroom.
- **ONNX Runtime QNN EP (Hexagon NPU)**: real and improving — plugin EP, `onnxruntime-qnn` win_arm64 wheel (note: [only cp312 gets the win_arm64 wheel](https://github.com/dhaneswara/ai-image-scaler)), HTP backend ships with the V81 skel matching Snapdragon X. BUT: quantization tooling is x64-only, NPU brings minute-scale first-run graph compile + cache-management pain, and **nobody has demonstrated these cache-aware transducer graphs on HTP** — sherpa-onnx runs the CPU provider. Treat NPU offload as a post-MVP experiment, not a dependency. (Context: even Microsoft's own Foundry Local had NPU crashes on this exact chip class — [Foundry-Local#719](https://github.com/microsoft/Foundry-Local/issues/719).)
- **sherpa-onnx Windows ARM64**: ships prebuilt `win-arm64` tarballs (e.g. [v1.12.40 win-arm64 shared lib](https://sourceforge.net/projects/sherpa-onnx.mirror/files/v1.12.40/sherpa-onnx-v1.12.40-win-arm64-shared-MD-Release-lib.tar.bz2/)), [NuGet `org.k2fsa.sherpa.onnx.runtime.win-arm64`](https://www.nuget.org/packages/org.k2fsa.sherpa.onnx.runtime.win-arm64/1.10.40), and (since [#3812](https://github.com/k2-fsa/sherpa-onnx/releases)) **Windows ARM64 Python wheels**. Docs include a dedicated ["Build sherpa-onnx on Windows (ARM64)"](https://k2-fsa.github.io/sherpa/onnx/) page. Static build supported too.
- **whisper.cpp / parakeet.cpp**: both ggml-based, both *can* compile for Windows ARM64, but only whisper.cpp is known-good there; parakeet.cpp ships no Windows binaries.

## Expected performance on the X Elite (CPU, INT8)

Direct measurements on Snapdragon X Elite for these exact models are not published — treat these as grounded extrapolations:

- sherpa-onnx parakeet-tdt-v2 int8, 2 threads, desktop-class core: RTF 0.03–0.12 ([sherpa-onnx NeMo transducer docs](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html)); on a single 2.3 GHz Cortex-A76 (RK3588) it's still real-time with headroom. Oryon cores are several× faster per thread → **RTF ≪ 0.1 expected even at 80 ms chunks for the 600M streaming models**, i.e. multiple simultaneous channels are fine.
- OpenWhispr ships Nemotron 600M INT8 streaming on CPU in production: "partial text lands well under a second behind your voice" on Apple Silicon and recent x86 laptops ([engineering write-up, 2026-07-18](https://openwhispr.com/blog/local-streaming-speech-to-text)). X Elite is in the same class.
- Moonshine's own table: Medium Streaming 74–269 ms latency per update on laptop-class CPUs; runs on a Pi 5 at ~800 ms.
- **RAM**: 600M INT8 ≈ 640 MB weights; with ONNX Runtime arenas and streaming caches expect roughly **1–1.5 GB resident per loaded engine**. Two channels (mic + loopback) can share one loaded model as two streams (sherpa-onnx online recognizers are multi-stream), keeping total ~1.5 GB — trivial against 32 GB.

## Model distribution

All candidates are on Hugging Face with on-demand download practicality:
- Nemotron streaming: ~632 MB (EN) / ~650 MB (multilingual) INT8 ONNX via sherpa-onnx release assets or HF; GGUF q8_0 also published on the NVIDIA repo for NeMo-Speech.cpp.
- Parakeet TDT v2/v3 INT8: ~640 MB sherpa-onnx tarballs.
- Moonshine: ~27 MB (tiny) to a few hundred MB (medium), downloaded on first use by the library itself.
- Download-once-and-cache is the established pattern (OpenWhispr, Earshot both do exactly this, and it matches Kiminola's "downloadable on-device models" destination line).

## Risks / open questions for the spec

- **sherpa-onnx version sensitivity**: Nemotron models need ≥ 1.13.4 or they mis-decode *silently*. Pin and bundle the runtime; add a known-audio smoke test at first run.
- **Nemotron online recognizer is greedy-search only** today — no hotword boosting for names/jargon. If that matters, it's an upstream contribution or a Zipformer fallback.
- **Streaming partials revise themselves** — UI must replace-in-place, not append (OpenWhispr learned this the hard way). Spec the transcript view accordingly.
- **WER gap vs batch**: streaming 6.93% vs 5.91–6.05% best batch. Optional pattern (OpenWhispr's): commit the stream, and if the flush fails, re-decode offline with Parakeet. Kiminola could offer Parakeet-TDT v3 as an optional "high-accuracy final pass" download later.
- **Windows ARM64 Moonshine** and **NPU offload** are both plausible but unverified — schedule a half-day spike only if we outgrow the CPU path (we won't for 2 channels).
- NVIDIA's cards target/test on Linux+NVIDIA GPU; the sherpa-onnx ONNX exports are the de-facto on-device path and are what everyone ships. Fine, but note the official card's "Preferred OS: Linux" when citing support expectations.

## Sources

- [nvidia/nemotron-speech-streaming-en-0.6b model card](https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b) (fetched 2026-08-12) — architecture, chunk sizes, WER tables, NVIDIA Open Model License
- [nvidia/nemotron-3.5-asr-streaming-0.6b](https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b) — multilingual streaming, OpenMDW-1.1 (via [vllm#47455](https://github.com/vllm-project/vllm/issues/47455) and [earshot MODELS.md](https://github.com/eknuth/earshot/blob/main/MODELS.md))
- [nvidia/parakeet-tdt-0.6b-v2 model card](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2) — CC-BY-4.0, 6.05% avg WER, 24-min full-attention
- [sherpa-onnx NeMo model docs](https://k2-fsa.github.io/sherpa/onnx/nemo/index.html) and [NeMo transducer page](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html) — model packages, sizes, RTF figures
- [sherpa-onnx releases](https://github.com/k2-fsa/sherpa-onnx/releases) + [win-arm64 prebuilt (SourceForge mirror)](https://sourceforge.net/projects/sherpa-onnx.mirror/files/v1.12.40/sherpa-onnx-v1.12.40-win-arm64-shared-MD-Release-lib.tar.bz2/) + [NuGet win-arm64](https://www.nuget.org/packages/org.k2fsa.sherpa.onnx.runtime.win-arm64/1.10.40) — Windows ARM64 binaries
- [sherpa-onnx#3572](https://github.com/k2-fsa/sherpa-onnx/issues/3572) — Nemotron online recognizer = greedy search only
- [OpenWhispr: Streaming Speech-to-Text on Your CPU](https://openwhispr.com/blog/local-streaming-speech-to-text) (2026-07-18) — production proof, model sizes, latency feel, version-pinning bug
- [moonshine-ai/moonshine](https://github.com/moonshine-ai/moonshine) (fetched 2026-08-12) — streaming model table (WER/params/latency), MIT, platform matrix
- [whisper.cpp stream example README](https://github.com/ggerganov/whisper.cpp/blob/master/examples/stream/README.md) — "naive example"; [OpenBenchmarking whisper.cpp on X1E78100](https://openbenchmarking.org/test/pts/whisper-cpp)
- [Vosk install docs](https://alphacephei.com/vosk/install) — Windows ARM64 explicitly unsupported
- [SenseVoice repo](https://github.com/FunAudioLLM/SenseVoice) (fetched 2026-08-12) — non-autoregressive offline, FunASR model license clarification, GGUF option
- [ONNX Runtime QNN EP repo](https://github.com/onnxruntime/onnxruntime-qnn), [QNN EP tutorial for Snapdragon](https://github.com/DakeQQ/Tutorial-ONNX-Runtime-Execution-Providers/blob/main/Qualcomm/README.md), [Foundry-Local#719](https://github.com/microsoft/Foundry-Local/issues/719) — NPU path status on X Elite
- [NVIDIA Open Model Agreement (2026-04-02 PDF)](https://www.nvidia.com/content/dam/en-zz/Solutions/license-agreements/enterprise-services/nvidia-open-model-agreement-2026-04-02.pdf), [OpenMDW about](https://openmdw.ai/about/), [OpenMDW-1.0 SPDX](https://spdx.org/licenses/OpenMDW-1.0.html) — licensing
- [mudler/parakeet.cpp](https://github.com/mudler/parakeet.cpp) — GGUF runtime, no Windows prebuilts
- [arXiv 2604.14493 — Pushing the Limits of On-Device Streaming ASR](https://arxiv.org/abs/2604.14493) — independent 50-config benchmark identifying Nemotron Speech Streaming as the strongest on-device CPU streaming candidate
