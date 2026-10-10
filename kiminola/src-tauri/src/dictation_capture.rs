//! Microphone-only dictation. Audio and recovery text never leave memory here.
//!
//! The OS thread owns the CPAL stream; a separate worker decodes. Stopping the
//! microphone must not wait for ASR, provider work, or an event consumer.

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Sample,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Condvar, Mutex,
};
use std::time::Duration;
use tokio::sync::oneshot;

use crate::asr::{AsrEngine, AsrLane};
use crate::resampler::ChannelResampler;

const AUDIO_QUEUE_CHUNKS: usize = 32;
const CHUNK_FRAMES: usize = 2048;
const SESSION_LIMIT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, serde::Serialize)]
pub struct MicrophoneOption {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub enum CaptureEvent {
    /// Complete current transcript, replacing the previous Text value.
    Text(String),
    Level(f32),
    Failed(String),
}

type EventSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

fn select_named_microphone<T>(devices: Vec<(String, T)>, id: &str) -> Result<T, String> {
    let mut matches = devices
        .into_iter()
        .filter(|(name, _)| microphone_id(name) == id);
    let (_, selected) = matches.next().ok_or("selected microphone is unavailable")?;
    if matches.next().is_some() {
        return Err("selected microphone name is ambiguous".into());
    }
    Ok(selected)
}

fn microphone_id(name: &str) -> String {
    format!("cpal-name:{name}")
}

fn named_microphones(host: &cpal::Host) -> Result<Vec<(String, cpal::Device)>, String> {
    host.input_devices()
        .map_err(|_| "could not enumerate microphones".to_string())?
        .map(|device| {
            let name = device
                .name()
                .map_err(|_| "could not identify a microphone".to_string())?;
            if name.is_empty() {
                return Err("microphone has no identifiable name".into());
            }
            Ok((name, device))
        })
        .collect()
}

/// CPAL 0.15 does not expose persistent Windows endpoint IDs. IDs therefore use
/// exact names, never enumeration indices. Duplicated names fail closed rather
/// than choosing a different device. The default option is represented by None.
pub fn list_microphones() -> Result<Vec<MicrophoneOption>, String> {
    let devices = named_microphones(&cpal::default_host())?;
    let mut seen = std::collections::HashSet::new();
    let mut options = Vec::with_capacity(devices.len());
    for (name, _) in devices {
        if !seen.insert(name.clone()) {
            return Err(
                "microphone names are ambiguous; use the Windows default or rename the devices"
                    .into(),
            );
        }
        options.push(MicrophoneOption {
            id: microphone_id(&name),
            name,
        });
    }
    options.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(options)
}

pub struct DictationCapture {
    control: Arc<Control>,
    finished: Option<oneshot::Receiver<Result<String, String>>>,
}

impl DictationCapture {
    pub async fn start(
        engine: Arc<AsrEngine>,
        microphone_id: Option<String>,
        sink: EventSink,
    ) -> Result<Self, String> {
        Self::start_with(
            move |input| CpalMicrophone::open(microphone_id, input),
            move || Ok(engine.lane()),
            sink,
        )
        .await
    }

    /// Request microphone shutdown without waiting for ASR or the event sink.
    /// Available text is still drained; the caller decides whether to discard it.
    pub fn cancel(&self) {
        self.control.stop();
    }

    pub async fn finish(mut self) -> Result<String, String> {
        self.cancel();
        self.finished
            .take()
            .ok_or("capture result is unavailable")?
            .await
            .map_err(|_| "dictation worker stopped".to_string())?
    }

    async fn start_with<S, D>(
        open: impl FnOnce(InputPort) -> Result<S, String> + Send + 'static,
        decoder: impl FnOnce() -> Result<D, String> + Send + 'static,
        sink: EventSink,
    ) -> Result<Self, String>
    where
        S: CaptureSource + 'static,
        D: Decoder + Send + 'static,
    {
        Self::start_with_limit(open, decoder, sink, SESSION_LIMIT).await
    }

