//! The capture pipeline for one Capture session.
//!
//! [`RecordingSession`] owns the lifecycle (start / pause / resume / stop) and
//! turns raw audio into Live transcript events. It talks to the outside world
//! through three seams, each with a production and a test adapter:
//! - [`AudioSource`]: Mic + System audio capture (`crate::audio::DefaultAudioSource`).
//! - [`Transcriber`]: streaming speech-to-text (`crate::asr::SherpaTranscriber`).
//! - [`TranscriptSink`]: where events go (the Tauri emitter in `recording`).
//!
//! Pipeline guarantees:
//! - Mic audio is labelled "You" and System audio "Others"; both are resampled
//!   to 16 kHz mono and the Mic gets a fixed gain before recognition.
//! - Each utterance has one id shared across its revisions; revisions count up
//!   from 1 and the last one of a finished utterance is not partial. Ids are
//!   unique across both channels within a session.
//! - Backpressure from recognition pauses forwarding but never loses or stops
//!   the stream; samples the capture device could not queue are reported as
//!   audio pressure (immediately, then at most every five seconds, and once
//!   more at the end if the totals changed).
//! - `stop` drains every captured buffer and flushes both channels before
//!   returning (bounded by a five-second finalization timeout).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::resampler::ChannelResampler;

/// Audio channel label as it appears in the live transcript.
#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptChannel {
    You,
    Others,
}

/// Tagged audio buffer flowing from a capture source into the session, in
/// the source's native sample rate (mono f32).
#[derive(Debug)]
pub enum AudioBuffer {
    Mic(Vec<f32>),
    Loopback(Vec<f32>),
}

/// Payload emitted on the `transcript:event` channel while a recording is active.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct TranscriptEvent {
    pub utterance_id: u64,
    pub revision: u32,
    pub channel: TranscriptChannel,
    pub text: String,
    pub is_partial: bool,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize, PartialEq, Eq)]
pub struct AudioPressureEvent {
    pub mic_dropped_samples: u64,
    pub loopback_dropped_samples: u64,
}

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
pub struct RecordingStartStatus {
    pub meeting_audio_available: bool,
    pub transcription_available: bool,
}

/// Running totals of samples a capture source had to drop.
#[derive(Debug, Default)]
pub(crate) struct AudioPressureCounters {
    mic_dropped_samples: AtomicU64,
    loopback_dropped_samples: AtomicU64,
}

impl AudioPressureCounters {
    pub(crate) fn add_mic(&self, samples: usize) {
        self.mic_dropped_samples
            .fetch_add(samples as u64, Ordering::Relaxed);
    }

    pub(crate) fn add_loopback(&self, samples: usize) {
        self.loopback_dropped_samples
            .fetch_add(samples as u64, Ordering::Relaxed);
    }

    fn snapshot(&self) -> AudioPressureEvent {
        AudioPressureEvent {
            mic_dropped_samples: self.mic_dropped_samples.load(Ordering::Relaxed),
            loopback_dropped_samples: self.loopback_dropped_samples.load(Ordering::Relaxed),
        }
    }
}

/// Sample rates returned when an audio source starts both capture paths.
#[derive(Debug)]
pub struct AudioStream {
    pub mic_sample_rate: u32,
    /// `0` when System audio could not be captured.
    pub loopback_sample_rate: u32,
    /// Receives audio buffers produced by the source. The source owns the
    /// corresponding sender; stopping the source closes this receiver.
    pub audio_rx: mpsc::Receiver<AudioBuffer>,
    pub(crate) pressure: Arc<AudioPressureCounters>,
}

/// Abstraction over a mic + loopback audio capture source.
#[async_trait]
pub trait AudioSource: Send + Sync {
    async fn start(&self) -> Result<AudioStream, String>;
    fn stop(&self);
}

/// Sink for transcript events produced by a recording session.
pub trait TranscriptSink: Send + Sync {
    fn emit(&self, event: TranscriptEvent);

    fn emit_audio_pressure(&self, _event: AudioPressureEvent) {}
}

/// The newest text a recognizer has for a channel's current utterance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recognition {
    pub channel: TranscriptChannel,
    pub text: String,
    /// The utterance ended (endpoint detected or input finished).
    pub is_final: bool,
}

