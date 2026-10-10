//! Default capture adapter for the recording session's `AudioSource` seam:
//! the default microphone through cpal plus system audio through WASAPI
//! loopback ([`loopback`]), optionally limited to a meeting's process tree.
//!
//! `cpal::Stream` is `!Send` on Windows, so both capture paths are owned by a
//! dedicated OS thread driven by commands. Buffers are delivered with
//! `try_send`; when the session's queue is full the dropped sample count is
//! recorded in the shared pressure counters instead of blocking the device.

pub(crate) mod loopback;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as sync_mpsc;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use tokio::sync::mpsc;

use crate::recording_session::{AudioBuffer, AudioPressureCounters, AudioSource, AudioStream};

/// Commands sent to the dedicated audio OS thread.
enum AudioCommand {
    Start {
        result_tx: tokio::sync::oneshot::Sender<Result<StreamRates, String>>,
        audio_tx: mpsc::Sender<AudioBuffer>,
        pressure: Arc<AudioPressureCounters>,
        target_process_id: Option<u32>,
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
                    Ok(AudioCommand::Start {
                        result_tx,
                        audio_tx,
                        pressure,
                        target_process_id,
                    }) => {
                        // Tear down any previous session first.
                        _mic_stream = None;
                        if let Some(cancel) = loopback_cancel.take() {
                            cancel.store(true, Ordering::Relaxed);
                        }
                        if let Some(handle) = _loopback_join.take() {
                            let _ = handle.join();
                        }

                        let mic_result =
                            build_mic_stream(&host, audio_tx.clone(), Arc::clone(&pressure));
                        match mic_result {
                            Ok((stream, mic_config)) => {
                                if let Err(e) = stream.play() {
                                    let _ = result_tx
                                        .send(Err(format!("failed to play mic stream: {e}")));
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
                                        target_process_id,
                                        pressure,
                                    );
                                }));
                                loopback_cancel = Some(cancel);

                                let loopback_rate =
                                    match started_rx.recv_timeout(Duration::from_millis(500)) {
                                        Ok(Ok(started)) => {
                                            eprintln!(
                                                "[recording] loopback source: {}",
                                                if started.process_tree {
                                                    "meeting process tree"
                                                } else {
                                                    "default output"
                                                }
                                            );
                                            started.sample_rate
                                        }
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
        pressure: Arc<AudioPressureCounters>,
        target_process_id: Option<u32>,
    ) -> Result<StreamRates, String> {
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(AudioCommand::Start {
                result_tx,
                audio_tx,
                pressure,
                target_process_id,
            })
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
    pressure: Arc<AudioPressureCounters>,
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
        cpal::SampleFormat::F32 => build_input_stream::<f32>(
            &device,
            &stream_config,
            channels,
            audio_tx,
            pressure,
            err_fn,
        )?,
        cpal::SampleFormat::I16 => build_input_stream::<i16>(
            &device,
            &stream_config,
            channels,
            audio_tx,
            pressure,
            err_fn,
        )?,
        cpal::SampleFormat::U16 => build_input_stream::<u16>(
            &device,
            &stream_config,
            channels,
            audio_tx,
            pressure,
            err_fn,
        )?,
        fmt => return Err(format!("unsupported sample format: {fmt}")),
    };

    Ok((stream, stream_config))
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    audio_tx: mpsc::Sender<AudioBuffer>,
    pressure: Arc<AudioPressureCounters>,
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
                    let sample_count = mono.len();
                    if matches!(
                        audio_tx.try_send(AudioBuffer::Mic(mono)),
                        Err(mpsc::error::TrySendError::Full(_))
                    ) {
                        pressure.add_mic(sample_count);
                    }
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
    target_process_id: Option<u32>,
    pressure: Arc<AudioPressureCounters>,
}

impl DefaultAudioSource {
    pub fn new() -> Self {
        Self::for_process(None)
    }

    pub fn for_process(target_process_id: Option<u32>) -> Self {
        Self {
            audio_thread: AudioThread::spawn(),
            target_process_id,
            pressure: Arc::new(AudioPressureCounters::default()),
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
        let rates = self
            .audio_thread
            .start(tx, Arc::clone(&self.pressure), self.target_process_id)
            .await?;
        Ok(AudioStream {
            mic_sample_rate: rates.mic_sample_rate,
            loopback_sample_rate: rates.loopback_sample_rate,
            audio_rx: rx,
            pressure: Arc::clone(&self.pressure),
        })
    }

    fn stop(&self) {
        let _ = self.audio_thread.stop();
    }
}