    async fn start_with_limit<S, D>(
        open: impl FnOnce(InputPort) -> Result<S, String> + Send + 'static,
        decoder: impl FnOnce() -> Result<D, String> + Send + 'static,
        sink: EventSink,
        limit: Duration,
    ) -> Result<Self, String>
    where
        S: CaptureSource + 'static,
        D: Decoder + Send + 'static,
    {
        let control = Arc::new(Control::default());
        let (started_tx, started_rx) = oneshot::channel();
        let (finished_tx, finished_rx) = oneshot::channel();
        // This guard also covers a dropped/aborted start future.
        let capture = Self {
            control: control.clone(),
            finished: Some(finished_rx),
        };
        std::thread::Builder::new()
            .name("dictation-microphone".into())
            .spawn(move || {
                let mut started = Some(started_tx);
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    run_capture(open, decoder, &control, &sink, &mut started, limit)
                }))
                .unwrap_or_else(|_| Err("dictation capture worker failed".into()));
                if let Err(error) = &outcome {
                    control.fail(error.clone());
                }
                if let Some(started) = started {
                    let error = outcome
                        .as_ref()
                        .err()
                        .cloned()
                        .unwrap_or_else(|| "dictation startup was interrupted".into());
                    let _ = started.send(Err(error));
                }
                control.report_failure(&sink);
                let _ = finished_tx.send(outcome);
            })
            .map_err(|_| "could not create dictation microphone worker".to_string())?;
        started_rx
            .await
            .map_err(|_| "dictation startup worker stopped".to_string())??;
        Ok(capture)
    }
}

impl Drop for DictationCapture {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Default)]
struct Control {
    stopped: AtomicBool,
    failure_reported: AtomicBool,
    wake: Condvar,
    error: Mutex<Option<String>>,
}

impl Control {
    fn stop(&self) {
        let _guard = self.error.lock().unwrap_or_else(|error| error.into_inner());
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
    }

    fn fail(&self, message: String) {
        let mut error = self.error.lock().unwrap_or_else(|error| error.into_inner());
        if error.is_none() {
            *error = Some(message);
        }
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
    }

    fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn report_failure(&self, sink: &EventSink) {
        if let Some(error) = self.error() {
            if !self.failure_reported.swap(true, Ordering::AcqRel) {
                // Never called from CPAL's realtime callback or while holding a lock.
                let _ = catch_unwind(AssertUnwindSafe(|| sink(CaptureEvent::Failed(error))));
            }
        }
    }

    fn wait(&self, limit: Duration) {
        let guard = self.error.lock().unwrap_or_else(|error| error.into_inner());
        let (mut guard, timeout) = self
            .wake
            .wait_timeout_while(guard, limit, |_| !self.stopped.load(Ordering::Acquire))
            .unwrap_or_else(|error| error.into_inner());
        if timeout.timed_out() && !self.stopped.load(Ordering::Acquire) {
            *guard = Some("dictation reached the ten-minute limit".into());
            self.stopped.store(true, Ordering::Release);
        }
    }
}

fn run_capture<S, D>(
    open: impl FnOnce(InputPort) -> Result<S, String>,
    make_decoder: impl FnOnce() -> Result<D, String>,
    control: &Arc<Control>,
    sink: &EventSink,
    started: &mut Option<oneshot::Sender<Result<(), String>>>,
    limit: Duration,
) -> Result<String, String>
where
    S: CaptureSource,
    D: Decoder + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(AUDIO_QUEUE_CHUNKS);
    let source = open(InputPort {
        sender,
        control: control.clone(),
    })?;
    if control.stopped.load(Ordering::Acquire) {
        return Err("dictation startup was interrupted".into());
    }
    let resampler = ChannelResampler::new(source.sample_rate())?;
    let decoder = make_decoder()?;
    let decoder_control = control.clone();
    let decoder_sink = sink.clone();
    let worker = std::thread::Builder::new()
        .name("dictation-asr".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                decode_audio(receiver, resampler, decoder, &decoder_sink)
            }))
            .unwrap_or_else(|_| Err("dictation decoder failed".into()));
            if let Err(error) = &result {
                decoder_control.fail(error.clone());
            }
            result
        })
        .map_err(|_| "could not create dictation decoder worker".to_string())?;

    let playback = catch_unwind(AssertUnwindSafe(|| {
        if control.stopped.load(Ordering::Acquire) {
            return Err("dictation startup was interrupted".into());
        }
        source.play()
    }))
    .unwrap_or_else(|_| Err("dictation microphone startup failed".into()));
    match playback {
        Ok(()) => {
            if let Some(started) = started.take() {
                if started.send(Ok(())).is_err() {
                    control.stop();
                }
            }
            control.wait(limit);
        }
        Err(error) => control.fail(error),
    }
    drop(source); // Disconnects the input queue and closes the mic BEFORE ASR drains.
    control.report_failure(sink);
    let result = worker
        .join()
        .unwrap_or_else(|_| Err("dictation decoder failed".into()));
    match control.error() {
        Some(error) => Err(error),
        None => result,
    }
}

