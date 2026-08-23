use std::collections::HashMap;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

use crate::asr::{resolve_asr_model_dir, AsrEngine};
use crate::recording_session::{
    AudioSource, DefaultAudioSource, RecordingSession, TranscriptEvent, TranscriptSink,
};

/// Shared Tauri state for recording.
pub struct RecordingState {
    session: Mutex<Option<ActiveRecording>>,
    asr_engine: Arc<tokio::sync::OnceCell<Option<Arc<AsrEngine>>>>,
    pending_loopback_target: SyncMutex<Option<PendingLoopbackTarget>>,
}

struct ActiveRecording {
    session: RecordingSession,
    transcript_store: Arc<TranscriptEventStore>,
}

#[derive(Default)]
struct TranscriptEventStore {
    latest: SyncMutex<HashMap<u64, TranscriptEvent>>,
}

impl TranscriptEventStore {
    fn record(&self, event: &TranscriptEvent) {
        let mut latest = self.latest.lock().unwrap();
        if latest
            .get(&event.utterance_id)
            .is_some_and(|existing| existing.revision >= event.revision)
        {
            return;
        }
        latest.insert(event.utterance_id, event.clone());
    }

    fn snapshot(&self) -> Vec<TranscriptEvent> {
        let mut events: Vec<_> = self.latest.lock().unwrap().values().cloned().collect();
        events.sort_by_key(|event| (event.start_ms, event.utterance_id));
        events
    }
}

const PENDING_LOOPBACK_TARGET_TTL: Duration = Duration::from_secs(60);

struct PendingLoopbackTarget {
    process_id: u32,
    queued_at: Instant,
}

impl RecordingState {
    fn new() -> Self {
        Self {
            session: Mutex::new(None),
            asr_engine: Arc::new(tokio::sync::OnceCell::new()),
            pending_loopback_target: SyncMutex::new(None),
        }
    }

    fn take_loopback_target(&self) -> Option<u32> {
        self.pending_loopback_target
            .lock()
            .unwrap()
            .take()
            .filter(|target| target.queued_at.elapsed() <= PENDING_LOOPBACK_TARGET_TTL)
            .map(|target| target.process_id)
    }
}

/// Carries the detected meeting PID across the prompt-to-recording navigation.
/// It is consumed once by `start_recording` and expires quickly so a later
/// manual meeting can never inherit a stale capture target.
pub fn queue_process_loopback_target(app: &AppHandle, process_id: u32) {
    if let Some(state) = app.try_state::<RecordingState>() {
        *state.pending_loopback_target.lock().unwrap() = Some(PendingLoopbackTarget {
            process_id,
            queued_at: Instant::now(),
        });
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
    store: Arc<TranscriptEventStore>,
}

impl TranscriptSink for TauriTranscriptSink {
    fn emit(&self, event: TranscriptEvent) {
        self.store.record(&event);
        if let Err(e) = self.app.emit("transcript:event", event) {
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

    let target_process_id = state.take_loopback_target();
    let audio_source: Arc<dyn AudioSource> =
        Arc::new(DefaultAudioSource::for_process(target_process_id));
    let transcript_store = Arc::new(TranscriptEventStore::default());
    let sink: Arc<dyn TranscriptSink> = Arc::new(TauriTranscriptSink {
        app,
        store: Arc::clone(&transcript_store),
    });
    let new_session = RecordingSession::new(audio_source, engine, sink);

    new_session.start().await?;
    *session = Some(ActiveRecording {
        session: new_session,
        transcript_store,
    });
    Ok(())
}

/// Stops the active recording session and returns the authoritative latest
/// revision of every utterance. The response closes the event-delivery race at
/// save time: the frontend can merge this snapshot before persisting.
#[tauri::command]
pub async fn stop_recording(
    state: State<'_, RecordingState>,
) -> Result<Vec<TranscriptEvent>, String> {
    let mut session = state.session.lock().await;
    let Some(active) = session.take() else {
        return Err("no recording in progress".into());
    };
    active.session.stop().await?;
    Ok(active.transcript_store.snapshot())
}

/// Pauses the active recording session. Capture streams are stopped but the
/// ASR lanes and consumer task stay alive so resume is fast.
#[tauri::command]
pub async fn pause_recording(state: State<'_, RecordingState>) -> Result<(), String> {
    let session = state.session.lock().await;
    let Some(active) = session.as_ref() else {
        return Err("no recording in progress".into());
    };
    active.session.pause().await
}

/// Resumes a paused recording session by rebuilding the capture streams.
#[tauri::command]
pub async fn resume_recording(state: State<'_, RecordingState>) -> Result<(), String> {
    let session = state.session.lock().await;
    let Some(active) = session.as_ref() else {
        return Err("no recording in progress".into());
    };
    active.session.resume().await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording_session::TranscriptChannel;

    fn event(utterance_id: u64, revision: u32, text: &str) -> TranscriptEvent {
        TranscriptEvent {
            utterance_id,
            revision,
            channel: TranscriptChannel::You,
            text: text.into(),
            is_partial: revision == 1,
            start_ms: 100,
            end_ms: 500,
        }
    }

    #[test]
    fn transcript_store_returns_only_the_latest_revision() {
        let store = TranscriptEventStore::default();
        store.record(&event(3, 2, "final"));
        store.record(&event(3, 1, "stale"));

        let snapshot = store.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].revision, 2);
        assert_eq!(snapshot[0].text, "final");
    }

    #[test]
    fn pending_process_target_is_one_shot_and_expires() {
        let state = RecordingState::new();
        *state.pending_loopback_target.lock().unwrap() = Some(PendingLoopbackTarget {
            process_id: 42,
            queued_at: Instant::now(),
        });
        assert_eq!(state.take_loopback_target(), Some(42));
        assert_eq!(state.take_loopback_target(), None);

        *state.pending_loopback_target.lock().unwrap() = Some(PendingLoopbackTarget {
            process_id: 99,
            queued_at: Instant::now() - PENDING_LOOPBACK_TARGET_TTL - Duration::from_secs(1),
        });
        assert_eq!(state.take_loopback_target(), None);
    }
}
