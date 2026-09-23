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

    /// Explicit end-of-input for callers that must retain the last short chunk.
    /// The output includes zero padding and the interpolator's delayed samples.
    /// Meeting capture does not call this, so its streaming behavior is unchanged.
    pub fn finish(mut self) -> Result<Vec<f32>, String> {
        let mut output = self.drain_output();
        if !self.input_buffer.is_empty() {
            let input = vec![std::mem::take(&mut self.input_buffer)];
            let tail = self
                .inner
                .process_partial(Some(&input), None)
                .map_err(|_| "failed to flush resampler input".to_string())?;
            output.extend_from_slice(&tail[0]);
        }
        // Even a whole input block can leave real samples inside the interpolator.
        let delayed = self
            .inner
            .process_partial::<Vec<f32>>(None, None)
            .map_err(|_| "failed to flush resampler delay".to_string())?;
        output.extend_from_slice(&delayed[0]);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_retains_short_final_audio() {
        let mut resampler = ChannelResampler::new(48_000).unwrap();
        resampler.push(&[0.75; 240]);
        assert!(resampler.drain_output().is_empty());
        let tail = resampler.finish().unwrap();
        assert!(tail.iter().filter(|&&value| value > 0.5).count() >= 78);
    }

    #[test]
    fn finish_drains_interpolator_delay_after_an_exact_input_block() {
        let mut resampler = ChannelResampler::new(16_000).unwrap();
        let mut input = vec![0.0; 1024];
        input[1023] = 0.75;
        resampler.push(&input);
        let first = resampler.drain_output();
        let tail = resampler.finish().unwrap();
        assert!(first.iter().all(|&sample| sample == 0.0));
        assert!(tail.iter().any(|&sample| sample > 0.5));
    }

    #[test]
    fn final_output_does_not_depend_on_callback_chunk_sizes() {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
            let input: Vec<f32> = (0..2345).map(|index| (index as f32 * 0.01).sin()).collect();
            let mut whole = ChannelResampler::new(rate).unwrap();
            whole.push(&input);
            let expected = whole.finish().unwrap();
            let mut split = ChannelResampler::new(rate).unwrap();
            let mut actual = Vec::new();
            for chunk in input.chunks(137) {
                split.push(chunk);
                actual.extend(split.drain_output());
            }
            actual.extend(split.finish().unwrap());
            assert_eq!(actual, expected, "callback partition at {rate} Hz");
            assert!(actual.iter().all(|sample| sample.is_finite()));
        }
    }
}
