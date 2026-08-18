use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as sync_mpsc;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::asr::AsrEngine;
use crate::loopback;
use crate::resampler::ChannelResampler;

/// Audio channel label as it appears in the live transcript.
#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptChannel {
    You,
    Others,
}

/// Tagged audio buffer flowing from a capture source into the async session.
/// After T4, payloads are 16 kHz mono f32.
#[allow(dead_code)]
#[derive(Debug)]
pub enum AudioBuffer {
    Mic(Vec<f32>),
    Loopback(Vec<f32>),
}

/// Payload emitted on the `transcript:event` channel while a recording is active.
#[derive(Clone, serde::Serialize)]
pub struct TranscriptEvent {
    pub channel: TranscriptChannel,
    pub text: String,
    pub is_partial: bool,
}

/// Sample rates returned when an audio source starts both capture paths.
#[derive(Debug)]
pub struct AudioStream {
    pub mic_sample_rate: u32,
    pub loopback_sample_rate: u32,
    /// Receives audio buffers produced by the source. The source owns the
    /// corresponding sender; stopping the source closes this receiver.
    pub audio_rx: mpsc::Receiver<AudioBuffer>,
}

/// Abstraction over a mic + loopback audio capture source.
#[async_trait]
pub trait AudioSource: Send + Sync {
    async fn start(&self) -> Result<AudioStream, String>;
    fn stop(&self);
}

/// Sink for transcript events produced by a recording session.
pub trait TranscriptSink: Send + Sync {
    fn emit(&self, channel: TranscriptChannel, text: &str, is_partial: bool);
}

/// Fixed mic boost applied before ASR; see the comment in the consumer loop.
const MIC_GAIN: f32 = 4.0;

/// Commands sent to the dedicated audio OS thread.
enum AudioCommand {
    Start {
        result_tx: tokio::sync::oneshot::Sender<Result<StreamRates, String>>,
        audio_tx: mpsc::Sender<AudioBuffer>,
    },
    Stop,
}

/// Sample rates returned when the audio thread starts both capture paths.
struct StreamRates {
    mic_sample_rate: u32,
    loopback_sample_rate: u32,
}

/// A handle to the dedicated audio thread that owns the `cpal::Stream`.
///
/// `cpal::Stream` is `!Send` on Windows, so the mic stream itself must live on a
/// single OS thread. The WASAPI loopback capture also runs on this thread.
/// This handle is `Send` and can be stored in Tauri state.
struct AudioThread {
    cmd_tx: sync_mpsc::Sender<AudioCommand>,
}

impl AudioThread {
    fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = sync_mpsc::channel::<AudioCommand>();

