use std::path::PathBuf;

use ndarray::{Array2, Array3};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Value;

const WINDOW_SAMPLES: usize = 512; // 32 ms at 16 kHz
const HIDDEN_SIZE: usize = 64;
const NUM_LAYERS: usize = 2;

/// Thresholds for speech start/end hysteresis.
const SPEECH_THRESHOLD: f32 = 0.5;
const SILENCE_THRESHOLD: f32 = 0.35;

/// A Silero VAD session that scores 16 kHz mono chunks and tracks whether speech
/// is currently detected.
pub struct VadSession {
    session: Session,
    h: Array3<f32>,
    c: Array3<f32>,
    buffer: Vec<f32>,
    /// Number of consecutive windows currently classified as speech/silence.
    speech_count: i32,
    silence_count: i32,
    is_speaking: bool,
}

impl VadSession {
    pub fn new(model_path: &str) -> Result<Self, String> {
        let session = Session::builder()
            .map_err(|e| format!("failed to create ONNX session builder: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("failed to set optimization level: {e}"))?
            .commit_from_file(model_path)
            .map_err(|e| format!("failed to load VAD model from {model_path}: {e}"))?;

        Ok(Self {
            session,
            h: Array3::zeros((NUM_LAYERS, 1, HIDDEN_SIZE)),
            c: Array3::zeros((NUM_LAYERS, 1, HIDDEN_SIZE)),
            buffer: Vec::new(),
            speech_count: 0,
            silence_count: 0,
            is_speaking: false,
        })
    }

    /// Push a chunk of 16 kHz mono f32 samples and update the speech state.
    pub fn push(&mut self, samples: &[f32]) {
        self.buffer.extend_from_slice(samples);

        while self.buffer.len() >= WINDOW_SAMPLES {
            let window: Vec<f32> = self.buffer.drain(..WINDOW_SAMPLES).collect();
            let prob = self.score(&window);
            self.update_state(prob);
        }
    }

    /// Returns true if the VAD currently believes speech is active.
    /// Not consumed yet — ASR endpointing drives the transcript for now.
    #[allow(dead_code)]
    pub fn is_speaking(&self) -> bool {
        self.is_speaking
    }

    fn score(&mut self, window: &[f32]) -> f32 {
        let input = Array2::from_shape_vec((1, WINDOW_SAMPLES), window.to_vec())
            .expect("window length matches shape");

        let input_value = Value::from_array(input)
            .map_err(|e| format!("failed to create VAD input tensor: {e}"))
            .unwrap();
        let h_value = Value::from_array(self.h.clone())
            .map_err(|e| format!("failed to create VAD h tensor: {e}"))
            .unwrap();
        let c_value = Value::from_array(self.c.clone())
            .map_err(|e| format!("failed to create VAD c tensor: {e}"))
            .unwrap();

        let outputs = match self.session.run(ort::inputs![input_value, h_value, c_value]) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("VAD inference failed: {e}");
                return 0.0;
            }
        };

        let prob = outputs[0]
            .try_extract_tensor::<f32>()
            .map(|(_shape, data)| data[0])
            .unwrap_or(0.0);

        if let Ok((_shape, h_next)) = outputs[1].try_extract_tensor::<f32>() {
            self.h.assign(&Array3::from_shape_vec((NUM_LAYERS, 1, HIDDEN_SIZE), h_next.to_vec()).unwrap());
        }
        if let Ok((_shape, c_next)) = outputs[2].try_extract_tensor::<f32>() {
            self.c.assign(&Array3::from_shape_vec((NUM_LAYERS, 1, HIDDEN_SIZE), c_next.to_vec()).unwrap());
        }

        prob
    }

    fn update_state(&mut self, prob: f32) {
        if prob >= SPEECH_THRESHOLD {
            self.speech_count += 1;
            self.silence_count = 0;
            if self.speech_count >= 2 {
                self.is_speaking = true;
            }
        } else if prob <= SILENCE_THRESHOLD {
            self.silence_count += 1;
            self.speech_count = 0;
            if self.silence_count >= 4 {
                self.is_speaking = false;
            }
        }
    }
}

/// Resolve the Silero VAD model path.
///
/// Checks, in order:
/// 1. `%LOCALAPPDATA%\Kiminola\models\silero_vad.onnx`
/// 2. `models\silero_vad.onnx` next to the running executable
pub fn resolve_model_path() -> Option<PathBuf> {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let p = PathBuf::from(local_app_data)
            .join("Kiminola")
            .join("models")
            .join("silero_vad.onnx");
        if p.exists() {
            return Some(p);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let p = exe.parent()?.join("models").join("silero_vad.onnx");
        if p.exists() {
            return Some(p);
        }
    }

    None
}
