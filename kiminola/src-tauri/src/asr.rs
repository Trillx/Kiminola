use std::path::PathBuf;
use std::sync::Arc;

use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};

/// The expensive half of ASR: a preloaded sherpa-onnx recognizer holding the
/// model weights. Built once (ideally in the background at app launch) and
/// shared by every recording session via cheap per-lane streams.
pub struct AsrEngine {
    recognizer: OnlineRecognizer,
}

/// One streaming ASR lane (mic or loopback) for a single recording session.
pub struct AsrLane {
    engine: Arc<AsrEngine>,
    stream: OnlineStream,
    /// Last text seen for this lane; used to suppress duplicate partial emissions.
    last_text: String,
}

impl AsrEngine {
    pub fn new(model_dir: &PathBuf) -> Option<Self> {
        let (encoder, decoder, joiner, tokens) = model_files(model_dir)?;

        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder = Some(encoder.to_string_lossy().into_owned());
        config.model_config.transducer.decoder = Some(decoder.to_string_lossy().into_owned());
        config.model_config.transducer.joiner = Some(joiner.to_string_lossy().into_owned());
        config.model_config.tokens = Some(tokens.to_string_lossy().into_owned());
        config.enable_endpoint = true;
        config.decoding_method = Some("greedy_search".into());
        // Measured on Snapdragon X Elite with batched dual-lane decode: 4
        // threads is the sweet spot; more slows down the small per-chunk calls.
        config.model_config.num_threads = 4;

        let recognizer = OnlineRecognizer::create(&config)?;
        Some(Self { recognizer })
    }

    /// Fresh per-recording lane; cheap (the recognizer owns the weights).
    pub fn lane(self: &Arc<Self>) -> AsrLane {
        AsrLane {
            engine: Arc::clone(self),
            stream: self.recognizer.create_stream(),
            last_text: String::new(),
        }
    }

    /// Decode every lane that has a full chunk buffered, in batched passes
    /// through the encoder (one step per lane per call, so loop until drained).
    /// Batching both lanes costs barely more than one — this is what keeps
    /// dual-lane decoding ahead of real time.
    pub fn decode_ready(&self, lanes: &mut [&mut AsrLane]) {
        loop {
            let ready: Vec<&OnlineStream> = lanes
                .iter()
                .filter(|l| self.recognizer.is_ready(&l.stream))
                .map(|l| &l.stream)
                .collect();
            if ready.is_empty() {
                break;
            }
            self.recognizer.decode_multiple_streams(&ready);
        }
    }
}

impl AsrLane {
    /// Buffer a chunk of 16 kHz mono f32 audio without decoding yet.
    pub fn feed(&mut self, samples: &[f32]) {
        self.stream.accept_waveform(16000, samples);
    }

    /// Feed a chunk and decode immediately (single-lane convenience path,
    /// exercised by the tests; the live pipeline uses feed + decode_ready).
    #[allow(dead_code)]
    pub fn push(&mut self, samples: &[f32]) -> Option<(String, bool)> {
        self.feed(samples);
        let engine = Arc::clone(&self.engine);
        engine.decode_ready(&mut [self]);
        self.take_result()
    }

    /// Return the newest transcript fragment if it changed since the last call.
    pub fn take_result(&mut self) -> Option<(String, bool)> {
        let result = self.engine.recognizer.get_result(&self.stream)?;
        let text = result.text.trim().to_string();

        if text == self.last_text {
            return None;
        }

        self.last_text = text.clone();
        Some((text, result.is_final))
    }

    /// Flush the stream and return any final result.
    pub fn finish(&mut self) -> Option<(String, bool)> {
        self.stream.input_finished();
        let recognizer = &self.engine.recognizer;
        while recognizer.is_ready(&self.stream) {
            recognizer.decode(&self.stream);
        }
        recognizer.get_result(&self.stream).map(|r| (r.text, true))
    }
}

/// Known model layouts, most-preferred first: the production Nemotron export,
/// then the small zipformer used during early bring-up.
fn model_files(dir: &PathBuf) -> Option<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    const LAYOUTS: &[(&str, &str, &str, &str)] = &[
        (
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ),
        (
            "encoder-epoch-99-avg-1.int8.onnx",
            "decoder-epoch-99-avg-1.onnx",
            "joiner-epoch-99-avg-1.int8.onnx",
            "tokens.txt",
        ),
    ];

    for (e, d, j, t) in LAYOUTS {
        let (e, d, j, t) = (dir.join(e), dir.join(d), dir.join(j), dir.join(t));
        if e.exists() && d.exists() && j.exists() && t.exists() {
            return Some((e, d, j, t));
        }
    }
    None
}