/// A streaming speech-to-text engine shared by every recording.
pub trait Transcriber: Send + Sync {
    /// Fresh recognition state for one Capture session, covering both channels.
    fn start_session(&self) -> Box<dyn TranscriberSession>;
}

/// Recognition for one Capture session. Audio is 16 kHz mono f32.
pub trait TranscriberSession: Send {
    /// Buffer audio for `channel` without necessarily decoding it yet.
    fn feed(&mut self, channel: TranscriptChannel, samples: &[f32]);
    /// Decode whatever is ready and return each channel's result if it changed
    /// since the last call ("You" before "Others").
    fn poll(&mut self) -> Vec<Recognition>;
    /// End of input for `channel`: flush it and return its final result.
    /// Callers finish one channel at a time and emit before finishing the
    /// next, so a slow channel cannot hold back another's finished text.
    fn finish(&mut self, channel: TranscriptChannel) -> Option<Recognition>;
}

/// Fixed mic boost applied before ASR; see the comment in the consumer loop.
const MIC_GAIN: f32 = 4.0;
const AUDIO_PRESSURE_REPORT_INTERVAL: Duration = Duration::from_secs(5);

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
    transcriber: Option<Arc<dyn Transcriber>>,
    sink: Arc<dyn TranscriptSink>,
    state: Mutex<SessionState>,
    internal_tx: Mutex<Option<mpsc::Sender<AudioBuffer>>>,
    consumer_handle: Mutex<Option<JoinHandle<()>>>,
}