        std::thread::spawn(move || {
            let host = cpal::default_host();
            let mut _mic_stream: Option<cpal::Stream> = None;
            let mut _loopback_join: Option<std::thread::JoinHandle<()>> = None;
            let mut loopback_cancel: Option<Arc<AtomicBool>> = None;

            loop {
                match cmd_rx.recv() {
                    Ok(AudioCommand::Start { result_tx, audio_tx }) => {
                        // Tear down any previous session first.
                        _mic_stream = None;
                        if let Some(cancel) = loopback_cancel.take() {
                            cancel.store(true, Ordering::Relaxed);
                        }
                        if let Some(handle) = _loopback_join.take() {
                            let _ = handle.join();
                        }

                        let mic_result = build_mic_stream(&host, audio_tx.clone());
                        match mic_result {
                            Ok((stream, mic_config)) => {
                                if let Err(e) = stream.play() {
                                    let _ = result_tx.send(Err(format!(
                                        "failed to play mic stream: {e}"
                                    )));
                                    continue;
                                }
                                _mic_stream = Some(stream);

                                let cancel = Arc::new(AtomicBool::new(false));
                                let (started_tx, started_rx) = sync_mpsc::channel();
                                let audio_tx_loopback = audio_tx.clone();
                                let cancel_clone = Arc::clone(&cancel);

                                _loopback_join = Some(std::thread::spawn(move || {
                                    loopback::capture_loopback(
                                        audio_tx_loopback,
                                        cancel_clone,
                                        started_tx,
                                    );
                                }));
                                loopback_cancel = Some(cancel);

                                let loopback_rate = match started_rx
                                    .recv_timeout(Duration::from_millis(500))
                                {
                                    Ok(Ok(rate)) => rate,
                                    Ok(Err(e)) => {
                                        eprintln!("loopback failed to start: {e}");
                                        0
                                    }
                                    Err(_) => {
                                        eprintln!("loopback startup timed out");
                                        0
                                    }
                                };

                                let _ = result_tx.send(Ok(StreamRates {
                                    mic_sample_rate: mic_config.sample_rate.0,
                                    loopback_sample_rate: loopback_rate,
                                }));
                            }
                            Err(e) => {
                                let _ = result_tx.send(Err(e));
                            }
                        }
                    }
                    Ok(AudioCommand::Stop) => {
                        _mic_stream = None;
                        if let Some(cancel) = loopback_cancel.take() {
                            cancel.store(true, Ordering::Relaxed);
                        }
                        if let Some(handle) = _loopback_join.take() {
                            let _ = handle.join();
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Self { cmd_tx }
    }

    async fn start(
        &self,
        audio_tx: mpsc::Sender<AudioBuffer>,
    ) -> Result<StreamRates, String> {
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(AudioCommand::Start { result_tx, audio_tx })
            .map_err(|_| "audio thread disconnected".to_string())?;
        result_rx
            .await
            .map_err(|_| "audio thread dropped result".to_string())?
    }

    fn stop(&self) -> Result<(), String> {
        self.cmd_tx
            .send(AudioCommand::Stop)
            .map_err(|_| "audio thread disconnected".to_string())?;
        Ok(())
    }
}

fn build_mic_stream(
    host: &cpal::Host,
    audio_tx: mpsc::Sender<AudioBuffer>,
) -> Result<(cpal::Stream, cpal::StreamConfig), String> {
    let device = host
        .default_input_device()
        .ok_or_else(|| "no default microphone found".to_string())?;

    let config = device
        .default_input_config()
        .map_err(|e| format!("failed to get mic config: {e}"))?;

    let stream_config: cpal::StreamConfig = config.config();
    let channels = stream_config.channels as usize;
    eprintln!(
        "[recording] mic device: {:?}, {} Hz, {} ch, {:?}",
        device.name().unwrap_or_else(|_| "<unknown>".into()),
        stream_config.sample_rate.0,
        stream_config.channels,
        config.sample_format()
    );
    let err_fn = |err| eprintln!("mic stream error: {err}");

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            build_input_stream::<f32>(&device, &stream_config, channels, audio_tx, err_fn)?
        }
        cpal::SampleFormat::I16 => {
            build_input_stream::<i16>(&device, &stream_config, channels, audio_tx, err_fn)?
        }
        cpal::SampleFormat::U16 => {
            build_input_stream::<u16>(&device, &stream_config, channels, audio_tx, err_fn)?
        }
        fmt => return Err(format!("unsupported sample format: {fmt}")),
    };

    Ok((stream, stream_config))
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    audio_tx: mpsc::Sender<AudioBuffer>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mono: Vec<f32> = data
                    .chunks(channels.max(1))
                    .map(|chunk| {
                        let sum: f32 = chunk.iter().map(|&s| f32::from_sample(s)).sum();
                        sum / channels.max(1) as f32
                    })
                    .collect();

                if !mono.is_empty() {
                    // TODO(T5+): count dropped samples instead of silently losing them.
                    let _ = audio_tx.try_send(AudioBuffer::Mic(mono));
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("failed to build mic stream: {e}"))
}

/// Default system audio source: cpal microphone + WASAPI loopback capture.
pub struct DefaultAudioSource {
    audio_thread: AudioThread,
}

impl DefaultAudioSource {
    pub fn new() -> Self {
        Self {
            audio_thread: AudioThread::spawn(),
        }
    }
}

impl Default for DefaultAudioSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AudioSource for DefaultAudioSource {
    async fn start(&self) -> Result<AudioStream, String> {
        let (tx, rx) = mpsc::channel::<AudioBuffer>(256);
        let rates = self.audio_thread.start(tx).await?;
        Ok(AudioStream {
            mic_sample_rate: rates.mic_sample_rate,
            loopback_sample_rate: rates.loopback_sample_rate,
            audio_rx: rx,
        })
    }

    fn stop(&self) {
        let _ = self.audio_thread.stop();
    }
}

/// Explicit lifecycle states for a recording session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Starting,
    Recording,
    Paused,
    Stopping,
}

/// Owns the recording lifecycle, audio processing, and transcript emission.
pub struct RecordingSession {
    audio_source: Arc<dyn AudioSource>,
    asr_engine: Option<Arc<AsrEngine>>,
    sink: Arc<dyn TranscriptSink>,
    state: Mutex<SessionState>,
    cancel: Arc<AtomicBool>,
    internal_tx: Mutex<Option<mpsc::Sender<AudioBuffer>>>,
    consumer_handle: Mutex<Option<JoinHandle<()>>>,
}

impl RecordingSession {
    pub fn new(
        audio_source: Arc<dyn AudioSource>,
        asr_engine: Option<Arc<AsrEngine>>,
        sink: Arc<dyn TranscriptSink>,
    ) -> Self {
        Self {
            audio_source,
            asr_engine,
            sink,
            state: Mutex::new(SessionState::Idle),
            cancel: Arc::new(AtomicBool::new(false)),
            internal_tx: Mutex::new(None),
            consumer_handle: Mutex::new(None),
        }
    }

    #[allow(dead_code)]
    pub async fn state(&self) -> SessionState {
        *self.state.lock().await
    }

    pub async fn start(&self) -> Result<(), String> {
        {
            let mut state = self.state.lock().await;
            if *state != SessionState::Idle {
                return Err("recording already in progress".into());
            }
            *state = SessionState::Starting;
        }

        let (internal_tx, internal_rx) = mpsc::channel::<AudioBuffer>(256);
        *self.internal_tx.lock().await = Some(internal_tx.clone());

        let stream = match self.audio_source.start().await {
            Ok(s) => s,
            Err(e) => {
                *self.state.lock().await = SessionState::Idle;
                return Err(e);
            }
        };

        // Forward buffers from the source's ephemeral channel into the persistent
        // internal channel so pause/resume can swap sources without restarting the
        // consumer task or ASR lanes.
        let forwarder_rx = stream.audio_rx;
        tokio::spawn(run_forwarder(forwarder_rx, internal_tx));

        let engine = self.asr_engine.clone();
        let sink = Arc::clone(&self.sink);
        let cancel = Arc::clone(&self.cancel);

        let handle = tokio::spawn(run_consumer(
            internal_rx,
            engine,
            sink,
            cancel,
            stream.mic_sample_rate,
            stream.loopback_sample_rate,
        ));
        *self.consumer_handle.lock().await = Some(handle);

        *self.state.lock().await = SessionState::Recording;
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), String> {
        {
            let mut state = self.state.lock().await;
            if *state != SessionState::Recording && *state != SessionState::Paused {
                return Err("no recording in progress".into());
            }
            *state = SessionState::Stopping;
        }

        self.cancel.store(true, Ordering::Relaxed);
        // Drop the keepalive sender so the consumer channel closes and the task
        // can finish cleanly after flushing trailing ASR text.
        let _ = self.internal_tx.lock().await.take();
        self.audio_source.stop();

        let mut handle = self.consumer_handle.lock().await;
        if let Some(mut h) = handle.take() {
            tokio::time::timeout(Duration::from_millis(500), &mut h)
                .await
                .ok();
            if !h.is_finished() {
                h.abort();
            }
        }

        *self.state.lock().await = SessionState::Idle;
        Ok(())
    }

    pub async fn pause(&self) -> Result<(), String> {
        {
            let mut state = self.state.lock().await;
            if *state != SessionState::Recording {
                return Err("no recording in progress".into());
            }
            *state = SessionState::Paused;
        }

        // Tearing down the streams drops their source-channel senders; the
        // keepalive internal_tx keeps the consumer task alive.
        self.audio_source.stop();
        Ok(())
    }

    pub async fn resume(&self) -> Result<(), String> {
        {
            let mut state = self.state.lock().await;
            if *state != SessionState::Paused {
                return Err("recording is not paused".into());
            }
            *state = SessionState::Starting;
        }

        let internal_tx = self
            .internal_tx
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| "recording has not been started".to_string())?
            .clone();

        let stream = match self.audio_source.start().await {
            Ok(s) => s,
            Err(e) => {
                *self.state.lock().await = SessionState::Paused;
                return Err(e);
            }
        };

        let forwarder_rx = stream.audio_rx;
        tokio::spawn(run_forwarder(forwarder_rx, internal_tx));

        *self.state.lock().await = SessionState::Recording;
        Ok(())
    }
}

async fn run_forwarder(
    mut source_rx: mpsc::Receiver<AudioBuffer>,
    internal_tx: mpsc::Sender<AudioBuffer>,
) {
    while let Some(buf) = source_rx.recv().await {
        // Backpressure from ASR must pause forwarding, not terminate the
        // forwarder. A full queue is temporary; only a closed consumer channel
        // means the recording session is actually finished.
        if internal_tx.send(buf).await.is_err() {
            break;
        }
    }
}

async fn run_consumer(
    mut audio_rx: mpsc::Receiver<AudioBuffer>,
    engine: Option<Arc<AsrEngine>>,
    sink: Arc<dyn TranscriptSink>,
    cancel: Arc<AtomicBool>,
    mic_rate: u32,
    loopback_rate: u32,
) {
    let mut mic_asr = engine.as_ref().map(|e| e.lane());
    let mut loopback_asr = engine.as_ref().map(|e| e.lane());

    let mut mic_resampler = ChannelResampler::new(mic_rate.max(1)).ok();
    let mut loopback_resampler = (loopback_rate > 0)
        .then(|| ChannelResampler::new(loopback_rate))
        .transpose()
        .unwrap_or(None);

    let started = std::time::Instant::now();
    let mut first_mic_audio_logged = false;
    let mut first_mic_text_logged = false;

    loop {
        tokio::select! {
            buf = audio_rx.recv() => {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                match buf {
                    Some(AudioBuffer::Mic(samples)) => {
                        if let Some(r) = mic_resampler.as_mut() {
                            r.push(&samples);
                            let mut resampled = r.drain_output();
                            // The mic array's signal sits around -25 dBFS on the test
                            // hardware, too quiet for the small ASR model to lock on.
                            // Fixed boost (measured: x4 restores full recognition);
                            // a proper AGC is follow-up work.
                            for s in resampled.iter_mut() {
                                *s = (*s * MIC_GAIN).clamp(-1.0, 1.0);
                            }
                            if !resampled.is_empty() {
                                if !first_mic_audio_logged {
                                    first_mic_audio_logged = true;
                                    eprintln!("[recording] first mic audio at +{:?}", started.elapsed());
                                }
                                if let Some(lane) = mic_asr.as_mut() {
                                    lane.feed(&resampled);
                                }
                            }
                        }
                    }
                    Some(AudioBuffer::Loopback(samples)) => {
                        if let Some(r) = loopback_resampler.as_mut() {
                            r.push(&samples);
                            let resampled = r.drain_output();
                            if !resampled.is_empty() {
                                if let Some(lane) = loopback_asr.as_mut() {
                                    lane.feed(&resampled);
                                }
                            }
                        }
                    }
                    None => break,
                }

                // Batch-decode any lane that buffered a full chunk (one
                // encoder pass for both lanes), then emit what changed.
                if let Some(engine) = engine.as_ref() {
                    let mut lanes: Vec<_> = mic_asr
                        .as_mut()
                        .into_iter()
                        .chain(loopback_asr.as_mut())
                        .collect();
                    engine.decode_ready(&mut lanes);
                }
                if let Some(lane) = mic_asr.as_mut() {
                    if let Some((text, is_final)) = lane.take_result() {
                        if !first_mic_text_logged {
                            first_mic_text_logged = true;
                            eprintln!("[recording] first mic text at +{:?}", started.elapsed());
                        }
                        sink.emit(TranscriptChannel::You, &text, !is_final);
                    }
                }
                if let Some(lane) = loopback_asr.as_mut() {
                    if let Some((text, is_final)) = lane.take_result() {
                        sink.emit(TranscriptChannel::Others, &text, !is_final);
                    }
                }
            }
        }
    }

    // Flush any trailing ASR text when the session ends.
    if let Some(asr) = mic_asr.as_mut() {
        if let Some((text, _)) = asr.finish() {
            sink.emit(TranscriptChannel::You, &text, false);
        }
    }
    if let Some(asr) = loopback_asr.as_mut() {
        if let Some((text, _)) = asr.finish() {
            sink.emit(TranscriptChannel::Others, &text, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct FakeAudioSource {
        samples: Vec<f32>,
        sample_rate: u32,
        cancel: Arc<AtomicBool>,
    }

    #[async_trait]
    impl AudioSource for FakeAudioSource {
        async fn start(&self) -> Result<AudioStream, String> {
            let (tx, rx) = mpsc::channel::<AudioBuffer>(256);
            let samples = self.samples.clone();
            let cancel = Arc::clone(&self.cancel);
            tokio::spawn(async move {
                for chunk in samples.chunks(1600) {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if tx.try_send(AudioBuffer::Mic(chunk.to_vec())).is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            });
            Ok(AudioStream {
                mic_sample_rate: self.sample_rate,
                loopback_sample_rate: 0,
                audio_rx: rx,
            })
        }

        fn stop(&self) {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    struct FakeSink {
        events: std::sync::Mutex<Vec<(TranscriptChannel, String, bool)>>,
    }

    impl TranscriptSink for FakeSink {
        fn emit(&self, channel: TranscriptChannel, text: &str, is_partial: bool) {
            self.events
                .lock()
                .unwrap()
                .push((channel, text.to_string(), is_partial));
        }
    }

    fn fake_source(samples: Vec<f32>, sample_rate: u32) -> Arc<FakeAudioSource> {
        Arc::new(FakeAudioSource {
            samples,
            sample_rate,
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    fn fake_sink() -> Arc<FakeSink> {
        Arc::new(FakeSink {
            events: std::sync::Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn audio_forwarder_survives_consumer_backpressure() {
        let (source_tx, source_rx) = mpsc::channel(2);
        let (internal_tx, mut internal_rx) = mpsc::channel(1);

        source_tx
            .send(AudioBuffer::Mic(vec![1.0]))
            .await
            .unwrap();
        source_tx
            .send(AudioBuffer::Mic(vec![2.0]))
            .await
            .unwrap();

        let forwarder = tokio::spawn(run_forwarder(source_rx, internal_tx));

        let first = internal_rx.recv().await.expect("first buffer should arrive");
        assert!(matches!(first, AudioBuffer::Mic(samples) if samples == vec![1.0]));

        let second = tokio::time::timeout(Duration::from_millis(500), internal_rx.recv())
            .await
            .expect("forwarder should wait for capacity instead of exiting")
            .expect("second buffer should arrive after capacity is freed");
        assert!(matches!(second, AudioBuffer::Mic(samples) if samples == vec![2.0]));

        drop(source_tx);
        forwarder.await.unwrap();
    }

    #[tokio::test]
    async fn lifecycle_without_asr() {
        let source = fake_source(vec![0.0f32; 16000], 16000);
        let sink = fake_sink();
        let session = RecordingSession::new(source, None, sink.clone());

        assert_eq!(session.state().await, SessionState::Idle);

        session.start().await.unwrap();
        assert_eq!(session.state().await, SessionState::Recording);

        session.pause().await.unwrap();
        assert_eq!(session.state().await, SessionState::Paused);

        session.resume().await.unwrap();
        assert_eq!(session.state().await, SessionState::Recording);

        session.stop().await.unwrap();
        assert_eq!(session.state().await, SessionState::Idle);

        assert!(sink.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn start_rejects_when_not_idle() {
        let source = fake_source(vec![0.0f32; 16000], 16000);
        let sink = fake_sink();
        let session = RecordingSession::new(source, None, sink.clone());

        session.start().await.unwrap();
        let err = session.start().await.unwrap_err();
        assert!(err.contains("already in progress"));
    }

    #[tokio::test]
    async fn stop_rejects_when_idle() {
        let source = fake_source(vec![0.0f32; 16000], 16000);
        let sink = fake_sink();
        let session = RecordingSession::new(source, None, sink.clone());

        let err = session.stop().await.unwrap_err();
        assert!(err.contains("no recording in progress"));
    }

    #[tokio::test]
    async fn pause_resume_reject_in_wrong_state() {
        let source = fake_source(vec![0.0f32; 16000], 16000);
        let sink = fake_sink();
        let session = RecordingSession::new(source, None, sink.clone());

        let err = session.pause().await.unwrap_err();
        assert!(err.contains("no recording in progress"));

        let err = session.resume().await.unwrap_err();
        assert!(err.contains("not paused"));
    }

    #[tokio::test]
    async fn session_emits_transcript_events_with_asr() {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(".scratch")
            .join("speech-test.wav");
        if !wav_path.exists() {
            eprintln!("speech-test.wav not found; skipping");
            return;
        }

        let engine = crate::asr::resolve_asr_model_dir()
            .and_then(|d| crate::asr::AsrEngine::new(&d))
            .map(Arc::new);
        let Some(engine) = engine else {
            eprintln!("ASR model not found; skipping");
            return;
        };

        let samples = read_wav_pcm16_mono(&wav_path);
        let source = fake_source(samples, 16000);
        let sink = fake_sink();
        let session = RecordingSession::new(source, Some(engine), sink.clone());

        session.start().await.unwrap();

        let mut found = false;
        for _ in 0..200 {
            if !sink.events.lock().unwrap().is_empty() {
                found = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        session.stop().await.unwrap();

        assert!(found, "expected transcript events");
    }

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
