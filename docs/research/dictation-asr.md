# Local speech engines for English dictation

- Researched on 2026-09-23 UTC for [Compare local speech engines for English dictation](https://github.com/Trillx/Kiminola/issues/44), under [Plan system-wide dictation for Kimi Nola](https://github.com/Trillx/Kiminola/issues/43).
- Kimi Nola baseline: `e4a448d99da8d372088e220e9c7465c5341902b5`.
- Status: research findings only. No engine selection, product-spec change, issue update, model download, inference run, microphone capture, or provider call.
- Evidence labels: **Fact** means source, metadata, or repository inspection. **Upstream claim** means the publisher's capability or performance statement, not a Kimi Nola measurement. **Inference** means a consequence that needs testing. **Recommendation** proposes an experiment, not a product decision. **Unverified** means this investigation did not establish it.

## Findings

The bounded first benchmark should compare three model/runtime combinations: the installed Nemotron English INT8 pack through sherpa-onnx, Whisper English through whisper.cpp, and Parakeet-TDT 0.6B v2 INT8 through sherpa-onnx. Whisper gets two sizes to test the resource tradeoff. This is a recommendation for experimental coverage, not a ranking or a default-engine decision.

1. Nemotron already provides genuine cache-aware streaming, punctuation, and capitalization. Its installed 160 ms chunk size is not a measured first-word latency or release-to-final latency. Reusing its weights and native runtime avoids another ASR download for existing users. The stop/endpoint contract still needs dictation-specific verification. [N1], [N2], [N6], [K1]
2. whisper.cpp has documented Windows x64 builds and current native Windows ARM64 build/release evidence. ARM64 is not limited to emulation. The examined `v1.9.4` release has no attached binaries; prerelease `b5130` has native ARM64 CPU and Adreno OpenCL packages. Source-build support, downloadable packages, and successful execution in Kimi Nola are different evidence levels. [W1], [W2], [W3]
3. Whisper uses sequence-to-sequence windowed decoding. Its microphone example repeatedly transcribes a rolling buffer; it is not equivalent to Nemotron's cached streaming encoder. Push-to-talk final-only transcription remains a valid use of an offline engine if its stop delay is acceptable. [W4], [W5]
4. Parakeet v2 adds an English, punctuation-capable offline comparator without adding another inference runtime. Its model card explicitly warns about word-for-word and incomplete-sentence use. That warning is particularly relevant to short dictation. Its public leaderboard results do not establish superiority here. [P1], [P2]
5. Two baseline discrepancies deserve a focused follow-up: the model manifest's declared total exceeds the sum of its files, and Kimi Nola's current use of `result.is_final` does not match sherpa's explicit endpoint/reset example. Neither was changed in this research. Details follow below. [K1], [K2], [N3], [N4]

## Scope and existing decisions

The system-wide dictation planning map is authoritative for the new scope: Windows x64 and native ARM64, English first, hold-to-talk and toggle, and delivery into email, team-chat, coding-app, and browser fields. Audio stays local. Basic transcription must work without a provider key. Optional cleanup may send only text to an explicitly selected provider, including OpenRouter. Plain transcription is an acceptable fallback, but consent and fallback behavior remain undecided.

Cleanup means removing filler, applying spoken corrections, and adding punctuation, paragraphs, or lists without summarizing or inventing content. ASR punctuation does not establish support for that entire cleanup contract. A code editor as a destination also does not yet define a voice-to-code grammar.

The baseline [SPEC.md](../../SPEC.md) defines an in-process meeting recognizer, a single default Model pack, and dual microphone/system lanes. Historical decisions [Local streaming ASR on Windows ARM64](../../.scratch/kiminola/issues/01-local-streaming-asr-on-windows-arm64.md), [Lock the stack and inference architecture](../../.scratch/kiminola/issues/05-lock-stack-and-inference-architecture.md), and [Model distribution and management UX](../../.scratch/kiminola/issues/07-model-distribution-and-management-ux.md) remain meeting decisions, not permission to choose a new dictation engine. Managed local-LLM enhancement is excluded from the meeting MVP, and Background companion currently never captures audio. This note does not resolve those scope boundaries.

## Comparison

All three candidates can perform base transcription locally without an API key. The table distinguishes the model from its runtime. NVIDIA's NeMo/Linux/GPU deployment guidance is not evidence of Windows support; sherpa's Windows builds supply that evidence for the ONNX exports.

| Candidate | Windows x64 and native ARM64 evidence | Dictation, punctuation, and finalization | Download and memory | Integration and main uncertainty |
|---|---|---|---|---|
| Existing Nemotron English 0.6B, 160 ms INT8 export, sherpa-onnx CPU | Fact: sherpa `v1.13.5` has x64 and ARM64 shared-runtime assets. Kimi Nola already uses its C API/Rust binding and architecture-specific native packaging. Historical ARM64 sample execution exists, but is not dictation evidence. [S1], [K1], [K4] | Upstream claim: native punctuation/case and cache-aware RNNT streaming. Fact: this specific export fixes the chunk size at 160 ms. Native decoding accepts incremental samples and has separate endpoint, reset, and input-finished APIs. Do not confuse an endpoint with the user's stop action. [N1], [N2], [N6], [N3] | Fact: four required files sum to **661,919,416 bytes, 631.26 MiB**. Declared manifest total is different. No additional model bytes for a user with the verified installed pack. Historical sample peak working set was 865.3 MiB; current dictation memory is unmeasured. [K2], [K4] | Existing shared weights are reusable in principle, with a fresh single-microphone stream. Meeting orchestration is not a dictation session. Greedy-only Nemotron decoding in the examined runtime means generic sherpa hotword settings must not be advertised as working for this model. [K1], [N5] |
| Whisper `base.en` or `small.en`, whisper.cpp CPU | Fact: x64 MSVC support; ARM64 LLVM/MSVC-target toolchain and CPU release job. Prerelease `b5130` has x64 and ARM64 packages. No native execution or Rust-binding build was performed here. [W1], [W2], [W3] | Fact: rolling-window or VAD-segmented offline inference, not native cached audio streaming. Transcription output can include punctuation/case. English language and transcription task can be explicit. OpenAI warns of hallucination and repetition. Short-fragment and silent-input behavior need testing. [W4], [W5], [W6] | Fact: hosted Q5_1 `base.en` is **59,721,011 bytes, 56.95 MiB**; Q5_1 `small.en` is **190,098,681 bytes, 181.29 MiB**. Upstream unquantized memory table says about 388 MB for base and 852 MB for small. Those are not measured Q5_1 Windows footprints. [W1], [W7] | Requires a separate native library/FFI adapter and GGML Model pack. Explicit stop must finish the retained audio buffer, await decode, and publish one authoritative result. Rolling hypotheses must not be appended as independent final text. Quantization quality, warm-up, stop delay, and native packaging remain unverified. |
| Parakeet-TDT 0.6B v2 English INT8, sherpa-onnx CPU | Fact: same sherpa native Windows runtime family. Official sherpa docs provide an offline NeMo-transducer configuration and Windows executable path. The exact candidate was not run on either target here. [S1], [P2] | Upstream claim: native punctuation/case and timestamps. Full-attention offline model; VAD-based simulated streaming is documented. Final-only stop decoding avoids treating VAD silence as permission to insert text. Model-card warning on incomplete sentences makes short-fragment evaluation mandatory. [P1], [P2] | Fact: four required export files total **661,190,513 bytes, 630.56 MiB**. Its actual ONNX Windows resident and peak memory are unmeasured. The model card's at-least-2-GB loading guidance concerns its documented deployment, not a verified INT8 sherpa footprint. [P1], [P3] | Reuses sherpa deployment and the download primitives, but uses `OfflineRecognizer`, not `OnlineRecognizer`. Different weights cannot be shared with the resident meeting Nemotron pack. Additional download and simultaneous-model RAM need justification. |

### Resource evidence and exact model identities

Sizes below are file bytes, not archive sizes, installer growth, or RAM. MiB conversions and sums were computed, not estimated. Hugging Face API metadata was read without downloading model weights.

| Model pack/configuration | Source and revision | Required bytes |
|---|---|---:|
| Installed Nemotron 160 ms INT8 | `csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25`, `237e551abd7a411ef92d3595454d9f6ab5fe7d6c` [N2], [K2] | 661,919,416 |
| Whisper `ggml-base.en-q5_1.bin` | `ggerganov/whisper.cpp`, `5359861c739e955e79d9a303bcbc70fb988958b1` [W7] | 59,721,011 |
| Whisper `ggml-small.en-q5_1.bin` | Same Whisper repository and revision [W7] | 190,098,681 |
| Parakeet v2 `encoder.int8.onnx`, `decoder.int8.onnx`, `joiner.int8.onnx`, `tokens.txt` | `csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8`, `1ab9323565ddb038682214b292f588070a538ce2` [P3] | 661,190,513 |

The Whisper repository also contains unquantized `base.en` at 147,964,211 bytes and `small.en` at 487,614,201 bytes. Those are useful controls only if a quantized candidate shows a suspicious quality regression. Q5_1 is selected for the proposed first experiment because exact hosted artifacts exist; the README's Q5_0 conversion example is not the same artifact. Quantization reduces file size but does not establish proportional RAM or latency savings. [W1], [W7]

**Fact:** `manifest.json` declares `total_bytes: 663919416`, while its four `files[].bytes` add to `661919416`. The difference is **2,000,000 bytes**. Remote metadata for the pinned revision agrees with the individual file sizes. This note preserves both values rather than silently correcting the source. The historical spike's file total is also not an exact identity for the current manifest. [K2], [N2], [K4]

**Unverified:** cold load, first usable partial, release-to-final latency, quality, power, and memory for every dictation configuration. No public WER or throughput figure in this note is a local dictation result.

## Streaming and short-utterance risks

### Nemotron

**Fact:** NVIDIA describes runtime chunk choices of 80, 160, 560, and 1120 ms. The installed converter's README says its exported ONNX chunk is 160 ms. A different chunk-size experiment should use a matching export; changing capture callback duration does not establish a different model context size. The exact pack's token file includes punctuation and uppercase tokens, supporting the card's punctuation/case claim without measuring output quality. [N1], [N2], [N6]

**Fact:** Kimi Nola's `AsrEngine` holds one recognizer behind `Arc`; each lane has its own stream. It enables endpointing, uses `greedy_search` and four threads, and drains all ready chunks. `finish()` calls `input_finished()`, decodes while ready, and marks the returned text final itself. The four-thread comment comes from dual-lane meeting measurements, not short single-lane dictation. [K1]

**Fact:** sherpa's Nemotron example calls `IsEndpoint` and then `Reset` explicitly. At end of input it appends 0.3 seconds of zero padding before `InputFinished` and drains decoding. That example uses a 560 ms export, so its padding is an experiment lead, not proof of a universal 300 ms requirement for Kimi Nola's 160 ms pack. The C API's endpoint defaults are 2.4 seconds of silence, 1.2 seconds after speech, or 20 seconds of utterance length. These are rules, not measured application delays. [N3], [N7]

**Inference requiring a replay test:** enabling endpointing alone should not be assumed to generate finalized segments. Kimi Nola's `take_result()` reads `result.is_final` but does not call `is_endpoint()` or `reset()`. sherpa's result definition says `is_final` is used by its websocket server; the examined Rust accessor reads result JSON without adding endpoint/reset behavior. The Nemotron implementation does not set that flag in `GetResult`. A test must establish actual segment and finalization behavior before copying the adapter into dictation. This is source-level evidence, not a reproduced user-facing defect. [K1], [N4], [N5], [N8]

**Recommendation:** first replay very short clips, clips ending immediately after the last phoneme, long intra-sentence pauses, and repeated start/stop episodes. Compare current finish behavior with the upstream endpoint/drain pattern and a declared tail-padding variant. Do not attribute clipped endings or missing finals to model quality before resolving the adapter contract.

### Whisper

**Fact:** the official stream example transcribes a rolling buffer every half second and describes itself as naive. It also offers VAD-triggered window transcription. This can give partial previews, but repeats some work and requires replacement/deduplication logic. The model's 30-second processing window does not require making the user wait 30 seconds before decoding a short utterance. [W4], [W8]

**Fact:** the C API exposes explicit language, translation, context, initial-prompt, no-speech, and abort options. Use `language = "en"` and `translate = false` in a proposed English baseline. Reset utterance context between independent dictations. Prompts can influence transcription but are not an exact vocabulary guarantee. The examined implementation returns without transcription for input shorter than 100 ms; VAD also has a minimum speech-duration filter. These must not silently discard short intended utterances. [W6], [W9]

**Upstream warning:** OpenAI documents output that was not spoken and repetitive text. Therefore silence, breath, keyboard noise, and clipped or ambiguous speech belong in the first acceptance fixture, not a later polish test. Punctuation/case generation does not guarantee faithful spelling of names, email addresses, paths, identifiers, or numbers. [W5]

**Recommendation:** benchmark final-only stop decoding first. Add rolling previews only if the interaction decision requires them; otherwise a streaming wrapper would add work without answering the engine question. Hold release and toggle-stop should both stop accepting new speech and finish the buffered input without waiting for an arbitrary VAD timeout.

### Parakeet v2

**Fact:** this is the English full-attention TDT model, not Parakeet Realtime EOU, a streaming Parakeet variant, or multilingual v3. sherpa documents offline decode and VAD-based simulated streaming for this exact export. It supports INT8, FP16, and less-quantized export alternatives, but the first comparison only needs INT8. [P1], [P2]

**Inference:** it is a useful test of whether utterance-complete decoding improves insert-ready text enough to justify another large pack. Neither NVIDIA's GPU/batched RTFx nor sherpa's example timings on other systems settle that question. Its incomplete-sentence warning could matter more than its aggregate leaderboard score for two-word chat replies. [P1], [P2]

## Native packaging and acceleration

**Fact:** sherpa `v1.13.5` publishes `win-x64-shared-MD-Release` and `win-arm64-shared-MD-Release` packages. Kimi Nola uses sherpa shared linking and separately stages `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `sherpa-onnx-c-api.dll`, and `sherpa-onnx-cxx-api.dll`. The exact executable and DLL PE architectures must agree. This is the existing CPU deployment path, not a CUDA requirement. [S1], [K5]

**Fact:** whisper.cpp `v1.9.4` contains a Windows ARM64 CPU job using its LLVM toolchain, an MSVC target, and `WHISPER_SDL2=OFF`. That toolchain specifies `-march=armv8.7-a`. Its release workflow also includes Adreno OpenCL. `b5130` assets confirm that native CPU and Adreno packages were published, but this investigation did not inspect their PE headers or execute them. The ARM64 build recipe alone is not proof of support for every older Windows ARM device. [W2], [W3], [W10]

**Recommendation:** use whisper's C API with Kimi Nola's capture path for the integration experiment, not a second SDL microphone owner. A Rust crate's existence is insufficient; test its exact dependency version and linking on both targets. Audit all DLLs and notices before redistribution. In particular, the examined ARM64 release workflow copies an OpenMP DLL from an MSVC `debug_nonredist` directory. Do not infer redistribution rights from a file appearing in an upstream archive. [W2], [W6]

Optional acceleration remains separate from the CPU comparison:

- whisper.cpp documents CUDA for NVIDIA GPUs, Vulkan, and OpenVINO encoder offload. Each changes drivers, binaries, and sometimes model artifacts. Current ARM64 release evidence also covers Adreno OpenCL, which is GPU acceleration, not Hexagon NPU support. No speedup on either target was measured here. [W1], [W2]
- ONNX Runtime documents Windows ARM64 QNN support on Snapdragon. It also specifies graph/operator/shape and quantization constraints. sherpa has Nemotron QNN export workflows, but the examined workflow builds Android/Linux artifacts, not evidence that Kimi Nola's current CPU INT8 pack runs on a Windows NPU. Neither "ONNX" nor "INT8" makes a pack QNN-ready. [S2], [S3]
- Recommendation: attempt one hardware-specific backend only if the CPU finalist misses an agreed latency or power budget. Record fallback behavior and confirm actual backend use. Do not multiply the initial model-by-device matrix with every advertised accelerator.

## Licenses and model-manager reuse

| Component | Documented terms | Distribution consequence |
|---|---|---|
| sherpa-onnx runtime | Apache-2.0. Its license does not replace a model's license. [L1] | Preserve required license/notices and audit bundled dependencies independently. |
| Installed English Nemotron model | NVIDIA Open Model License, as linked by the originating NVIDIA card. The converted repository names that upstream model. [N1], [N2], [N6], [L2] | Commercial use and derivative distribution are permitted subject to the agreement. Redistribution requires the agreement and a Notice file containing `Licensed by NVIDIA Corporation under the NVIDIA Open Model License`. This is not the newer multilingual model's different license. The agreement also contains use, guardrail, and compliance terms; do not describe it as unconditional MIT-style licensing. |
| Whisper model weights and whisper.cpp runtime | OpenAI explicitly licenses code and weights under MIT; whisper.cpp has its own MIT notice. [W8], [L3] | Preserve the relevant copyright and permission notices for both. Conversion/quantization does not erase the original notice requirements. |
| Parakeet v2 model | CC-BY-4.0 in NVIDIA's card and converted-model metadata. [P1], [P3], [L4] | Attribute the source, link the license, and identify conversion/quantization changes. Preserve supplied notices and do not imply NVIDIA endorsement. Runtime terms remain separate. |

This is a summary of primary license text, not legal clearance. The installed Nemotron manifest contains only weights and tokens. It does not itself deliver a license/Notice file. Whether existing app-level attribution satisfies all obligations was not audited; resolve that explicitly before distributing another pack. [K2]

**Fact:** `models.rs` has reusable pinned Hugging Face URLs, HTTP Range resume, expected lengths, SHA-256 verification, progress events, and `.part`-to-final publication. Its manifest schema is file-based rather than ONNX-specific. But it embeds one global manifest, builds URLs through that singleton, and hardcodes the `models/nemotron` destination. `asr.rs` separately recognizes specific ONNX filenames. Adding a second engine is therefore not just changing a JSON file. [K1], [K3]

**Recommendation:** any future multi-pack design should identify pack, revision, runtime, architecture-independent model files, expected hashes, and license notices explicitly. Keep candidate packs in distinct revision-aware directories and do not overwrite the meeting pack. A single Whisper GGML file and Parakeet's four ONNX/token files can use the downloader primitives after those assumptions are separated. Pin and verify SHA-256, not the shorter generic "SHA" strings printed in whisper's model README. No new manifest was created here. [K3], [W7]

**Inference:** retaining Nemotron for Meetings while loading Whisper or Parakeet for dictation adds a second set of model weights. Shared use of sherpa does not make two different models share weights. Sharing one Nemotron engine between Meetings and dictation still needs explicit scheduling, cancellation, ownership, and peak-memory tests. Current dual-lane batching is meeting-specific, and does not establish safe arbitrary concurrent recognition. [K1], [K3]

## Bounded shortlist and exclusions

Recommended first-pass configurations, with no preference implied:

1. Current pinned Nemotron 160 ms INT8, sherpa `v1.13.5`, CPU, greedy decode, one mic lane. Start with the existing four-thread setting, but record it as a baseline rather than a dictation optimum.
2. Whisper `base.en` Q5_1, whisper.cpp `v1.9.4` source build, CPU, English transcription, final-only decode. This covers a smaller download and a distinct model family.
3. Whisper `small.en` Q5_1 with the same runtime and decoding policy. This tests whether the smaller model leaves avoidable accuracy on the table without starting with a large multilingual model.
4. Parakeet-TDT 0.6B v2 INT8, sherpa `v1.13.5`, CPU, greedy offline decode on explicit stop. This isolates an utterance-complete, punctuated English model within the existing runtime family.

Record all decoding defaults and thread settings. Reuse the same resampled audio across configurations. If Q5_1 materially changes a finalist's behavior, compare only that finalist with its unquantized counterpart before rejecting the family.

Only Parakeet is added to the requested Nemotron/Whisper comparison. This keeps the experiment small while covering native streaming, small windowed sequence-to-sequence models, and a same-runtime offline transducer.

Moonshine Voice is a reserve, not a first-pass candidate. Its current upstream README/license say English and streaming models are MIT and run locally. However, release `v0.1.5` exposes a Windows x86_64 library, not a Windows ARM64 library. That establishes a prebuilt-distribution gap, not impossibility of a native source port. Adding another integration and ARM64 qualification path is not necessary to answer the first experiment. Revisit it if the initial candidates miss the resource budget. [M1], [M2]

Multilingual Nemotron/Parakeet, large/turbo Whisper, other runtimes for the same models, and accelerator-specific packs are deferred. That is a limit on experiment scope, not a claim that they are inferior. English-first scope and the existing CPU/native deployment are enough to justify this initial boundary. [N1], [P1], [W1]

## Minimum future experiments

These are proposed tests, not executed work. They need separate authorization for builds, model acquisition, and any audio collection.

### 1. Finalization and native-load gate

Before quality scoring, run a small deterministic public or consented fixture through the installed Nemotron adapter and each candidate's native library on one x64 PC and one Snapdragon ARM64 PC. Verify PE architecture for the process and every loaded native DLL, model revision/hash, successful load without a provider key, and decode while offline. A cross-compile or an emulated x64 process does not pass ARM64 runtime qualification.

Test a one-word utterance, silence, an utterance ending with no trailing silence, a pause inside a sentence, and two consecutive start/stop episodes. Assert exactly one authoritative final snapshot per episode, no previous-episode text, no dropped last word, and bounded cancellation. Replay the same boundary fixture through the actual resampler/drain path. Do not modify reference audio to favor one engine without reporting the transformation.

This gate specifically resolves Nemotron's endpoint/`is_final`/reset behavior and tail-padding question. It also catches VAD thresholds that discard short speech. Do not wait for the full quality corpus to discover that a wrapper never finalizes correctly.

### 2. Small paired dictation screen

Use one fixed set of **60 speech clips plus 10 non-speech controls**. This is a proposed screening size, not a statistically conclusive release corpus. Include at least three consenting speakers or a licensed existing fixture with several speakers. No private email/chat content is needed. Use invented task text, but genuine recorded speech with declared provenance, not invented recognition results.

Stratify the speech clips across:

- One- or two-word replies and incomplete fragments.
- Ordinary short email and team-chat sentences.
- Proper names, email addresses, numbers, dates, paths, and coding identifiers.
- Fillers, repetitions, and spoken corrections.
- Multi-sentence passages, list/paragraph requests, and long pauses within a continuing thought.
- Representative accent, microphone, quiet-room, and moderate-noise variation.

Include both rapid hold-release and toggle-stop boundaries, plus a few longer passages crossing internal segmentation boundaries. Silence, breathing, keyboard sounds, and background playback belong among the non-speech controls. Keep intended text and literal spoken reference separate where a cleanup or spoken-formatting rule has not been decided.

Run the four configurations on both devices, CPU first, with identical audio and pinned decoder settings. Measure:

| Question | Minimum recorded evidence |
|---|---|
| Does it preserve the words? | Normalized WER plus exact-match rates for names, numbers, addresses, and identifiers. Manually classify insertions, deletions, repetitions, and changed meaning. Report errors by short-fragment/task category, not only one average. |
| Is plain output usable? | Punctuation/capitalization errors and blinded human correction count/time. Score raw ASR separately from cleanup. Do not reward silently deleting spoken corrections before their intended semantics are agreed. |
| Does it invent text on non-speech? | False-output count and actual output for each non-speech control. No-speech safety is not proven by average WER. |
| How long does the user wait? | Cold model load, warm first usable partial where offered, and explicit-stop-to-authoritative-final latency at p50/p95. State the timestamp boundaries. Final-only candidates have no partial-latency result. Measure ASR finalization separately from text cleanup and insertion. |
| What does it cost locally? | Installed model bytes, resident idle working set/private bytes, active peak memory, CPU time, wall time, and RTF. Repeat a finalist under the agreed laptop battery/power mode and ordinary foreground workload. |

Collect at least three timing repetitions after warm-up and several fresh-process loads. This screens for practical differences; it does not turn a small number of speakers into broad accuracy certification. Expand only the categories where finalists differ or fail.

### 3. Finalist lifecycle and power check

After the paired screen, test only the finalists for rapid repeated dictations, long toggle sessions, cancel during decoding, device loss, and return after idle unloading if unloading is proposed. If dictation can overlap a Meeting, add that precise contention test; otherwise test that exclusion explicitly. Confirm no audio persistence, no network dependency for base transcription, and no cross-episode context leakage.

Power comparisons require the same device, workload duration, battery/AC state, power mode, and measurement method. Do not infer battery savings from RTF alone. Extra acceleration experiments and broader app insertion tests belong after this gate, not inside the first engine screen.

The human engine decision should then compare paired quality and correction effort against agreed stop-latency, memory, download, and power budgets. None of those budgets has been supplied yet. This note sets no implicit pass threshold or automatic winner.

## Follow-up questions

1. Must users see live partial text, or is a recording indicator followed by final text acceptable? This determines whether an offline candidate needs a streaming wrapper at all.
2. What are the maximum acceptable warm stop-to-final p95 and cold first-use delay? Is keeping a model resident while the app is idle acceptable, and what idle-memory budget applies?
3. Which x64 CPU class and minimum RAM must pass? Is ARM64 support specifically Snapdragon X-class, or must the ARM64 packaging work on earlier devices too?
4. Does "coding apps" mean prose in an editor, exact identifiers/paths, or spoken syntax? Should "comma", "new paragraph", and "scratch that" be literal words or formatting/correction instructions, and how are literal uses escaped?
5. May dictation run during a Meeting? If so, which session owns the mic, how is duplicate speech handled, and can one recognizer service both without unacceptable delay?
6. Is another model download acceptable to existing users, and would a better offline final justify keeping both the meeting pack and a dictation pack? No picker or managed multi-model policy is currently adopted.
7. Will the next experiment separately reproduce the Nemotron endpoint/reset and tail-drain behavior before comparing quality? Who owns the manifest-total discrepancy and model-license notice audit?
8. For the cleanup decision, when may text leave the machine, and what should happen on provider failure, timeout, or suspected meaning change? Plain local output is an accepted fallback, but automatic insertion of that fallback is not yet authorized by a settled interaction rule.

## Evidence limits

The historical [Snapdragon spike](../../.scratch/kiminola/issues/11-spike-wasapi-loopback-nemotron-on-snapdragon.md) ran model-supplied sample WAVs, not this dictation corpus. Its 0.14 weighted RTF, 865.3 MiB peak working set, and 2.6% normalized WER cannot be labeled dictation latency, general dictation accuracy, punctuation quality, or release-to-insertion performance. Four-thread dual-lane source comments have the same limitation. [K1], [K4]

Primary docs, source, release metadata, and Model pack metadata were inspected. No claim here establishes a measured candidate winner, safe cross-app insertion, successful native candidate execution, acceptable cleanup fidelity, or legal clearance. The parent must verify this asset before recording the speech-engine research resolution; engine selection remains a human decision.

## Primary sources

Repository links pin the baseline. Upstream runtime code is versioned where practical. Model cards and documentation can change; they were retrieved for this investigation. Model-size API links pin the recorded revisions.

[K1]: https://github.com/Trillx/Kiminola/blob/e4a448d99da8d372088e220e9c7465c5341902b5/kiminola/src-tauri/src/asr.rs
[K2]: https://github.com/Trillx/Kiminola/blob/e4a448d99da8d372088e220e9c7465c5341902b5/kiminola/src-tauri/models/manifest.json
[K3]: https://github.com/Trillx/Kiminola/blob/e4a448d99da8d372088e220e9c7465c5341902b5/kiminola/src-tauri/src/models.rs
[K4]: https://github.com/Trillx/Kiminola/blob/e4a448d99da8d372088e220e9c7465c5341902b5/.scratch/kiminola/issues/11-spike-wasapi-loopback-nemotron-on-snapdragon.md
[K5]: https://github.com/Trillx/Kiminola/blob/e4a448d99da8d372088e220e9c7465c5341902b5/kiminola/scripts/prepare-native-deps.ps1
[N1]: https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b
[N2]: https://huggingface.co/api/models/csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25/revision/237e551abd7a411ef92d3595454d9f6ab5fe7d6c?blobs=true
[N3]: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/c-api-examples/streaming-nemotron-c-api.c
[N4]: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/sherpa-onnx/csrc/online-recognizer.h
[N5]: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/sherpa-onnx/csrc/online-recognizer-transducer-nemo-impl.h
[N6]: https://huggingface.co/csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25/blob/237e551abd7a411ef92d3595454d9f6ab5fe7d6c/README.md
[N7]: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/sherpa-onnx/c-api/c-api.cc
[N8]: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/sherpa-onnx/rust/sherpa-onnx/src/online_asr.rs
[S1]: https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.5
[S2]: https://onnxruntime.ai/docs/execution-providers/QNN-ExecutionProvider.html
[S3]: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/.github/workflows/export-nemotron-speech-streaming-en-0.6b-qnn.yaml
[W1]: https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/README.md
[W2]: https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/.github/workflows/release.yml
[W3]: https://github.com/ggml-org/whisper.cpp/releases/tag/b5130
[W4]: https://github.com/ggml-org/whisper.cpp/blob/master/examples/stream/README.md
[W5]: https://github.com/openai/whisper/blob/main/model-card.md
[W6]: https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/include/whisper.h
[W7]: https://huggingface.co/api/models/ggerganov/whisper.cpp/revision/5359861c739e955e79d9a303bcbc70fb988958b1?blobs=true
[W8]: https://github.com/openai/whisper/blob/main/README.md
[W9]: https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/src/whisper.cpp
[W10]: https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/cmake/arm64-windows-llvm.cmake
[P1]: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2
[P2]: https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html
[P3]: https://huggingface.co/api/models/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/revision/1ab9323565ddb038682214b292f588070a538ce2?blobs=true
[L1]: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/LICENSE
[L2]: https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license/
[L3]: https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/LICENSE
[L4]: https://creativecommons.org/licenses/by/4.0/legalcode.en
[M1]: https://github.com/moonshine-ai/moonshine/blob/main/LICENSE
[M2]: https://github.com/moonshine-ai/moonshine/releases/tag/v0.1.5

Additional source details used to distinguish API behavior and artifact formats:

- [Pinned Nemotron export README](https://huggingface.co/csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25/blob/237e551abd7a411ef92d3595454d9f6ab5fe7d6c/README.md) and [token vocabulary](https://huggingface.co/csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25/blob/237e551abd7a411ef92d3595454d9f6ab5fe7d6c/tokens.txt).
- [sherpa Rust online accessor](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/sherpa-onnx/rust/sherpa-onnx/src/online_asr.rs), [C API endpoint defaults and JSON accessor](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/sherpa-onnx/c-api/c-api.cc), and [online result implementation](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/sherpa-onnx/csrc/online-recognizer.cc).
- [Kimi Nola meeting drain/stop path](https://github.com/Trillx/Kiminola/blob/e4a448d99da8d372088e220e9c7465c5341902b5/kiminola/src-tauri/src/recording_session.rs#L505-L736), [shared-linking configuration](https://github.com/Trillx/Kiminola/blob/e4a448d99da8d372088e220e9c7465c5341902b5/kiminola/src-tauri/Cargo.toml), and [portable DLL staging](https://github.com/Trillx/Kiminola/blob/e4a448d99da8d372088e220e9c7465c5341902b5/kiminola/scripts/stage-portable-package.ps1).
- [Whisper ARM64 toolchain](https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/cmake/arm64-windows-llvm.cmake), [v1.9.4 release metadata](https://api.github.com/repos/ggml-org/whisper.cpp/releases/tags/v1.9.4), and [short-input/decoding implementation](https://github.com/ggml-org/whisper.cpp/blob/v1.9.4/src/whisper.cpp).
- [OpenAI Whisper README, including code/weight MIT statement and windowing](https://github.com/openai/whisper/blob/main/README.md), [whisper.cpp model-format documentation](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md), and [Moonshine README](https://github.com/moonshine-ai/moonshine/blob/main/README.md).