impl RecordingSession {
    /// `transcriber` is `None` when no Model pack is installed: audio is still
    /// captured but no transcript text is produced.
    pub fn new(
        audio_source: Arc<dyn AudioSource>,
        transcriber: Option<Arc<dyn Transcriber>>,
        sink: Arc<dyn TranscriptSink>,
    ) -> Self {
        Self {
            audio_source,
            transcriber,
            sink,
            state: Mutex::new(SessionState::Idle),
            internal_tx: Mutex::new(None),
            consumer_handle: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub async fn state(&self) -> SessionState {
        *self.state.lock().await
    }

    pub async fn start(&self) -> Result<RecordingStartStatus, String> {
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
        let start_status = RecordingStartStatus {
            meeting_audio_available: stream.loopback_sample_rate > 0,
            transcription_available: self.transcriber.is_some(),
        };

        // Forward buffers from the source's ephemeral channel into the persistent
        // internal channel so pause/resume can swap sources without restarting the
        // consumer task or ASR lanes.
        let pressure = Arc::clone(&stream.pressure);
        let forwarder_rx = stream.audio_rx;
        tokio::spawn(run_forwarder(forwarder_rx, internal_tx));

        let transcription = self.transcriber.as_ref().map(|t| t.start_session());
        let sink = Arc::clone(&self.sink);
        let handle = tokio::spawn(run_consumer(
            internal_rx,
            transcription,
            sink,
            stream.mic_sample_rate,
            stream.loopback_sample_rate,
            pressure,
        ));
        *self.consumer_handle.lock().await = Some(handle);

        *self.state.lock().await = SessionState::Recording;
        Ok(start_status)
    }

    pub async fn stop(&self) -> Result<(), String> {
        {
            let mut state = self.state.lock().await;
            if *state != SessionState::Recording && *state != SessionState::Paused {
                return Err("no recording in progress".into());
            }
            *state = SessionState::Stopping;
        }

        // Stop the producers, then drop the keepalive sender. Forwarders drain
        // their already-captured buffers before the internal channel closes, so
        // the consumer can decode everything and flush both ASR lanes.
        self.audio_source.stop();
        let _ = self.internal_tx.lock().await.take();

        let mut handle = self.consumer_handle.lock().await;
        if let Some(mut h) = handle.take() {
            match tokio::time::timeout(Duration::from_secs(5), &mut h).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    *self.state.lock().await = SessionState::Idle;
                    return Err(format!("transcript finalization task failed: {error}"));
                }
                Err(_) => {
                    h.abort();
                    *self.state.lock().await = SessionState::Idle;
                    return Err("timed out finalizing the transcript".into());
                }
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

    pub async fn resume(&self) -> Result<RecordingStartStatus, String> {
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
        let start_status = RecordingStartStatus {
            meeting_audio_available: stream.loopback_sample_rate > 0,
            transcription_available: self.transcriber.is_some(),
        };

        let forwarder_rx = stream.audio_rx;
        tokio::spawn(run_forwarder(forwarder_rx, internal_tx));

        *self.state.lock().await = SessionState::Recording;
        Ok(start_status)
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

/// Per-channel state of the consumer: resampling and utterance timing.
struct Lane {
    channel: TranscriptChannel,
    gain: f32,
    resampler: Option<ChannelResampler>,
    timeline: TranscriptLaneTimeline,
}

impl Lane {
    /// Resample (and boost) newly captured audio to 16 kHz mono.
    fn resample(&mut self, samples: &[f32]) -> Vec<f32> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Vec::new();
        };
        resampler.push(samples);
        let mut resampled = resampler.drain_output();
        if self.gain != 1.0 {
            for s in resampled.iter_mut() {
                *s = (*s * self.gain).clamp(-1.0, 1.0);
            }
        }
        self.timeline.add_samples(resampled.len());
        resampled
    }
}

async fn run_consumer(
    mut audio_rx: mpsc::Receiver<AudioBuffer>,
    mut transcription: Option<Box<dyn TranscriberSession>>,
    sink: Arc<dyn TranscriptSink>,
    mic_rate: u32,
    loopback_rate: u32,
    pressure: Arc<AudioPressureCounters>,
) {
    let mut mic = Lane {
        channel: TranscriptChannel::You,
        // The mic array's signal sits around -25 dBFS on the test hardware,
        // too quiet for the small ASR model to lock on. Fixed boost
        // (measured: x4 restores full recognition); a proper AGC is
        // follow-up work.
        gain: MIC_GAIN,
        resampler: ChannelResampler::new(mic_rate.max(1)).ok(),
        timeline: TranscriptLaneTimeline::default(),
    };
    let mut loopback = Lane {
        channel: TranscriptChannel::Others,
        gain: 1.0,
        resampler: (loopback_rate > 0)
            .then(|| ChannelResampler::new(loopback_rate))
            .transpose()
            .unwrap_or(None),
        timeline: TranscriptLaneTimeline::default(),
    };

    let started = std::time::Instant::now();
    let mut first_mic_audio_logged = false;
    let mut first_mic_text_logged = false;
    let mut next_utterance_id = 1u64;
    let mut pressure_reporter = AudioPressureReporter::default();

    let mut emit = |mic: &mut Lane, loopback: &mut Lane, recognition: Recognition| {
        let lane = match recognition.channel {
            TranscriptChannel::You => mic,
            TranscriptChannel::Others => loopback,
        };
        if let Some(event) = lane.timeline.event(
            lane.channel,
            &recognition.text,
            recognition.is_final,
            &mut next_utterance_id,
        ) {
            sink.emit(event);
        }
    };
    while let Some(buf) = audio_rx.recv().await {
        let (lane, samples) = match buf {
            AudioBuffer::Mic(samples) => (&mut mic, samples),
            AudioBuffer::Loopback(samples) => (&mut loopback, samples),
        };
        let resampled = lane.resample(&samples);
        if !resampled.is_empty() {
            if lane.channel == TranscriptChannel::You && !first_mic_audio_logged {
                first_mic_audio_logged = true;
                eprintln!("[recording] first mic audio at +{:?}", started.elapsed());
            }
            if let Some(transcription) = transcription.as_mut() {
                transcription.feed(lane.channel, &resampled);
            }
        }

        // Decode any channel that buffered a full chunk, then emit what changed.
        if let Some(transcription) = transcription.as_mut() {
            for recognition in transcription.poll() {
                if recognition.channel == TranscriptChannel::You && !first_mic_text_logged {
                    first_mic_text_logged = true;
                    eprintln!("[recording] first mic text at +{:?}", started.elapsed());
                }
                emit(&mut mic, &mut loopback, recognition);
            }
        }
        if let Some(event) = pressure_reporter.event(&pressure, Instant::now(), false) {
            sink.emit_audio_pressure(event);
        }
    }

    if let Some(event) = pressure_reporter.event(&pressure, Instant::now(), true) {
        sink.emit_audio_pressure(event);
    }

    // Flush any trailing ASR text when the session ends.
    if let Some(transcription) = transcription.as_mut() {
        for channel in [TranscriptChannel::You, TranscriptChannel::Others] {
            if let Some(recognition) = transcription.finish(channel) {
                emit(&mut mic, &mut loopback, recognition);
            }
        }
    }
}

#[derive(Default)]
struct AudioPressureReporter {
    last_reported: AudioPressureEvent,
    last_emit: Option<Instant>,
}

impl AudioPressureReporter {
    fn event(
        &mut self,
        counters: &AudioPressureCounters,
        now: Instant,
        force: bool,
    ) -> Option<AudioPressureEvent> {
        let current = counters.snapshot();
        if current == self.last_reported
            || (current.mic_dropped_samples == 0 && current.loopback_dropped_samples == 0)
        {
            return None;
        }
        if !force
            && self
                .last_emit
                .is_some_and(|last| now.duration_since(last) < AUDIO_PRESSURE_REPORT_INTERVAL)
        {
            return None;
        }

        self.last_reported = current.clone();
        self.last_emit = Some(now);
        Some(current)
    }
}

#[derive(Default)]
struct TranscriptLaneTimeline {
    samples_seen: u64,
    current_utterance_id: Option<u64>,
    current_start_ms: u64,
    last_final_end_ms: u64,
    revision: u32,
    last_text: String,
}

impl TranscriptLaneTimeline {
    fn add_samples(&mut self, count: usize) {
        self.samples_seen = self.samples_seen.saturating_add(count as u64);
    }

    fn event(
        &mut self,
        channel: TranscriptChannel,
        text: &str,
        is_final: bool,
        next_utterance_id: &mut u64,
    ) -> Option<TranscriptEvent> {
        let end_ms = self.samples_seen.saturating_mul(1_000) / 16_000;
        let trimmed = text.trim();
        let event_text = if trimmed.is_empty() && is_final && !self.last_text.is_empty() {
            self.last_text.clone()
        } else if trimmed.is_empty() {
            if is_final {
                self.reset_after_final(end_ms);
            }
            return None;
        } else {
            trimmed.to_string()
        };

        let utterance_id = match self.current_utterance_id {
            Some(id) => id,
            None => {
                let id = *next_utterance_id;
                *next_utterance_id = (*next_utterance_id).saturating_add(1);
                self.current_utterance_id = Some(id);
                // ASR emits its first words after some audio has accumulated.
                // Bound that unknown onset to the previous endpoint and a
                // conservative two-second lookback for cross-lane matching.
                self.current_start_ms = end_ms.saturating_sub(2_000).max(self.last_final_end_ms);
                self.revision = 0;
                id
            }
        };

        self.revision = self.revision.saturating_add(1);
        self.last_text = event_text.clone();
        let event = TranscriptEvent {
            utterance_id,
            revision: self.revision,
            channel,
            text: event_text,
            is_partial: !is_final,
            start_ms: self.current_start_ms,
            end_ms,
        };

        if is_final {
            self.reset_after_final(end_ms);
        }
        Some(event)
    }

    fn reset_after_final(&mut self, end_ms: u64) {
        self.current_utterance_id = None;
        self.current_start_ms = end_ms;
        self.last_final_end_ms = end_ms;
        self.revision = 0;
        self.last_text.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    /// Scripted capture source. Buffers are delivered losslessly (`send`), so
    /// any loss inside the session is visible to the test; `dropped` simulates
    /// samples the capture device itself could not queue.
    struct FakeAudioSource {
        buffers: Vec<(TranscriptChannel, Vec<f32>)>,
        mic_rate: u32,
        loopback_rate: u32,
        dropped: (usize, usize),
        cancel: Arc<AtomicBool>,
        delivered_all: Arc<AtomicBool>,
        pressure: Arc<AudioPressureCounters>,
    }

    impl FakeAudioSource {
        fn new(buffers: Vec<(TranscriptChannel, Vec<f32>)>, mic_rate: u32, loopback_rate: u32) -> Self {
            Self {
                buffers,
                mic_rate,
                loopback_rate,
                dropped: (0, 0),
                cancel: Arc::new(AtomicBool::new(false)),
                delivered_all: Arc::new(AtomicBool::new(false)),
                pressure: Arc::new(AudioPressureCounters::default()),
            }
        }

        fn mic_only(samples: Vec<f32>, rate: u32) -> Arc<Self> {
            let buffers = samples
                .chunks(1600)
                .map(|chunk| (TranscriptChannel::You, chunk.to_vec()))
                .collect();
            Arc::new(Self::new(buffers, rate, 0))
        }

        async fn wait_until_delivered(&self) {
            while !self.delivered_all.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }

    #[async_trait]
    impl AudioSource for FakeAudioSource {
        async fn start(&self) -> Result<AudioStream, String> {
            let (tx, rx) = mpsc::channel::<AudioBuffer>(256);
            self.pressure.add_mic(self.dropped.0);
            self.pressure.add_loopback(self.dropped.1);
            let buffers = self.buffers.clone();
            let cancel = Arc::clone(&self.cancel);
            let delivered_all = Arc::clone(&self.delivered_all);
            tokio::spawn(async move {
                for (channel, samples) in buffers {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let buffer = match channel {
                        TranscriptChannel::You => AudioBuffer::Mic(samples),
                        TranscriptChannel::Others => AudioBuffer::Loopback(samples),
                    };
                    if tx.send(buffer).await.is_err() {
                        return;
                    }
                }
                delivered_all.store(true, Ordering::Release);
            });
            Ok(AudioStream {
                mic_sample_rate: self.mic_rate,
                loopback_sample_rate: self.loopback_rate,
                audio_rx: rx,
                pressure: Arc::clone(&self.pressure),
            })
        }

        fn stop(&self) {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Scripted recognizer: each step fires once its channel has been fed at
    /// least `after_samples` samples. Records every sample it receives.
    #[derive(Default)]
    struct ScriptedTranscriber {
        steps: Vec<(TranscriptChannel, usize, &'static str, bool)>,
        finish: Vec<(TranscriptChannel, &'static str)>,
        /// While closed, `feed` blocks: simulates recognition falling behind.
        feed_gate: Option<Arc<AtomicBool>>,
        /// While closed, finishing "Others" blocks: simulates a slow final decode.
        others_finish_gate: Option<Arc<AtomicBool>>,
        fed: Arc<std::sync::Mutex<BTreeMap<&'static str, Vec<f32>>>>,
    }

    impl ScriptedTranscriber {
        fn fed(&self, channel: TranscriptChannel) -> Vec<f32> {
            self.fed
                .lock()
                .unwrap()
                .get(label(channel))
                .cloned()
                .unwrap_or_default()
        }
    }

    fn label(channel: TranscriptChannel) -> &'static str {
        match channel {
            TranscriptChannel::You => "you",
            TranscriptChannel::Others => "others",
        }
    }

    struct ScriptedSession {
        pending: VecDeque<(TranscriptChannel, usize, &'static str, bool)>,
        finish: Vec<(TranscriptChannel, &'static str)>,
        feed_gate: Option<Arc<AtomicBool>>,
        others_finish_gate: Option<Arc<AtomicBool>>,
        fed: Arc<std::sync::Mutex<BTreeMap<&'static str, Vec<f32>>>>,
    }

    impl Transcriber for ScriptedTranscriber {
        fn start_session(&self) -> Box<dyn TranscriberSession> {
            Box::new(ScriptedSession {
                pending: self.steps.iter().copied().collect(),
                finish: self.finish.clone(),
                feed_gate: self.feed_gate.clone(),
                others_finish_gate: self.others_finish_gate.clone(),
                fed: Arc::clone(&self.fed),
            })
        }
    }

    impl TranscriberSession for ScriptedSession {
        fn feed(&mut self, channel: TranscriptChannel, samples: &[f32]) {
            if let Some(gate) = &self.feed_gate {
                while !gate.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            self.fed
                .lock()
                .unwrap()
                .entry(label(channel))
                .or_default()
                .extend_from_slice(samples);
        }

        fn poll(&mut self) -> Vec<Recognition> {
            let mut out = Vec::new();
            for channel in [TranscriptChannel::You, TranscriptChannel::Others] {
                let fed = self
                    .fed
                    .lock()
                    .unwrap()
                    .get(label(channel))
                    .map_or(0, Vec::len);
                let ready = self
                    .pending
                    .iter()
                    .position(|step| step.0 == channel && step.1 <= fed);
                if let Some(index) = ready {
                    let (channel, _, text, is_final) = self.pending.remove(index).unwrap();
                    out.push(Recognition {
                        channel,
                        text: text.into(),
                        is_final,
                    });
                }
            }
            out
        }

        fn finish(&mut self, channel: TranscriptChannel) -> Option<Recognition> {
            if let (TranscriptChannel::Others, Some(gate)) = (channel, &self.others_finish_gate) {
                while !gate.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            self.finish
                .iter()
                .find(|&&(finished, _)| finished == channel)
                .map(|&(channel, text)| Recognition {
                    channel,
                    text: text.into(),
                    is_final: true,
                })
        }
    }

    #[derive(Default)]
    struct FakeSink {
        events: std::sync::Mutex<Vec<TranscriptEvent>>,
        pressure: std::sync::Mutex<Vec<AudioPressureEvent>>,
    }

    impl TranscriptSink for FakeSink {
        fn emit(&self, event: TranscriptEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn emit_audio_pressure(&self, event: AudioPressureEvent) {
            self.pressure.lock().unwrap().push(event);
        }
    }

    /// How many 16 kHz samples the session's resampler yields for these buffer sizes.
    fn resampled_len(rate: u32, buffers: &[usize]) -> usize {
        let mut resampler = ChannelResampler::new(rate).unwrap();
        buffers
            .iter()
            .map(|&len| {
                resampler.push(&vec![0.0; len]);
                resampler.drain_output().len()
            })
            .sum()
    }

    fn by_utterance(events: &[TranscriptEvent]) -> BTreeMap<u64, Vec<TranscriptEvent>> {
        let mut utterances: BTreeMap<u64, Vec<TranscriptEvent>> = BTreeMap::new();
        for event in events {
            utterances
                .entry(event.utterance_id)
                .or_default()
                .push(event.clone());
        }
        utterances
    }

    #[tokio::test]
    async fn pipeline_transcribes_both_channels_into_revisioned_utterances() {
        use TranscriptChannel::{Others, You};
        // Two seconds of each channel: Mic at 16 kHz, System audio at 48 kHz.
        let mut buffers = Vec::new();
        for _ in 0..32 {
            buffers.push((You, vec![0.5; 1024]));
            buffers.push((Others, vec![0.25; 3072]));
        }
        let source = Arc::new(FakeAudioSource::new(buffers, 16_000, 48_000));
        let transcriber = Arc::new(ScriptedTranscriber {
            steps: vec![
                (You, 4_096, "I can", false),
                (You, 8_192, "I can take it", true),
                (You, 16_384, "next", false),
                (Others, 4_000, "Go ahead", false),
                // An empty final keeps the last partial text.
                (Others, 9_000, "", true),
            ],
            finish: vec![(You, "next steps")],
            ..ScriptedTranscriber::default()
        });
        let sink = Arc::new(FakeSink::default());
        let session = RecordingSession::new(
            source.clone(),
            Some(transcriber.clone() as Arc<dyn Transcriber>),
            sink.clone(),
        );

        let started = session.start().await.unwrap();
        assert!(started.meeting_audio_available);
        assert!(started.transcription_available);
        source.wait_until_delivered().await;
        session.stop().await.unwrap();

        let events = sink.events.lock().unwrap().clone();
        let utterances = by_utterance(&events);
        type Revisions<'a> = Vec<(u32, &'a str, bool)>;
        let summary: Vec<(TranscriptChannel, Revisions)> = utterances
            .values()
            .map(|revisions| {
                (
                    revisions[0].channel,
                    revisions
                        .iter()
                        .map(|e| (e.revision, e.text.as_str(), e.is_partial))
                        .collect(),
                )
            })
            .collect();
        let you: Vec<_> = summary.iter().filter(|(c, _)| *c == You).map(|(_, r)| r.clone()).collect();
        let others: Vec<_> = summary.iter().filter(|(c, _)| *c == Others).map(|(_, r)| r.clone()).collect();
        assert_eq!(
            you,
            vec![
                vec![(1, "I can", true), (2, "I can take it", false)],
                vec![(1, "next", true), (2, "next steps", false)],
            ]
        );
        assert_eq!(others, vec![vec![(1, "Go ahead", true), (2, "Go ahead", false)]]);

        // Utterance ids are unique across channels and every revision of an
        // utterance stays on one channel with a stable start.
        assert_eq!(utterances.len(), 3);
        for revisions in utterances.values() {
            assert!(revisions.iter().all(|e| e.channel == revisions[0].channel));
            assert!(revisions.iter().all(|e| e.start_ms == revisions[0].start_ms));
            assert!(revisions.windows(2).all(|w| w[0].end_ms <= w[1].end_ms));
        }
        let second_you = &utterances.values().filter(|r| r[0].channel == You).nth(1).unwrap()[0];
        assert!(second_you.start_ms >= 512, "a new utterance starts at the previous endpoint");

        // Mic audio is boosted (and clipped); System audio is resampled to 16 kHz unboosted.
        let mic = transcriber.fed(You);
        let system = transcriber.fed(Others);
        assert_eq!(mic.len(), resampled_len(16_000, &[1024; 32]));
        assert!(mic.iter().all(|s| (-1.0..=1.0).contains(s)));
        assert!(mic.contains(&1.0), "boosted Mic audio clips at full scale");
        assert_eq!(system.len(), resampled_len(48_000, &[3072; 32]));
        assert!(system.iter().all(|s| *s <= 0.26));
        assert!(sink.pressure.lock().unwrap().is_empty(), "nothing was dropped");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pipeline_emits_mic_final_before_a_slow_system_finish() {
        use TranscriptChannel::{Others, You};
        let source = Arc::new(FakeAudioSource::new(
            vec![(You, vec![0.1; 1024]), (Others, vec![0.1; 1024])],
            16_000,
            16_000,
        ));
        // Finishing "Others" stalls until the Mic final reaches the sink. If the
        // Mic final were held behind it, stop() would hit its finalize timeout.
        let gate = Arc::new(AtomicBool::new(false));
        let transcriber = Arc::new(ScriptedTranscriber {
            finish: vec![(You, "last mic words"), (Others, "last system words")],
            others_finish_gate: Some(Arc::clone(&gate)),
            ..ScriptedTranscriber::default()
        });
        let sink = Arc::new(FakeSink::default());
        let session = RecordingSession::new(
            source.clone(),
            Some(transcriber as Arc<dyn Transcriber>),
            sink.clone(),
        );
        let watcher = {
            let sink = Arc::clone(&sink);
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                loop {
                    let mic_final = sink.events.lock().unwrap().iter().any(|e| {
                        e.channel == You && !e.is_partial && e.text == "last mic words"
                    });
                    if mic_final {
                        gate.store(true, Ordering::Release);
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
        };

        session.start().await.unwrap();
        source.wait_until_delivered().await;
        let stopped = session.stop().await;
        gate.store(true, Ordering::Release);
        watcher.abort();

        stopped.expect("the Mic final is emitted without waiting for the System finish");
        let events = sink.events.lock().unwrap().clone();
        assert!(events.iter().any(|e| e.channel == Others && e.text == "last system words"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pipeline_reports_dropped_audio_and_survives_recognition_backpressure() {
        let buffers = (0..600)
            .map(|_| (TranscriptChannel::You, vec![0.1; 1024]))
            .collect();
        let mut source = FakeAudioSource::new(buffers, 16_000, 0);
        source.dropped = (160, 80);
        let source = Arc::new(source);
        // Recognition stalls until both queues are full; forwarding must wait, not stop.
        let gate = Arc::new(AtomicBool::new(false));
        let transcriber = Arc::new(ScriptedTranscriber {
            feed_gate: Some(Arc::clone(&gate)),
            ..ScriptedTranscriber::default()
        });
        let sink = Arc::new(FakeSink::default());
        let session = RecordingSession::new(
            source.clone(),
            Some(transcriber.clone() as Arc<dyn Transcriber>),
            sink.clone(),
        );

        let started = session.start().await.unwrap();
        assert!(!started.meeting_audio_available);
        // 600 buffers exceed the source and session queues (256 each) combined.
        tokio::time::sleep(Duration::from_millis(200)).await;
        gate.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(10), source.wait_until_delivered())
            .await
            .expect("the source must be able to deliver every buffer after the stall");
        session.stop().await.unwrap();

        assert_eq!(
            transcriber.fed(TranscriptChannel::You).len(),
            resampled_len(16_000, &[1024; 600]),
            "every captured buffer reaches recognition"
        );
        assert_eq!(
            *sink.pressure.lock().unwrap(),
            vec![AudioPressureEvent {
                mic_dropped_samples: 160,
                loopback_dropped_samples: 80,
            }],
            "dropped audio is reported once, without duplicate final reports"
        );
    }

    #[test]
    fn audio_pressure_reports_immediately_then_throttles_updates() {
        let counters = AudioPressureCounters::default();
        let mut reporter = AudioPressureReporter::default();
        let started = Instant::now();

        counters.add_mic(160);
        assert_eq!(
            reporter.event(&counters, started, false),
            Some(AudioPressureEvent {
                mic_dropped_samples: 160,
                loopback_dropped_samples: 0,
            })
        );

        counters.add_mic(160);
        assert_eq!(
            reporter.event(&counters, started + Duration::from_secs(1), false),
            None
        );
        assert_eq!(
            reporter.event(&counters, started + Duration::from_secs(5), false),
            Some(AudioPressureEvent {
                mic_dropped_samples: 320,
                loopback_dropped_samples: 0,
            })
        );

        counters.add_loopback(80);
        assert_eq!(
            reporter.event(&counters, started + Duration::from_secs(6), true),
            Some(AudioPressureEvent {
                mic_dropped_samples: 320,
                loopback_dropped_samples: 80,
            })
        );
    }

    #[tokio::test]
    async fn lifecycle_without_asr() {
        let source = FakeAudioSource::mic_only(vec![0.0f32; 16000], 16000);
        let sink = Arc::new(FakeSink::default());
        let session = RecordingSession::new(source, None, sink.clone());

        assert_eq!(session.state().await, SessionState::Idle);

        let started = session.start().await.unwrap();
        assert!(!started.meeting_audio_available);
        assert!(!started.transcription_available);
        assert_eq!(session.state().await, SessionState::Recording);

        session.pause().await.unwrap();
        assert_eq!(session.state().await, SessionState::Paused);

        let resumed = session.resume().await.unwrap();
        assert!(!resumed.meeting_audio_available);
        assert!(!resumed.transcription_available);
        assert_eq!(session.state().await, SessionState::Recording);

        session.stop().await.unwrap();
        assert_eq!(session.state().await, SessionState::Idle);

        assert!(sink.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn start_rejects_when_not_idle() {
        let source = FakeAudioSource::mic_only(vec![0.0f32; 16000], 16000);
        let session = RecordingSession::new(source, None, Arc::new(FakeSink::default()));

        session.start().await.unwrap();
        let err = session.start().await.unwrap_err();
        assert!(err.contains("already in progress"));
    }

    #[tokio::test]
    async fn stop_rejects_when_idle() {
        let source = FakeAudioSource::mic_only(vec![0.0f32; 16000], 16000);
        let session = RecordingSession::new(source, None, Arc::new(FakeSink::default()));

        let err = session.stop().await.unwrap_err();
        assert!(err.contains("no recording in progress"));
    }

    #[tokio::test]
    async fn pause_resume_reject_in_wrong_state() {
        let source = FakeAudioSource::mic_only(vec![0.0f32; 16000], 16000);
        let session = RecordingSession::new(source, None, Arc::new(FakeSink::default()));

        let err = session.pause().await.unwrap_err();
        assert!(err.contains("no recording in progress"));

        let err = session.resume().await.unwrap_err();
        assert!(err.contains("not paused"));
    }

    #[tokio::test]
    #[ignore = "needs the installed ASR Model pack and .scratch/speech-test.wav; run with --ignored"]
    async fn model_pack_session_emits_transcript_events() {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(".scratch")
            .join("speech-test.wav");
        assert!(wav_path.exists(), "speech-test.wav not found at {}", wav_path.display());

        let engine = crate::asr::resolve_asr_model_dir()
            .and_then(|d| crate::asr::AsrEngine::new(&d))
            .expect("ASR Model pack must be installed");
        let transcriber: Arc<dyn Transcriber> =
            Arc::new(crate::asr::SherpaTranscriber::new(Arc::new(engine)));

        let samples = read_wav_pcm16_mono(&wav_path);
        let source = FakeAudioSource::mic_only(samples, 16000);
        let sink = Arc::new(FakeSink::default());
        let session = RecordingSession::new(source, Some(transcriber), sink.clone());

        let started = session.start().await.unwrap();
        assert!(started.transcription_available);

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
        assert!(
            &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
            "not a RIFF/WAVE file"
        );

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