fn decode_audio<D: Decoder>(
    receiver: mpsc::Receiver<Vec<f32>>,
    mut resampler: ChannelResampler,
    mut decoder: D,
    sink: &EventSink,
) -> Result<String, String> {
    let mut text = String::new();
    // ASR revisions replace only the current utterance. Endpoint finality must
    // commit it even when the text matches the last partial we published.
    let mut finalized = String::new();
    let mut partial = String::new();
    let mut publish = |revision: Option<(String, bool)>, text: &mut String| {
        if let Some((revision, is_final)) = revision {
            let revision = revision.trim();
            if !revision.is_empty() {
                partial = revision.to_string();
            }
            if is_final && !partial.is_empty() {
                if !finalized.is_empty() {
                    finalized.push(' ');
                }
                finalized.push_str(&partial);
                partial.clear();
            }
            let current = match (finalized.is_empty(), partial.is_empty()) {
                (false, false) => format!("{finalized} {partial}"),
                (_, true) => finalized.clone(),
                (true, false) => partial.clone(),
            };
            if current != *text {
                *text = current;
                sink(CaptureEvent::Text(text.clone()));
            }
        }
    };
    for samples in receiver {
        let level = (samples.iter().map(|sample| sample * sample).sum::<f32>()
            / samples.len().max(1) as f32)
            .sqrt()
            .clamp(0.0, 1.0);
        sink(CaptureEvent::Level(level));
        resampler.push(&samples);
        let audio = resampler.drain_output();
        if !audio.is_empty() {
            publish(decoder.push(&audio)?, &mut text);
        }
    }
    let tail = resampler.finish()?;
    if !tail.is_empty() {
        publish(decoder.push(&tail)?, &mut text);
    }
    // Streaming transducers need trailing silence to release encoder context.
    // It can finalize an utterance and reset the lane, so publish it BEFORE
    // finish: an empty/new finish result must not hide the endpoint's text.
    publish(decoder.push(&[0.0; 4800])?, &mut text);
    publish(decoder.finish()?, &mut text);
    sink(CaptureEvent::Level(0.0));
    Ok(text)
}

trait CaptureSource {
    fn sample_rate(&self) -> u32;
    fn play(&self) -> Result<(), String>;
}

trait Decoder {
    fn push(&mut self, samples: &[f32]) -> Result<Option<(String, bool)>, String>;
    fn finish(&mut self) -> Result<Option<(String, bool)>, String>;
}

impl Decoder for AsrLane {
    fn push(&mut self, samples: &[f32]) -> Result<Option<(String, bool)>, String> {
        Ok(AsrLane::push(self, samples))
    }

    fn finish(&mut self) -> Result<Option<(String, bool)>, String> {
        Ok(AsrLane::finish(self))
    }
}

struct CpalMicrophone {
    stream: cpal::Stream,
    sample_rate: u32,
}

impl CpalMicrophone {
    fn open(selected: Option<String>, input: InputPort) -> Result<Self, String> {
        let host = cpal::default_host();
        // Resolve once. No default-device watcher and no mid-session fallback.
        let device = match selected {
            Some(id) => select_named_microphone(named_microphones(&host)?, &id)?,
            None => host
                .default_input_device()
                .ok_or("no default microphone is available")?,
        };
        let supported = device
            .default_input_config()
            .map_err(|_| "could not read microphone configuration".to_string())?;
        let config = supported.config();
        if config.channels == 0 || config.sample_rate.0 == 0 {
            return Err("microphone configuration is invalid".into());
        }
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, input),
            cpal::SampleFormat::F64 => build_stream::<f64>(&device, &config, input),
            cpal::SampleFormat::I8 => build_stream::<i8>(&device, &config, input),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, input),
            cpal::SampleFormat::I32 => build_stream::<i32>(&device, &config, input),
            cpal::SampleFormat::I64 => build_stream::<i64>(&device, &config, input),
            cpal::SampleFormat::U8 => build_stream::<u8>(&device, &config, input),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, input),
            cpal::SampleFormat::U32 => build_stream::<u32>(&device, &config, input),
            cpal::SampleFormat::U64 => build_stream::<u64>(&device, &config, input),
            _ => Err("microphone sample format is unsupported".into()),
        }?;
        Ok(Self {
            stream,
            sample_rate: config.sample_rate.0,
        })
    }
}