/// Resolve the ASR model directory.
///
/// Checks, in order:
/// 1. `%LOCALAPPDATA%\Kiminola\models\nemotron` (production model)
/// 2. `%LOCALAPPDATA%\Kiminola\models` (bring-up zipformer)
/// 3. `models[\/nemotron]` next to the running executable
pub fn resolve_asr_model_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(local_app_data).join("Kiminola").join("models");
        candidates.push(base.join("nemotron"));
        candidates.push(base);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("models").join("nemotron"));
            candidates.push(parent.join("models"));
        }
    }

    candidates.into_iter().find(|d| model_files(d).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: the sherpa-onnx runtime loads, the model initializes, and a
    /// second of silence decodes without panicking.
    #[test]
    fn session_decodes_silence() {
        let Some(model_dir) = resolve_asr_model_dir() else {
            eprintln!("ASR model dir not found; skipping");
            return;
        };

        let engine = Arc::new(AsrEngine::new(&model_dir).expect("recognizer should build from the model dir"));
        let mut session = engine.lane();

        let silence = vec![0.0f32; 16000];
        let _ = session.push(&silence);
        let _ = session.finish();
    }

    /// End-to-end: feed a real 16 kHz mono PCM WAV of spoken English through the
    /// recognizer in 100 ms chunks and expect non-empty text out.
    #[test]
    fn session_transcribes_speech_wav() {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(".scratch")
            .join("speech-test.wav");
        if !wav_path.exists() {
            eprintln!("speech-test.wav not found; skipping");
            return;
        }

        let Some(model_dir) = resolve_asr_model_dir() else {
            eprintln!("ASR model dir not found; skipping");
            return;
        };
        let engine = Arc::new(AsrEngine::new(&model_dir).expect("recognizer should build from the model dir"));
        let mut session = engine.lane();

        let samples = read_wav_pcm16_mono(&wav_path);
        assert!(!samples.is_empty(), "wav should contain samples");

        let mut last_text = String::new();
        let mut first_text_at_secs: Option<f32> = None;
        let mut fed_secs = 0.0f32;
        let mut emissions = 0u32;
        for chunk in samples.chunks(1600) {
            fed_secs += chunk.len() as f32 / 16000.0;
            if let Some((text, is_final)) = session.push(chunk) {
                emissions += 1;
                if first_text_at_secs.is_none() && !text.trim().is_empty() {
                    first_text_at_secs = Some(fed_secs);
                    eprintln!("first non-empty text after {fed_secs:.2}s of audio: {text:?} (is_final={is_final})");
                }
                last_text = text;
            }
        }
        eprintln!("push() emitted {emissions} times over {fed_secs:.1}s of audio");
        if let Some((text, _)) = session.finish() {
            if !text.trim().is_empty() {
                last_text = text;
            }
        }

        eprintln!("transcribed: {last_text:?}");
        assert!(
            !last_text.trim().is_empty(),
            "expected non-empty transcript for speech input"
        );
    }

    /// Replay a live mic dump (raw f32le at 16 kHz, written by the recording
    /// diagnostics) through the recognizer to compare live vs offline behavior.
    #[test]
    fn session_transcribes_mic_dump() {
        let dump_path = std::env::temp_dir().join("kiminola-mic-dump.f32");
        if !dump_path.exists() {
            eprintln!("mic dump not found; skipping");
            return;
        }
        let Some(model_dir) = resolve_asr_model_dir() else {
            eprintln!("ASR model dir not found; skipping");
            return;
        };
        let engine = Arc::new(AsrEngine::new(&model_dir).expect("recognizer should build from the model dir"));
        let mut session = engine.lane();

        let bytes = std::fs::read(&dump_path).expect("dump should be readable");
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        eprintln!("replaying {:.1}s of mic audio", samples.len() as f32 / 16000.0);

        let mut last_text = String::new();
        let mut first_text_at_secs: Option<f32> = None;
        let mut fed_secs = 0.0f32;
        let decode_start = std::time::Instant::now();
        for chunk in samples.chunks(1600) {
            fed_secs += chunk.len() as f32 / 16000.0;
            if let Some((text, _)) = session.push(chunk) {
                if first_text_at_secs.is_none() && !text.trim().is_empty() {
                    first_text_at_secs = Some(fed_secs);
                }
                last_text = text;
            }
        }
        let decode_wall = decode_start.elapsed();
        eprintln!(
            "decoded {fed_secs:.1}s of audio in {decode_wall:.2?} (RTF {:.2})",
            decode_wall.as_secs_f32() / fed_secs
        );
        if let Some((text, _)) = session.finish() {
            if !text.trim().is_empty() {
                last_text = text;
            }
        }
        eprintln!("first text after {first_text_at_secs:?}s of audio; final: {last_text:?}");
    }

    /// Worst-case throughput: two lanes (mic + loopback) both carrying speech,
    /// fed alternately like the live consumer does. Combined RTF must stay
    /// well under 1.0 or the channel backlog grows without bound.
    #[test]
    fn dual_lane_rtf() {
        let dump_path = std::env::temp_dir().join("kiminola-mic-dump.f32");
        if !dump_path.exists() {
            eprintln!("mic dump not found; skipping");
            return;
        }
        let Some(model_dir) = resolve_asr_model_dir() else {
            eprintln!("ASR model dir not found; skipping");
            return;
        };
        let engine = Arc::new(
            AsrEngine::new(&model_dir).expect("recognizer should build from the model dir"),
        );
        let mut lane_a = engine.lane();
        let mut lane_b = engine.lane();

        let bytes = std::fs::read(&dump_path).expect("dump should be readable");
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        let mut fed_secs = 0.0f32;
        let start = std::time::Instant::now();
        for chunk in samples.chunks(1600) {
            fed_secs += chunk.len() as f32 / 16000.0;
            lane_a.feed(chunk);
            lane_b.feed(chunk);
            engine.decode_ready(&mut [&mut lane_a, &mut lane_b]);
            let _ = lane_a.take_result();
            let _ = lane_b.take_result();
        }
        let wall = start.elapsed();
        eprintln!(
            "dual-lane batched: 2 x {fed_secs:.1}s decoded in {wall:.2?} (combined RTF {:.2})",
            wall.as_secs_f32() / fed_secs
        );
    }

    /// Minimal RIFF reader: 16-bit PCM mono, any sample rate (tests supply 16 kHz).
    fn read_wav_pcm16_mono(path: &std::path::Path) -> Vec<f32> {
        let bytes = std::fs::read(path).expect("wav should be readable");
        assert!(&bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE", "not a RIFF/WAVE file");

        let mut pos = 12;
        while pos + 8 <= bytes.len() {
            let tag = &bytes[pos..pos + 4];
            let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            if tag == b"data" {
                let data = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
                return data
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
                    .collect();
            }
            pos += 8 + size + (size % 2);
        }
        Vec::new()
    }
}
