use rubato::{FastFixedIn, PolynomialDegree, Resampler};

const TARGET_RATE: u32 = 16000;

/// Real-time mono-to-mono resampler that accepts arbitrary input chunks and
/// drains resampled 16 kHz mono output on demand.
pub struct ChannelResampler {
    inner: FastFixedIn<f32>,
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
}

impl ChannelResampler {
    pub fn new(input_sample_rate: u32) -> Result<Self, String> {
        if input_sample_rate == 0 {
            return Err("input sample rate must be non-zero".into());
        }

        let ratio = TARGET_RATE as f64 / input_sample_rate as f64;
        let inner = FastFixedIn::new(ratio, 1.0, PolynomialDegree::Linear, 1024, 1)
            .map_err(|e| format!("failed to create resampler: {e}"))?;

        Ok(Self {
            inner,
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
        })
    }

    /// Push a chunk of mono f32 samples at the input sample rate.
    pub fn push(&mut self, samples: &[f32]) {
        self.input_buffer.extend_from_slice(samples);
    }

    /// Resample as much buffered input as possible and return all available
    /// 16 kHz mono output. The returned vector may be empty.
    pub fn drain_output(&mut self) -> Vec<f32> {
        loop {
            let needed = self.inner.input_frames_next();
            if self.input_buffer.len() < needed {
                break;
            }

            let input = vec![self.input_buffer[..needed].to_vec()];
            match self.inner.process(&input, None) {
                Ok(out) => {
                    self.output_buffer.extend_from_slice(&out[0]);
                }
                Err(e) => {
                    eprintln!("resampling failed: {e}");
                    break;
                }
            }
            self.input_buffer.drain(..needed);
        }

        std::mem::take(&mut self.output_buffer)
    }
}
