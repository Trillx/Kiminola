use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

use crate::asr::{resolve_asr_model_dir, AsrEngine};
use crate::recording_session::{
    AudioSource, DefaultAudioSource, RecordingSession, TranscriptChannel, TranscriptEvent,
    TranscriptSink,
};

/// Shared Tauri state for recording.
pub struct RecordingState {
    session: Mutex<Option<RecordingSession>>,
    asr_engine: Arc<tokio::sync::OnceCell<Option<Arc<AsrEngine>>>>,
}

impl RecordingState {
    fn new() -> Self {
        Self {
            session: Mutex::new(None),
            asr_engine: Arc::new(tokio::sync::OnceCell::new()),
        }
    }
}

/// Get the shared ASR engine, loading the model on first use. The load runs on
/// a blocking thread; concurrent callers share the same in-flight load.
async fn ensure_asr_engine(
    cell: &tokio::sync::OnceCell<Option<Arc<AsrEngine>>>,
) -> Option<Arc<AsrEngine>> {
    cell.get_or_init(|| async {
        let start = std::time::Instant::now();
        let engine = tokio::task::spawn_blocking(|| {
            resolve_asr_model_dir().and_then(|d| AsrEngine::new(&d).map(Arc::new))
        })
        .await
        .unwrap_or(None);
        eprintln!("[recording] ASR engine loaded in {:?}", start.elapsed());
        engine
    })
    .await
    .clone()
}

struct TauriTranscriptSink {
    app: AppHandle,
}

impl TranscriptSink for TauriTranscriptSink {
    fn emit(&self, channel: TranscriptChannel, text: &str, is_partial: bool) {
        if let Err(e) = self.app.emit(
            "transcript:event",
            TranscriptEvent {
                channel,
                text: text.into(),
                is_partial,
            },
        ) {
            eprintln!("failed to emit transcript:event: {e}");
        }
    }
}

/// Starts a recording session using the default microphone and system loopback.
///
/// Audio is resampled to 16 kHz mono, then streamed through sherpa-onnx ASR;
/// recognized text is emitted on `transcript:event`. If the ASR model is not
/// installed, recording still runs but no transcript text is produced.
#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: State<'_, RecordingState>,
) -> Result<(), String> {
    let mut session = state.session.lock().await;
    if session.is_some() {
        return Err("recording already in progress".into());
    }

    // Grab the preloaded ASR engine (warmed in the background at app launch);
    // lanes are cheap per-recording streams over the shared weights.
    let engine = ensure_asr_engine(&state.asr_engine).await;
    if engine.is_none() {
        eprintln!("ASR model not found; no transcript text will be produced");
    }

    let audio_source: Arc<dyn AudioSource> = Arc::new(DefaultAudioSource::new());
    let sink: Arc<dyn TranscriptSink> = Arc::new(TauriTranscriptSink { app });
    let new_session = RecordingSession::new(audio_source, engine, sink);

    new_session.start().await?;
    *session = Some(new_session);
    Ok(())
}

/// Stops the active recording session and clears state.
#[tauri::command]
pub async fn stop_recording(state: State<'_, RecordingState>) -> Result<(), String> {
    let mut session = state.session.lock().await;
    let Some(s) = session.take() else {
        return Err("no recording in progress".into());
    };
    s.stop().await
}

/// Pauses the active recording session. Capture streams are stopped but the
/// ASR lanes and consumer task stay alive so resume is fast.
#[tauri::command]
pub async fn pause_recording(state: State<'_, RecordingState>) -> Result<(), String> {
    let session = state.session.lock().await;
    let Some(s) = session.as_ref() else {
        return Err("no recording in progress".into());
    };
    s.pause().await
}

/// Resumes a paused recording session by rebuilding the capture streams.
#[tauri::command]
pub async fn resume_recording(state: State<'_, RecordingState>) -> Result<(), String> {
    let session = state.session.lock().await;
    let Some(s) = session.as_ref() else {
        return Err("no recording in progress".into());
    };
    s.resume().await
}

/// Helper used by `lib.rs` to install the recording state into the Tauri manager.
pub fn setup(app: &mut tauri::App) {
    let state = RecordingState::new();
    let cell = Arc::clone(&state.asr_engine);
    app.manage(state);

    // Warm the ASR model in the background so the first recording doesn't
    // wait on the ~650 MB encoder load.
    tauri::async_runtime::spawn(async move {
        let _ = ensure_asr_engine(&cell).await;
    });
}
