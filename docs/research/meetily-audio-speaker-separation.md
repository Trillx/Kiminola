# Meetily audio capture and speaker-separation research

- **Researched:** 2026-08-22
- **Meetily Community Edition snapshot:** [`0281737d87d26352fb0adc78c8c0975f691b23d1`](https://github.com/Zackriya-Solutions/meetily/commit/0281737d87d26352fb0adc78c8c0975f691b23d1) (`main`, 2026-06-05)
- **Unmerged source-attribution proposal:** [PR #661](https://github.com/Zackriya-Solutions/meetily/pull/661), head [`d509f67f9d14d4e8221bae529e2ff801a2652607`](https://github.com/Zackriya-Solutions/meetily/commit/d509f67f9d14d4e8221bae529e2ff801a2652607)
- **Scope:** source inspection only; no Meetily runtime or hardware audio test was performed.

## Bottom line

Kimi Nola already has the architectural part that current Meetily Community Edition is missing: Kimi Nola keeps microphone and system audio in separate ASR lanes and emits them as `You` and `Others`. Current Meetily captures both inputs but mixes them before VAD/transcription, then labels the mixed result generically. Its released path therefore is **not** an implementation to copy for speaker separation.

The best next move for Kimi Nola is:

1. Give every live utterance a stable identity, source, revision/finality state, and audio-relative timestamps. Keep simultaneous `You` and `Others` partials in separate keyed UI entries instead of one global partial slot.
2. Preserve the existing two independent ASR lanes all the way through persistence.
3. Add conservative, post-ASR echo-duplicate suppression across overlapping `You`/`Others` finals, preferring the system-audio copy when the texts are highly similar.
4. Add meeting-process loopback as an optional capture mode, with default-endpoint loopback as a fallback.
5. Treat acoustic echo cancellation (AEC) as an optional speakerphone enhancement, not the source-labeling mechanism. Headphones remain the clean baseline.

This solves two different problems without conflating them:

- **Who is this?** Deterministic source attribution: microphone = `You`; system loopback = `Others`.
- **Why did the remote person appear twice?** Speaker leakage/echo: the laptop speakers entered both system loopback and the microphone, so the two source transcripts need AEC and/or duplicate suppression.

True diarization is a third, later problem: distinguishing Alice, Bob, and Carol within `Others`. Meetily Community Edition does not provide it.

## What current Meetily Community Edition actually does

### Capture begins as two tagged streams

Meetily creates separate microphone and system streams, and its Windows device layer enumerates WASAPI render endpoints as loopback-capable outputs ([`recording_manager.rs`, lines 51–126](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/recording_manager.rs#L51-L126); [`windows.rs`, lines 7–33](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/devices/platform/windows.rs#L7-L33)). It opens mic and render devices through CPAL input streams and tags chunks `Microphone` or `System` ([`stream.rs`, lines 55–135](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/stream.rs#L55-L135) and [lines 374–415](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/stream.rs#L374-L415)). Meetily pins a CPAL revision whose WASAPI backend sets loopback when an input stream targets a render device ([Meetily `Cargo.toml`, lines 210–212](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/Cargo.toml#L210-L212); [CPAL source, lines 558–562](https://github.com/RustAudio/cpal/blob/51c3b43c54591203fe3edc9a31ff7595650f7103/src/host/wasapi/device.rs#L558-L562)).

Each source owns persistent resampling state. Microphone audio alone receives high-pass filtering, optional RNNoise, and loudness normalization; system audio remains unprocessed ([`pipeline.rs`, lines 202–346](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/pipeline.rs#L202-L346)). Those are useful implementation patterns.

### It then mixes away the source information

The active pipeline uses separate mic/system ring buffers, aligns them in windows, and zero-pads the missing side ([`pipeline.rs`, lines 16–140](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/pipeline.rs#L16-L140)). It then:

1. mixes mic and system audio;
2. runs one continuous VAD over the mix;
3. sends each speech chunk to ASR as `DeviceType::Microphone // Mixed audio`; and
4. sends the frontend a generic `source: "Audio"`.

See [`pipeline.rs`, lines 817–877](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/pipeline.rs#L817-L877) and [`worker.rs`, lines 171–220](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/transcription/worker.rs#L171-L220).

That makes current Meetily unsuitable as a model for `You` versus `Others`. It also makes overlapping speech harder for ASR because two speakers become one waveform.

Meetily does have good shutdown discipline: it drains until the channel closes, explicitly flushes VAD, and tracks queued/completed work ([`pipeline.rs`, lines 766–898](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/pipeline.rs#L766-L898); [`worker.rs`, lines 289–359](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/transcription/worker.rs#L289-L359)). Its hot-path channels are unbounded, however, so it trades queue-full loss for possible memory growth if transcription falls behind.

### “Diarization support” is not present in the active Community Edition path

The active crate exports `audio`, not the separate `audio_v2` directory ([`lib.rs`, line 40](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/lib.rs#L40)). The repository contains a migration adding a nullable `speaker` column ([migration, lines 1–5](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/migrations/20251110000001_add_speaker_field.sql#L1-L5)), but the active transcript model does not expose that field ([`models.rs`, lines 24–38](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/database/models.rs#L24-L38)). A schema artifact is not a working diarization pipeline.

The frontend README claims “speaker diarization support” ([`frontend/README.md`, lines 5–12](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/README.md#L5-L12)), while the root README places speaker diarization/identification in a separate PRO codebase and marks it planned/coming soon ([`README.md`, lines 215–231](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/README.md#L215-L231)). The source code is the stronger evidence: current Community Edition does not perform diarization, remote-speaker identification, or even retained mic/system attribution.

### Echo suppression is also absent

Current Meetily has no Windows acoustic echo canceller and no cross-source, time-and-text duplicate suppression. Its `clean_repetitive_text` routine only removes repetition inside one ASR result ([`post_processor.rs`, lines 126–176](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/post_processor.rs#L126-L176)).

One related macOS fix is worth understanding: its aggregate capture device uses only the process tap, rather than adding both the output device and a tap of the same output, which had captured playback twice ([`core_audio.rs`, lines 84–145](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/capture/core_audio.rs#L84-L145)). That prevents a duplicated capture graph; it is not acoustic echo cancellation for sound leaking from speakers into the mic.

### Its “partial transcript” state is not a strong model for two concurrent lanes

Meetily uses one serial transcription worker ([`worker.rs`, lines 39–68](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/transcription/worker.rs#L39-L68)). Each result receives a new sequence ID, and the frontend buffers/sorts results by audio time and sequence rather than maintaining a stable utterance and replacing its revisions ([`worker.rs`, lines 127–195](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/transcription/worker.rs#L127-L195); [`TranscriptContext.tsx`, lines 180–271](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src/contexts/TranscriptContext.tsx#L180-L271)).

Whisper labels a completed chunk shorter than 15 seconds as `is_partial`; it is not emitting successive revisions of the same utterance ([`whisper_engine.rs`, lines 515–577](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/whisper_engine/whisper_engine.rs#L515-L577)). Parakeet always reports non-partial ([`parakeet_provider.rs`, lines 36–41](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/transcription/parakeet_provider.rs#L36-L41)). The visible “streaming” effect is largely an 800 ms typewriter animation after a complete string arrives ([`TranscriptView.tsx`, lines 174–236](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src/components/TranscriptView.tsx#L174-L236)).

## The useful Meetily idea is in an open, unmerged PR

[Issue #642](https://github.com/Zackriya-Solutions/meetily/issues/642) describes the same loss of source provenance. [PR #661](https://github.com/Zackriya-Solutions/meetily/pull/661) proposes a practical correction, but as of 2026-08-22 it is open, targets `devtest`, and is not part of Community Edition `main`.

The proposal:

- introduces stable wire values for transcript source ([`recording_state.rs`, lines 19–34](https://github.com/Zackriya-Solutions/meetily/blob/d509f67f9d14d4e8221bae529e2ff801a2652607/frontend/src-tauri/src/audio/recording_state.rs#L19-L34));
- runs separate microphone and system VAD processors and preserves the source into ASR, while still mixing only for the saved recording ([`pipeline.rs`, lines 680–701](https://github.com/Zackriya-Solutions/meetily/blob/d509f67f9d14d4e8221bae529e2ff801a2652607/frontend/src-tauri/src/audio/pipeline.rs#L680-L701), [lines 823–950](https://github.com/Zackriya-Solutions/meetily/blob/d509f67f9d14d4e8221bae529e2ff801a2652607/frontend/src-tauri/src/audio/pipeline.rs#L823-L950));
- compares only cross-source transcripts whose audio windows overlap, using normalized Levenshtein similarity; it suppresses the microphone copy when system audio already exists, or reuses/replaces the earlier microphone sequence when system audio arrives later ([`worker.rs`, lines 43–123](https://github.com/Zackriya-Solutions/meetily/blob/d509f67f9d14d4e8221bae529e2ff801a2652607/frontend/src-tauri/src/audio/transcription/worker.rs#L43-L123) and [lines 301–363](https://github.com/Zackriya-Solutions/meetily/blob/d509f67f9d14d4e8221bae529e2ff801a2652607/frontend/src-tauri/src/audio/transcription/worker.rs#L301-L363)); and
- upserts frontend and IndexedDB records by sequence so a later system result can replace an earlier microphone duplicate ([`TranscriptContext.tsx`, lines 245–317](https://github.com/Zackriya-Solutions/meetily/blob/d509f67f9d14d4e8221bae529e2ff801a2652607/frontend/src/contexts/TranscriptContext.tsx#L245-L317); [`indexedDBService.ts`, lines 244–288](https://github.com/Zackriya-Solutions/meetily/blob/d509f67f9d14d4e8221bae529e2ff801a2652607/frontend/src/services/indexedDBService.ts#L244-L288)).

This is a useful blueprint, not proof. Its `0.82` text-similarity and `35%` overlap constants are heuristics, not validated universal thresholds. It does not implement AEC or named-speaker diarization. A single serial ASR worker may also accumulate delay when two people speak at once. The PR's tests cover ordering and basic replace/suppress cases ([`worker.rs`, lines 746–840](https://github.com/Zackriya-Solutions/meetily/blob/d509f67f9d14d4e8221bae529e2ff801a2652607/frontend/src-tauri/src/audio/transcription/worker.rs#L746-L840)), but no live Windows verification was found.

## Kimi Nola's current position

Kimi Nola already creates distinct microphone and loopback resamplers/ASR lanes and emits their results as `You` and `Others` ([`recording_session.rs`, lines 486–587](../../kiminola/src-tauri/src/recording_session.rs#L486-L587)). That is the right core architecture and should remain.

The immediate correctness gap is downstream identity/state:

- the backend event contains only `channel`, `text`, and `is_partial` ([`recording_session.rs`, lines 16–39](../../kiminola/src-tauri/src/recording_session.rs#L16-L39));
- the TypeScript event/line types contain no utterance ID, revision, or time window ([`tauri.ts`, lines 5–17](../../kiminola/src/lib/tauri.ts#L5-L17));
- the recording page maintains one global `partialIndex`, so a result from one lane can replace the latest partial from the other lane ([`+page.svelte`, lines 27–63](../../kiminola/src/routes/record/+page.svelte#L27-L63)); and
- the database schema has nullable `start_ms`/`end_ms`, but the save path persists only channel/text ([`0001_init.sql`, lines 19–27](../../kiminola/src-tauri/migrations/0001_init.sql#L19-L27); [`db.rs`, lines 73–93](../../kiminola/src-tauri/src/db.rs#L73-L93) and [lines 219–226](../../kiminola/src-tauri/src/db.rs#L219-L226)).

Current Kimi Nola loopback captures the default render endpoint, not a meeting process ([`loopback.rs`, lines 25–53](../../kiminola/src-tauri/src/loopback.rs#L25-L53)). Its mic and loopback callbacks also use `try_send`, so queue-full drops should be counted and tested rather than ignored ([`recording_session.rs`, line 263](../../kiminola/src-tauri/src/recording_session.rs#L263); [`loopback.rs`, lines 120–124](../../kiminola/src-tauri/src/loopback.rs#L120-L124)).

## Recommended Kimi Nola design

### 1. Fix live utterance identity first

Use a stable event contract along these lines:

```text
utterance_id: stable within one recording
channel: you | others
revision: increasing number
is_final: boolean
start_ms: audio-relative time
end_ms: audio-relative time (or provisional while partial)
text: current hypothesis
```

Maintain live UI state as a map keyed by `(channel, utterance_id)`. A new revision updates only that entry. Finalization makes it durable; ordering is by `start_ms`, with a deterministic tie-breaker. Persistence should upsert by stable utterance ID, then reject revisions older than the stored revision. This removes the cross-lane race caused by a single `partialIndex`.

### 2. Keep the audio/ASR lanes separate

The desired path is:

```text
microphone -> mic preprocessing -> mic VAD/ASR -> You ---------+
                                                                +-> ordered transcript state
system loopback -> system preprocessing -> system VAD/ASR -> Others
```

Do not mix before VAD or ASR. If Kimi Nola later retains audio, mixing can be a separate recording-output branch; it should never be the transcription branch. Keep per-source resampling/processors stateful across callbacks, flush both lanes on stop, and expose local queue/drop/flush metrics for tests and diagnostics.

### 3. Add conservative transcript-level echo duplicate suppression

After both source lanes produce timestamped final segments, compare only opposite-source candidates that overlap in audio time. Normalize text, require enough lexical content, then compute a similarity score. If a high-confidence duplicate is found:

- prefer `Others`/system audio because it is the direct digital source;
- suppress a later `You` duplicate; or
- replace an earlier `You` segment with the later `Others` segment using the same stable UI/persistence identity.

Meetily PR #661's 0.82 similarity and 35% overlap are reasonable experimental starting points, not values to copy as product truth. Short phrases such as “yes” or “okay,” genuine repetition by the local speaker, double-talk, and different ASR errors are the dangerous cases. Begin conservatively, instrument local debug decisions (no analytics), and tune from recorded fixtures.

Required regression cases include: headphones; laptop speakers; system-first and mic-first duplicate arrival; out-of-order finalization; genuine local repetition of remote words; overlapping different speakers; short acknowledgements; silent system stream; queue pressure; and stop/flush with trailing speech.

### 4. Add process-specific loopback with a measured fallback

Microsoft's official Application Loopback sample uses `ActivateAudioInterfaceAsync` with `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK` to include a target process tree or exclude it, independent of the hardware endpoint ([sample README](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/ApplicationLoopback/README.md); [`LoopbackCapture.cpp`](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/ApplicationLoopback/cpp/LoopbackCapture.cpp)). It requires Windows build 20348 or later.

This is attractive for Zoom/Teams/browser meetings because notification sounds and unrelated media do not become `Others`. It should be optional and guarded by silence detection. A field report against the Microsoft sample reports silence for Teams desktop while Zoom and browser Teams work ([Windows-classic-samples issue #414](https://github.com/microsoft/Windows-classic-samples/issues/414)); that is not a platform guarantee, but it is enough reason to retain automatic fallback to the current default-endpoint loopback.

### 5. Add AEC only as an opportunistic speakerphone layer

Microsoft's official AEC sample shows `IAcousticEchoCancellationControl::SetEchoCancellationRenderEndpoint`: a communications capture stream can use a render endpoint as the echo reference ([AEC sample README](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/AcousticEchoCancellation/README.md); [`AECCapture.cpp`](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/AcousticEchoCancellation/cpp/AECCapture.cpp)). The sample requires Windows 11 build 22540 or later and checks whether the device exposes the AEC effect.

This makes OS AEC a useful fast path when available, not a universal solution for Kimi Nola's Windows 10/hardware matrix. The safe fallback remains independent source ASR plus timestamp/text duplicate suppression. A universal embedded AEC library should be considered only after live tests show that post-ASR suppression is insufficient.

## What to borrow from Meetily

| Idea | Recommendation | Reason |
|---|---|---|
| Persistent per-device resampling and mic-only preprocessing | Reimplement/adapt | Correct state ownership; avoids resetting filters/resamplers per callback. |
| Independent source VAD/ASR from PR #661 | Keep Kimi Nola's existing version | Kimi Nola already has the better two-lane shape. |
| Ring-buffer alignment and zero-padding | Borrow only if a synchronized mixed recording is later needed | Useful for recording output, unnecessary for separate ASR. |
| Drain-until-close, explicit per-lane flush, queue/completion metrics | Borrow | Protects trailing speech and makes backpressure observable. |
| Cross-source overlap + text-similarity dedup from PR #661 | Prototype and tune | Practical echo fallback; thresholds and short-phrase handling need Kimi Nola fixtures. |
| Stable sequence replacement/upsert from PR #661 | Borrow the concept, use a real utterance/revision model | Supports late correction without duplicate UI/database rows. |
| Mix mic/system before VAD/ASR | Do not copy | Erases source identity and harms overlapping speech. |
| Meetily CE diarization | Nothing to copy | It is not implemented in the active CE path. |
| Meetily “intelligent ducking” constants | Do not copy | Comments/code disagree: a stated 50 ms/400 ms design is implemented as 600 ms windows with up to 4.8 seconds buffering ([`pipeline.rs`, lines 26–39](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/pipeline.rs#L26-L39)); the supposed mic scale is unused and system gain remains 1.0 ([lines 145–180](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/frontend/src-tauri/src/audio/pipeline.rs#L145-L180)). |

## License and reuse implications

Meetily Community Edition is MIT licensed ([`LICENSE.md`, lines 1–21](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/LICENSE.md#L1-L21)). Kimi Nola is also MIT licensed ([local `LICENSE`, lines 1–20](../../LICENSE#L1-L20)), so the licenses are compatible. If substantial Meetily code is copied, retain Meetily's copyright and MIT permission notice in the copied/substantial portion and add an appropriate third-party notice. A clean reimplementation of the architectural ideas reduces provenance complexity but does not remove the need to audit dependency/model licenses.

Meetily credits whisper.cpp, Screenpipe, transcribe-rs, and third-party Parakeet models ([`README.md`, lines 257–267](https://github.com/Zackriya-Solutions/meetily/blob/0281737d87d26352fb0adc78c8c0975f691b23d1/README.md#L257-L267)). Any copied file should be checked for upstream-derived portions and their notices. Meetily PRO is described as a separate codebase; this Community Edition MIT license does not establish permission to reuse PRO code or branding.

This is a source-license compatibility observation, not legal advice.

## Uncertainties and verification gaps

- Meetily Community Edition and PR #661 were inspected statically, not run. Audio driver behavior, Windows ARM64 compatibility, real echo characteristics, latency, and shutdown behavior remain unverified.
- PR #661 is open and unmerged. Its design and thresholds may change, and its current status is not release evidence.
- `audio_v2` exists in the Meetily tree but is not exported by the active crate; it also contains unfinished work. It was not treated as shipping behavior.
- No evidence was found that current Meetily CE identifies individual remote speakers. The conflicting frontend README claim appears stale or aspirational.
- Process-loopback behavior varies by application architecture. Kimi Nola needs live tests for Zoom, classic/new Teams, browser meetings, and multi-process trees, plus fallback tests.
- OS-provided AEC availability depends on Windows version, selected endpoints, mode, and device effects. Feature detection and fallback are required.
- Transcript duplicate suppression is probabilistic. It must never silently merge non-overlapping segments or same-source repetition, and should preserve enough debug evidence to explain a suppression decision locally.

## Primary sources

- [Meetily Community Edition repository at the inspected commit](https://github.com/Zackriya-Solutions/meetily/tree/0281737d87d26352fb0adc78c8c0975f691b23d1)
- [Meetily issue #642: source attribution request](https://github.com/Zackriya-Solutions/meetily/issues/642)
- [Meetily PR #661: unmerged source-attribution proposal](https://github.com/Zackriya-Solutions/meetily/pull/661)
- [CPAL WASAPI implementation at Meetily's pinned revision](https://github.com/RustAudio/cpal/blob/51c3b43c54591203fe3edc9a31ff7595650f7103/src/host/wasapi/device.rs#L558-L562)
- [Microsoft Application Loopback sample](https://github.com/microsoft/Windows-classic-samples/tree/main/Samples/ApplicationLoopback)
- [Microsoft Acoustic Echo Cancellation sample](https://github.com/microsoft/Windows-classic-samples/tree/main/Samples/AcousticEchoCancellation)