impl CaptureSource for CpalMicrophone {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn play(&self) -> Result<(), String> {
        self.stream
            .play()
            .map_err(|_| "could not start the microphone".into())
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    input: InputPort,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    let failed = input.control.clone();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                for chunk in data.chunks(CHUNK_FRAMES * channels) {
                    let mono: Vec<f32> = chunk
                        .chunks_exact(channels)
                        .map(|frame| {
                            frame
                                .iter()
                                .map(|&sample| f32::from_sample(sample))
                                .sum::<f32>()
                                / channels as f32
                        })
                        .collect();
                    input.push(&mono);
                }
            },
            move |_| failed.fail("microphone disconnected or stopped unexpectedly".into()),
            None,
        )
        .map_err(|_| "could not open the microphone".into())
}

struct InputPort {
    sender: mpsc::SyncSender<Vec<f32>>,
    control: Arc<Control>,
}

impl InputPort {
    fn push(&self, samples: &[f32]) {
        for chunk in samples.chunks(CHUNK_FRAMES) {
            if self.control.stopped.load(Ordering::Acquire) {
                break;
            }
            let audio = chunk
                .iter()
                .map(|sample| {
                    if sample.is_finite() {
                        sample.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    }
                })
                .collect();
            if self.sender.try_send(audio).is_err() {
                self.control
                    .fail("dictation audio could not be processed in time".into());
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };
    use std::time::Duration;

    #[test]
    fn selected_microphone_rejects_ambiguous_names_instead_of_switching() {
        assert!(select_named_microphone(
            vec![("USB mic".into(), 1), ("USB mic".into(), 2)],
            "cpal-name:USB mic"
        )
        .is_err());
        assert!(
            select_named_microphone(vec![("other mic".into(), 3)], "cpal-name:USB mic").is_err()
        );
        assert_eq!(
            select_named_microphone(vec![("USB mic".into(), 4)], "cpal-name:USB mic").unwrap(),
            4
        );
    }

    struct SyntheticMicrophone {
        input: InputPort,
        released: Arc<AtomicBool>,
    }

    impl CaptureSource for SyntheticMicrophone {
        fn sample_rate(&self) -> u32 {
            48_000
        }
        fn play(&self) -> Result<(), String> {
            // Too short to leave the resampler until explicit finalization.
            self.input.push(&[0.5; 240]);
            Ok(())
        }
    }

    impl Drop for SyntheticMicrophone {
        fn drop(&mut self) {
            self.released.store(true, Ordering::SeqCst);
        }
    }

    #[derive(Default)]
    struct SyntheticDecoder {
        heard_audio: bool,
    }

    impl Decoder for SyntheticDecoder {
        fn push(&mut self, samples: &[f32]) -> Result<Option<(String, bool)>, String> {
            self.heard_audio |= samples.iter().any(|&sample| sample > 0.1);
            Ok(None)
        }
        fn finish(&mut self) -> Result<Option<(String, bool)>, String> {
            Ok(self
                .heard_audio
                .then(|| ("synthetic words".to_string(), true)))
        }
    }

    #[derive(Default)]
    struct ScriptedDecoder {
        revisions: std::collections::VecDeque<(&'static str, bool)>,
        silence_revision: Option<(&'static str, bool)>,
        final_revision: Option<(&'static str, bool)>,
    }

    impl Decoder for ScriptedDecoder {
        fn push(&mut self, samples: &[f32]) -> Result<Option<(String, bool)>, String> {
            let revision = if samples == [0.0; 4800] {
                self.silence_revision.take()
            } else {
                self.revisions.pop_front()
            };
            Ok(revision.map(|(text, is_final)| (text.into(), is_final)))
        }

        fn finish(&mut self) -> Result<Option<(String, bool)>, String> {
            assert!(self.revisions.is_empty(), "all audio must reach ASR");
            Ok(self
                .final_revision
                .take()
                .map(|(text, is_final)| (text.into(), is_final)))
        }
    }

    async fn scripted_capture(decoder: ScriptedDecoder) -> (String, Vec<String>) {
        struct ScriptedMicrophone {
            input: InputPort,
            chunks: usize,
        }
        impl CaptureSource for ScriptedMicrophone {
            fn sample_rate(&self) -> u32 {
                48_000
            }
            fn play(&self) -> Result<(), String> {
                for _ in 0..self.chunks {
                    self.input.push(&[0.5; CHUNK_FRAMES]);
                }
                Ok(())
            }
        }
        let chunks = decoder.revisions.len();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let capture = DictationCapture::start_with(
            move |input| Ok(ScriptedMicrophone { input, chunks }),
            move || Ok(decoder),
            Arc::new(move |event| {
                if let CaptureEvent::Text(text) = event {
                    observed.lock().unwrap().push(text);
                }
            }),
        )
        .await
        .unwrap();
        let text = tokio::time::timeout(Duration::from_secs(2), capture.finish())
            .await
            .expect("scripted capture must finish without microphone access")
            .unwrap();
        let revisions = events.lock().unwrap().clone();
        (text, revisions)
    }

    #[tokio::test]
    async fn utterance_endpoints_preserve_final_text_while_later_partials_change() {
        let (text, events) = scripted_capture(ScriptedDecoder {
            revisions: [
                (" first utterance ", false),
                ("first utterance", false),
                ("first utterance", true),
                ("secon", false),
                ("second revised", false),
            ]
            .into(),
            final_revision: Some(("second complete", true)),
            ..Default::default()
        })
        .await;
        assert_eq!(text, "first utterance second complete");
        assert_eq!(
            events,
            [
                "first utterance",
                "first utterance secon",
                "first utterance second revised",
                "first utterance second complete",
            ]
        );
    }

    #[tokio::test]
    async fn utterance_endpoint_from_trailing_silence_survives_empty_finish() {
        let (text, events) = scripted_capture(ScriptedDecoder {
            revisions: [("opening", true), ("last par", false)].into(),
            silence_revision: Some(("last complete", true)),
            final_revision: Some(("", true)),
        })
        .await;
        assert_eq!(text, "opening last complete");
        assert_eq!(
            events,
            ["opening", "opening last par", "opening last complete"]
        );
    }

    #[tokio::test]
    async fn utterance_endpoints_keep_intentionally_repeated_words() {
        let (text, events) = scripted_capture(ScriptedDecoder {
            revisions: [("yes", false), ("yes", true), ("yes", false), ("yes", true)].into(),
            ..Default::default()
        })
        .await;
        assert_eq!(text, "yes yes");
        assert_eq!(events, ["yes", "yes yes"]);
    }

    #[tokio::test]
    async fn utterance_empty_endpoint_commits_partial_before_the_next_revision() {
        let (text, events) = scripted_capture(ScriptedDecoder {
            revisions: [
                ("first", false),
                (" ", true),
                ("", false),
                ("second wrong", false),
                ("second", false),
                ("", true),
            ]
            .into(),
            ..Default::default()
        })
        .await;
        assert_eq!(text, "first second");
        assert_eq!(events, ["first", "first second wrong", "first second"]);
    }

    #[tokio::test]
    async fn utterance_partial_from_trailing_silence_is_replaced_by_finish() {
        let (text, events) = scripted_capture(ScriptedDecoder {
            revisions: [("first", true), ("sec", false)].into(),
            silence_revision: Some(("second pending", false)),
            final_revision: Some(("second complete", true)),
        })
        .await;
        assert_eq!(text, "first second complete");
        assert_eq!(
            events,
            [
                "first",
                "first sec",
                "first second pending",
                "first second complete"
            ]
        );
    }

    #[tokio::test]
    async fn utterance_endpoint_from_trailing_silence_precedes_new_finish_text() {
        let (text, events) = scripted_capture(ScriptedDecoder {
            revisions: [("first", true)].into(),
            silence_revision: Some(("second", true)),
            final_revision: Some(("third", true)),
        })
        .await;
        assert_eq!(text, "first second third");
        assert_eq!(events, ["first", "first second", "first second third"]);
    }

    async fn wait_for_release(released: &AtomicBool) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !released.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("microphone must be released");
    }

    #[tokio::test]
    async fn time_limit_stops_capture_with_review_error_without_caller_stop() {
        let released = Arc::new(AtomicBool::new(false));
        let source_released = released.clone();
        let failed = Arc::new(AtomicBool::new(false));
        let observed = failed.clone();
        let capture = DictationCapture::start_with_limit(
            move |input| Ok(SyntheticMicrophone { input, released: source_released }),
            || Ok(SyntheticDecoder::default()),
            Arc::new(move |event| {
                if matches!(event, CaptureEvent::Failed(ref error) if error.contains("ten-minute limit")) {
                    observed.store(true, Ordering::SeqCst);
                }
            }),
            Duration::from_millis(10),
        ).await.unwrap();
        let timed_out = tokio::time::timeout(Duration::from_millis(500), async {
            while !failed.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        capture.cancel();
        let outcome = capture.finish().await;
        assert!(
            timed_out.is_ok(),
            "capture must enforce its time limit itself"
        );
        assert!(released.load(Ordering::SeqCst));
        assert_eq!(
            outcome.unwrap_err(),
            "dictation reached the ten-minute limit"
        );
    }

    #[tokio::test]
    async fn cancel_wakes_microphone_without_waiting_for_a_blocked_sink() {
        let released = Arc::new(AtomicBool::new(false));
        let source_released = released.clone();
        let (entered_tx, entered_rx) = oneshot::channel();
        let entered_tx = Mutex::new(Some(entered_tx));
        let (unblock_tx, unblock_rx) = mpsc::channel();
        let unblock_rx = Mutex::new(unblock_rx);
        let capture = DictationCapture::start_with(
            move |input| {
                Ok(SyntheticMicrophone {
                    input,
                    released: source_released,
                })
            },
            || Ok(SyntheticDecoder::default()),
            Arc::new(move |_| {
                if let Some(entered) = entered_tx.lock().unwrap().take() {
                    let _ = entered.send(());
                    let _ = unblock_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3));
                }
            }),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        capture.cancel();
        wait_for_release(&released).await;
        unblock_tx.send(()).unwrap();
        assert_eq!(capture.finish().await.unwrap(), "synthetic words");
    }

    #[tokio::test]
    async fn dropping_capture_releases_the_microphone() {
        let released = Arc::new(AtomicBool::new(false));
        let source_released = released.clone();
        let capture = DictationCapture::start_with(
            move |input| {
                Ok(SyntheticMicrophone {
                    input,
                    released: source_released,
                })
            },
            || Ok(SyntheticDecoder::default()),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        drop(capture);
        wait_for_release(&released).await;
    }

    #[tokio::test]
    async fn aborting_start_never_plays_the_late_microphone() {
        struct UnplayedSource {
            mic: SyntheticMicrophone,
            played: Arc<AtomicBool>,
        }
        impl CaptureSource for UnplayedSource {
            fn sample_rate(&self) -> u32 {
                self.mic.sample_rate()
            }
            fn play(&self) -> Result<(), String> {
                self.played.store(true, Ordering::SeqCst);
                Ok(())
            }
        }
        let released = Arc::new(AtomicBool::new(false));
        let source_released = released.clone();
        let played = Arc::new(AtomicBool::new(false));
        let source_played = played.clone();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (unblock_tx, unblock_rx) = mpsc::channel();
        let task = tokio::spawn(DictationCapture::start_with(
            move |input| {
                let _ = entered_tx.send(());
                let _ = unblock_rx.recv_timeout(Duration::from_secs(3));
                Ok(UnplayedSource {
                    mic: SyntheticMicrophone {
                        input,
                        released: source_released,
                    },
                    played: source_played,
                })
            },
            || Ok(SyntheticDecoder::default()),
            Arc::new(|_| {}),
        ));
        entered_rx.await.unwrap();
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        unblock_tx.send(()).unwrap();
        wait_for_release(&released).await;
        assert!(!played.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn decoder_startup_error_releases_the_microphone_before_returning() {
        let released = Arc::new(AtomicBool::new(false));
        let source_released = released.clone();
        let result = DictationCapture::start_with(
            move |input| {
                Ok(SyntheticMicrophone {
                    input,
                    released: source_released,
                })
            },
            || Err::<SyntheticDecoder, _>("decoder unavailable".into()),
            Arc::new(|_| {}),
        )
        .await;
        assert!(matches!(result, Err(error) if error == "decoder unavailable"));
        assert!(released.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn failed_finalization_keeps_the_latest_text_callback() {
        struct FailingDecoder;
        impl Decoder for FailingDecoder {
            fn push(&mut self, _: &[f32]) -> Result<Option<(String, bool)>, String> {
                Ok(Some(("words retained for review".into(), false)))
            }
            fn finish(&mut self) -> Result<Option<(String, bool)>, String> {
                Err("decoder finalization failed".into())
            }
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let capture = DictationCapture::start_with(
            |input| {
                Ok(SyntheticMicrophone {
                    input,
                    released: Arc::new(AtomicBool::new(false)),
                })
            },
            || Ok(FailingDecoder),
            Arc::new(move |event| observed.lock().unwrap().push(event)),
        )
        .await
        .unwrap();
        assert_eq!(
            capture.finish().await.unwrap_err(),
            "decoder finalization failed"
        );
        let events = events.lock().unwrap();
        assert!(events.iter().any(
            |event| matches!(event, CaptureEvent::Text(text) if text == "words retained for review")
        ));
        assert_eq!(events.iter().filter(|event| matches!(event, CaptureEvent::Failed(error) if error == "decoder finalization failed")).count(), 1);
    }

    #[tokio::test]
    async fn overflow_fails_instead_of_silently_dropping_audio() {
        struct OverflowSource {
            mic: SyntheticMicrophone,
        }
        impl CaptureSource for OverflowSource {
            fn sample_rate(&self) -> u32 {
                self.mic.sample_rate()
            }
            fn play(&self) -> Result<(), String> {
                // Fill before the worker can receive. This is the same bounded
                // port used by the native callback, with no microphone access.
                for _ in 0..AUDIO_QUEUE_CHUNKS + 1 {
                    self.mic.input.push(&[0.5; CHUNK_FRAMES]);
                }
                Ok(())
            }
        }
        let control = Arc::new(Control::default());
        let (sender, receiver) = mpsc::sync_channel(AUDIO_QUEUE_CHUNKS);
        let released = Arc::new(AtomicBool::new(false));
        let source = OverflowSource {
            mic: SyntheticMicrophone {
                input: InputPort {
                    sender,
                    control: control.clone(),
                },
                released,
            },
        };
        source.play().unwrap();
        assert!(control.stopped.load(Ordering::Acquire));
        assert_eq!(
            control.error().as_deref(),
            Some("dictation audio could not be processed in time")
        );
        assert_eq!(receiver.try_iter().count(), AUDIO_QUEUE_CHUNKS);
    }

    #[test]
    fn input_port_bounds_audio_and_sanitizes_non_finite_samples() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let input = InputPort {
            sender,
            control: Arc::new(Control::default()),
        };
        input.push(&[f32::NAN, f32::INFINITY, -2.0, 2.0, 0.5]);
        assert_eq!(receiver.recv().unwrap(), vec![0.0, 0.0, -1.0, 1.0, 0.5]);
        input.control.stop();
        input.push(&[1.0]);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn capture_handle_and_public_start_future_are_send() {
        fn assert_send<T: Send>() {}
        fn assert_future_send<T: std::future::Future + Send>(_: T) {}
        assert_send::<DictationCapture>();
        // Do not poll the future: this type check must never open a microphone.
        fn check(engine: Arc<AsrEngine>) {
            assert_future_send(DictationCapture::start(engine, None, Arc::new(|_| {})));
        }
        let _ = check;
        let _: fn() -> Result<Vec<MicrophoneOption>, String> = list_microphones;
    }

    #[tokio::test]
    async fn device_failure_closes_microphone_and_reports_before_decoder_finishes() {
        struct StalledDecoder {
            entered: Option<oneshot::Sender<()>>,
            release: mpsc::Receiver<()>,
        }
        impl Decoder for StalledDecoder {
            fn push(&mut self, _: &[f32]) -> Result<Option<(String, bool)>, String> {
                if let Some(entered) = self.entered.take() {
                    let _ = entered.send(());
                    let _ = self.release.recv_timeout(Duration::from_secs(3));
                }
                Ok(Some(("recover these words".into(), false)))
            }
            fn finish(&mut self) -> Result<Option<(String, bool)>, String> {
                Ok(None)
            }
        }
        struct RunningSource {
            mic: SyntheticMicrophone,
        }
        impl CaptureSource for RunningSource {
            fn sample_rate(&self) -> u32 {
                48_000
            }
            fn play(&self) -> Result<(), String> {
                self.mic.input.push(&[0.5; 2048]);
                Ok(())
            }
        }
        let released = Arc::new(AtomicBool::new(false));
        let source_released = released.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        let (control_tx, control_rx) = oneshot::channel();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let capture = DictationCapture::start_with(
            move |input| {
                let _ = control_tx.send(input.control.clone());
                Ok(RunningSource {
                    mic: SyntheticMicrophone {
                        input,
                        released: source_released,
                    },
                })
            },
            move || {
                Ok(StalledDecoder {
                    entered: Some(entered_tx),
                    release: release_rx,
                })
            },
            Arc::new(move |event| observed.lock().unwrap().push(event)),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        control_rx
            .await
            .unwrap()
            .fail("microphone disconnected".into());
        let reported = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let failed = events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|event| matches!(event, CaptureEvent::Failed(_)));
                if released.load(Ordering::SeqCst) && failed {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        release_tx.send(()).unwrap();
        let result = capture.finish().await;
        assert!(
            reported.is_ok(),
            "failure notification must not wait for decoding"
        );
        assert_eq!(result.unwrap_err(), "microphone disconnected");
        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, CaptureEvent::Failed(_)))
                .count(),
            1
        );
        assert!(events.iter().any(
            |event| matches!(event, CaptureEvent::Text(text) if text == "recover these words")
        ));
    }

    #[tokio::test]
    async fn finish_drains_short_audio_before_returning_authoritative_text() {
        let released = Arc::new(AtomicBool::new(false));
        let events = Arc::new(Mutex::new(Vec::new()));
        let source_released = released.clone();
        let observed = events.clone();
        let capture = DictationCapture::start_with(
            move |input| {
                Ok(SyntheticMicrophone {
                    input,
                    released: source_released,
                })
            },
            || Ok(SyntheticDecoder::default()),
            Arc::new(move |event| observed.lock().unwrap().push(event)),
        )
        .await
        .unwrap();
        assert!(!released.load(Ordering::SeqCst));
        let text = tokio::time::timeout(Duration::from_secs(2), capture.finish())
            .await
            .expect("capture must stop")
            .unwrap();
        assert_eq!(text, "synthetic words");
        assert!(released.load(Ordering::SeqCst));
        let events = events.lock().unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, CaptureEvent::Text(text) if text == "synthetic words")));
        assert!(events.iter().any(
            |event| matches!(event, CaptureEvent::Level(level) if *level > 0.0 && *level <= 1.0)
        ));
        assert!(!events
            .iter()
            .any(|event| matches!(event, CaptureEvent::Failed(_))));
    }

    #[tokio::test]
    #[ignore = "requires installed Nemotron pack; synthetic silence only, never opens a mic"]
    async fn installed_asr_drains_synthetic_capture_without_microphone_access() {
        struct SilenceSource {
            input: InputPort,
        }
        impl CaptureSource for SilenceSource {
            fn sample_rate(&self) -> u32 {
                48_000
            }
            fn play(&self) -> Result<(), String> {
                self.input.push(&[0.0; 4800]);
                Ok(())
            }
        }
        let model_dir = crate::asr::resolve_asr_model_dir().expect("Nemotron pack required");
        let engine = Arc::new(AsrEngine::new(&model_dir).expect("model must initialize"));
        // Reuse the same recognizer weights for distinct dictation sessions.
        for _ in 0..2 {
            let engine = engine.clone();
            let events = Arc::new(Mutex::new(Vec::new()));
            let observed = events.clone();
            let capture = DictationCapture::start_with(
                |input| Ok(SilenceSource { input }),
                move || Ok(engine.lane()),
                Arc::new(move |event| observed.lock().unwrap().push(event)),
            )
            .await
            .unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(30), capture.finish())
                    .await
                    .expect("synthetic capture must finish")
                    .unwrap(),
                ""
            );
            assert!(!events
                .lock()
                .unwrap()
                .iter()
                .any(|event| matches!(event, CaptureEvent::Failed(_))));
        }
    }
}
