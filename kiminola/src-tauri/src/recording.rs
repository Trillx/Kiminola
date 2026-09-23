use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

use crate::asr::{resolve_asr_model_dir, AsrEngine};
use crate::capture_gate::{CaptureGate, CaptureLease, CaptureOwner};
use crate::recording_session::{
    AudioPressureEvent, AudioSource, DefaultAudioSource, RecordingSession, RecordingStartStatus,
    TranscriptEvent, TranscriptSink,
};

/// Shared Tauri state for recording.
pub struct RecordingState {
    session: Mutex<Option<ActiveRecording>>,
    asr_engine: Arc<Mutex<Option<Arc<AsrEngine>>>>,
    pending_loopback_target: SyncMutex<Option<PendingLoopbackTarget>>,
    active: AtomicBool,
    updating: AtomicBool,
    update_lease: SyncMutex<Option<CaptureLease>>,
}

struct ActiveRecording {
    session: RecordingSession,
    transcript_store: Arc<TranscriptEventStore>,
    // Ownership lasts through pause and the final decoder drain.
    _capture_lease: CaptureLease,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct RecordingStopResult {
    pub transcript: Vec<TranscriptEvent>,
    pub finalization_warning: Option<String>,
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

fn result_after_finalization(
    stop_result: Result<(), String>,
    store: &TranscriptEventStore,
) -> RecordingStopResult {
    let finalization_warning = stop_result.err().map(|error| {
        // A final ASR flush can time out after the live transcript has already
        // accumulated useful revisions. Returning that authoritative snapshot
        // is safer than trapping the frontend before it can persist anything.
        eprintln!("[recording] transcript finalization warning: {error}");
        "The final speech-processing pass did not finish, so the last few words may be missing."
            .to_string()
    });
    RecordingStopResult {
        transcript: store.snapshot(),
        finalization_warning,
    }
}

const PENDING_LOOPBACK_TARGET_TTL: Duration = Duration::from_secs(60);

struct PendingLoopbackTarget {
    process_id: u32,
    process_started_at: u64,
    queued_at: Instant,
}

impl RecordingState {
    fn new() -> Self {
        Self {
            session: Mutex::new(None),
            asr_engine: Arc::new(Mutex::new(None)),
            pending_loopback_target: SyncMutex::new(None),
            active: AtomicBool::new(false),
            updating: AtomicBool::new(false),
            update_lease: SyncMutex::new(None),
        }
    }

    fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    fn set_active_for_app(&self, app: &AppHandle, active: bool) {
        self.set_active(active);
        crate::meeting_presence::recording_activity_changed(app, active);
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn take_loopback_target(
        &self,
        is_current: impl FnOnce(u32, u64) -> bool,
    ) -> Result<Option<u32>, String> {
        let target = self
            .pending_loopback_target
            .lock()
            .unwrap()
            .take()
            .filter(|target| target.queued_at.elapsed() <= PENDING_LOOPBACK_TARGET_TTL);
        let Some(target) = target else {
            return Ok(None);
        };
        if !is_current(target.process_id, target.process_started_at) {
            return Err("The detected app closed or changed before recording started. Please start a new recording.".into());
        }
        Ok(Some(target.process_id))
    }

    fn clear_pending_loopback_target(&self) {
        self.pending_loopback_target.lock().unwrap().take();
    }
}

/// Synchronous recording activity check for native lifecycle handlers such as
/// the tray menu. `true` includes startup and transcript finalization so the
/// process cannot exit while a session is becoming durable.
pub fn is_recording_active(app: &AppHandle) -> bool {
    app.try_state::<RecordingState>()
        .is_some_and(|state| state.is_active())
}

/// Carries the detected meeting PID across the prompt-to-recording navigation.
/// It is consumed once by `start_recording` and expires quickly so a later
/// manual meeting can never inherit a stale capture target.
pub fn queue_process_loopback_target(app: &AppHandle, process_id: u32, process_started_at: u64) {
    if let Some(state) = app.try_state::<RecordingState>() {
        *state.pending_loopback_target.lock().unwrap() = Some(PendingLoopbackTarget {
            process_id,
            process_started_at,
            queued_at: Instant::now(),
        });
    }
}

/// Ensures a default-output start cannot inherit an earlier prompt's
/// still-unconsumed process target.
pub fn clear_process_loopback_target(app: &AppHandle) {
    if let Some(state) = app.try_state::<RecordingState>() {
        state.clear_pending_loopback_target();
    }
}

/// Get the shared ASR engine, loading the model on first use. Failed loads stay
/// retryable so a model installed during onboarding works without an app
/// restart; successful loads remain cached for the process lifetime.
async fn ensure_asr_engine(cache: &Arc<Mutex<Option<Arc<AsrEngine>>>>) -> Option<Arc<AsrEngine>> {
    let start = std::time::Instant::now();
    let engine = get_or_try_init(cache, || {
        resolve_asr_model_dir().and_then(|directory| AsrEngine::new(&directory))
    })
    .await;
    eprintln!("[recording] ASR engine resolved in {:?}", start.elapsed());
    engine
}

pub(crate) async fn shared_asr_engine(app: &AppHandle) -> Option<Arc<AsrEngine>> {
    let state = app.state::<RecordingState>();
    ensure_asr_engine(&state.asr_engine).await
}

async fn get_or_try_init<T, F>(cache: &Arc<Mutex<Option<Arc<T>>>>, loader: F) -> Option<Arc<T>>
where
    T: Send + Sync + 'static,
    F: FnOnce() -> Option<T> + Send + 'static,
{
    let mut cached = Arc::clone(cache).lock_owned().await;
    if let Some(value) = cached.as_ref() {
        return Some(Arc::clone(value));
    }

    // The blocking load cannot be cancelled once started. Give it ownership of
    // the cache guard as well, so dropping any waiter neither permits another
    // cold load nor loses the completed engine. Only model initialization runs
    // here; a cancelled caller must never continue into microphone startup.
    tokio::task::spawn_blocking(move || {
        let loaded = loader().map(Arc::new);
        *cached = loaded.clone();
        loaded
    })
    .await
    .ok()
    .flatten()
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

    fn emit_audio_pressure(&self, event: AudioPressureEvent) {
        eprintln!(
            "[recording] capture queue pressure: {} mic sample(s), {} loopback sample(s) dropped",
            event.mic_dropped_samples, event.loopback_dropped_samples
        );
        if let Err(e) = self.app.emit("recording:audio-pressure", event) {
            eprintln!("failed to emit recording:audio-pressure: {e}");
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
    database: State<'_, crate::db::DbState>,
) -> Result<RecordingStartStatus, String> {
    crate::db::ensure_pool(&database.pool).await?;
    let mut session = state.session.lock().await;
    if state.updating.load(Ordering::Acquire) {
        return Err(
            "The app is preparing to update. Recording is paused until it restarts.".into(),
        );
    }
    if session.is_some() {
        return Err("recording already in progress".into());
    }
    let capture_lease = app.state::<CaptureGate>().claim(CaptureOwner::Meeting)?;
    state.set_active_for_app(&app, true);

    // Grab the preloaded ASR engine (warmed in the background at app launch);
    // lanes are cheap per-recording streams over the shared weights.
    let engine = ensure_asr_engine(&state.asr_engine).await;
    if engine.is_none() {
        eprintln!("ASR model not found; no transcript text will be produced");
    }

    let target_process_id =
        match state.take_loopback_target(crate::meeting_presence::process_instance_is_current) {
            Ok(target) => target,
            Err(error) => {
                drop(capture_lease);
                state.set_active_for_app(&app, false);
                return Err(error);
            }
        };
    let audio_source: Arc<dyn AudioSource> =
        Arc::new(DefaultAudioSource::for_process(target_process_id));
    let transcript_store = Arc::new(TranscriptEventStore::default());
    let sink: Arc<dyn TranscriptSink> = Arc::new(TauriTranscriptSink {
        app: app.clone(),
        store: Arc::clone(&transcript_store),
    });
    let new_session = RecordingSession::new(audio_source, engine, sink);

    let start_status = match new_session.start().await {
        Ok(status) => status,
        Err(error) => {
            drop(capture_lease);
            state.set_active_for_app(&app, false);
            return Err(error);
        }
    };
    *session = Some(ActiveRecording {
        session: new_session,
        transcript_store,
        _capture_lease: capture_lease,
    });
    if let Err(error) = app.emit("recording:started", ()) {
        eprintln!("failed to emit recording:started: {error}");
    }
    Ok(start_status)
}

#[tauri::command]
pub async fn prepare_app_update(
    app: AppHandle,
    state: State<'_, RecordingState>,
    database: State<'_, crate::db::DbState>,
) -> Result<(), String> {
    let session = state.session.lock().await;
    if session.is_some() || state.is_active() {
        return Err("Finish and save the current recording before updating.".into());
    }
    if state.updating.load(Ordering::Acquire) {
        return Err("The app is already preparing an update.".into());
    }
    let lease = app.state::<CaptureGate>().claim(CaptureOwner::Update)?;
    state.updating.store(true, Ordering::Release);
    // Publish before awaiting suspension so a disconnected IPC waiter cannot
    // leave the update barrier without ownership.
    *state.update_lease.lock().unwrap() = Some(lease);
    if let Err(error) = database.pool.suspend().await {
        state.update_lease.lock().unwrap().take();
        state.updating.store(false, Ordering::Release);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub async fn cancel_app_update(
    state: State<'_, RecordingState>,
    database: State<'_, crate::db::DbState>,
) -> Result<(), String> {
    let _session = state.session.lock().await;
    database.pool.resume().await;
    state.updating.store(false, Ordering::Release);
    state.update_lease.lock().unwrap().take();
    Ok(())
}

/// Stops the active recording session and returns the authoritative latest
/// revision of every utterance. The response closes the event-delivery race at
/// save time: the frontend can merge this snapshot before persisting. Repeated
/// calls are safe and return an empty delta, which lets the UI recover when the
/// first command completed but its IPC response was lost.
#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: State<'_, RecordingState>,
) -> Result<RecordingStopResult, String> {
    let mut session = state.session.lock().await;
    let Some(active) = session.take() else {
        state.set_active_for_app(&app, false);
        return Ok(RecordingStopResult {
            transcript: Vec::new(),
            finalization_warning: None,
        });
    };
    let stop_result = active.session.stop().await;
    let result = result_after_finalization(stop_result, &active.transcript_store);
    drop(active);
    state.set_active_for_app(&app, false);
    Ok(result)
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
pub async fn resume_recording(
    state: State<'_, RecordingState>,
) -> Result<RecordingStartStatus, String> {
    let session = state.session.lock().await;
    let Some(active) = session.as_ref() else {
        return Err("no recording in progress".into());
    };
    active.session.resume().await
}

/// Helper used by `lib.rs` to install the recording state into the Tauri manager.
pub fn setup(app: &mut tauri::App) {
    app.manage(CaptureGate::default());
    let state = RecordingState::new();
    let cache = Arc::clone(&state.asr_engine);
    app.manage(state);

    // Warm the ASR model in the background so the first recording doesn't
    // wait on the ~650 MB encoder load.
    tauri::async_runtime::spawn(async move {
        let _ = ensure_asr_engine(&cache).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording_session::TranscriptChannel;
    use std::sync::atomic::AtomicUsize;

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

    #[tokio::test]
    async fn asr_cache_retries_failed_loads_and_keeps_the_first_success() {
        let cache = Arc::new(Mutex::new(None));
        let attempts = Arc::new(AtomicUsize::new(0));

        let first_attempts = Arc::clone(&attempts);
        let missing = get_or_try_init(&cache, move || {
            first_attempts.fetch_add(1, Ordering::Relaxed);
            None::<String>
        })
        .await;
        assert!(missing.is_none());

        let second_attempts = Arc::clone(&attempts);
        let loaded = get_or_try_init(&cache, move || {
            second_attempts.fetch_add(1, Ordering::Relaxed);
            Some("ready".to_string())
        })
        .await
        .expect("a later install should be loadable");

        let third_attempts = Arc::clone(&attempts);
        let cached = get_or_try_init(&cache, move || {
            third_attempts.fetch_add(1, Ordering::Relaxed);
            Some("replacement".to_string())
        })
        .await
        .expect("the successful load should remain cached");

        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        assert!(Arc::ptr_eq(&loaded, &cached));
        assert_eq!(cached.as_str(), "ready");
    }

    #[tokio::test]
    async fn asr_cache_cancelled_waiter_keeps_cold_load_for_meeting_and_dictation() {
        let cache = Arc::new(Mutex::new(None));
        let attempts = Arc::new(AtomicUsize::new(0));
        let microphone_started = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let waiter = {
            let cache = Arc::clone(&cache);
            let attempts = Arc::clone(&attempts);
            let microphone_started = Arc::clone(&microphone_started);
            tokio::spawn(async move {
                let engine = get_or_try_init(&cache, move || {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Some("original cold load".to_string())
                })
                .await;
                // This stands for the caller's next step, never a real mic.
                microphone_started.store(true, Ordering::SeqCst);
                engine
            })
        };
        tokio::time::timeout(Duration::from_secs(5), started_rx)
            .await
            .expect("the blocking loader should start")
            .unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());

        let meeting_attempts = Arc::clone(&attempts);
        let meeting = get_or_try_init(&cache, move || {
            meeting_attempts.fetch_add(1, Ordering::SeqCst);
            Some("duplicate Meeting load".to_string())
        });
        let dictation_attempts = Arc::clone(&attempts);
        let dictation = get_or_try_init(&cache, move || {
            dictation_attempts.fetch_add(1, Ordering::SeqCst);
            Some("duplicate Dictation load".to_string())
        });
        tokio::pin!(meeting, dictation);
        // Poll both real cache consumers while the original loader is blocked.
        // No scheduling sleeps or expensive ASR model instances are needed.
        assert!(futures::poll!(meeting.as_mut()).is_pending());
        assert!(futures::poll!(dictation.as_mut()).is_pending());
        release_tx.send(()).unwrap();
        let (meeting, dictation) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(meeting, dictation)
        })
        .await
        .expect("both consumers should resolve after the loader is released");
        let meeting = meeting.unwrap();
        let dictation = dictation.unwrap();
        let cached = get_or_try_init(&cache, || panic!("a warm cache must not reload"))
            .await
            .unwrap();

        assert_eq!(attempts.load(Ordering::SeqCst), 1, "only one cold load");
        assert_eq!(meeting.as_str(), "original cold load");
        assert!(Arc::ptr_eq(&meeting, &dictation));
        assert!(Arc::ptr_eq(&meeting, &cached));
        assert!(!microphone_started.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn asr_cache_cancelled_load_completes_without_any_waiter() {
        for outcome in [Some("ready"), None] {
            let cache = Arc::new(Mutex::new(None));
            let attempts = Arc::new(AtomicUsize::new(0));
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let waiter = {
                let cache = Arc::clone(&cache);
                let attempts = Arc::clone(&attempts);
                tokio::spawn(async move {
                    get_or_try_init(&cache, move || {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        outcome.map(str::to_string)
                    })
                    .await
                })
            };
            tokio::time::timeout(Duration::from_secs(5), started_rx)
                .await
                .unwrap()
                .unwrap();
            waiter.abort();
            assert!(waiter.await.unwrap_err().is_cancelled());
            release_tx.send(()).unwrap();

            // Observe completion without polling another initialization future.
            let completed = tokio::time::timeout(Duration::from_secs(5), cache.lock())
                .await
                .expect("completion must release the cache even with no waiters")
                .clone();
            assert_eq!(completed.as_deref().map(String::as_str), outcome);
            let retry_attempts = Arc::clone(&attempts);
            let next = get_or_try_init(&cache, move || {
                retry_attempts.fetch_add(1, Ordering::SeqCst);
                Some("retry".to_string())
            })
            .await
            .unwrap();
            if let Some(completed) = completed {
                assert!(Arc::ptr_eq(&completed, &next));
                assert_eq!(attempts.load(Ordering::SeqCst), 1);
            } else {
                assert_eq!(next.as_str(), "retry");
                assert_eq!(attempts.load(Ordering::SeqCst), 2);
            }
        }
    }

    #[tokio::test]
    async fn asr_cache_concurrent_cold_consumers_serialize_success_and_retry() {
        for outcome in [Some("ready"), None] {
            let cache = Arc::new(Mutex::new(None));
            let load_active = Arc::new(AtomicBool::new(false));
            let retry_attempts = Arc::new(AtomicUsize::new(0));
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let meeting = {
                let cache = Arc::clone(&cache);
                let load_active = Arc::clone(&load_active);
                tokio::spawn(async move {
                    get_or_try_init(&cache, move || {
                        load_active.store(true, Ordering::SeqCst);
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        load_active.store(false, Ordering::SeqCst);
                        outcome.map(str::to_string)
                    })
                    .await
                })
            };
            tokio::time::timeout(Duration::from_secs(5), started_rx)
                .await
                .unwrap()
                .unwrap();
            let attempts = Arc::clone(&retry_attempts);
            let dictation = get_or_try_init(&cache, move || {
                assert!(
                    !load_active.load(Ordering::SeqCst),
                    "loads must not overlap"
                );
                attempts.fetch_add(1, Ordering::SeqCst);
                Some("retry".to_string())
            });
            tokio::pin!(dictation);
            assert!(futures::poll!(dictation.as_mut()).is_pending());
            release_tx.send(()).unwrap();
            let (meeting, dictation) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(meeting, dictation)
            })
            .await
            .unwrap();
            let meeting = meeting.unwrap();
            let dictation = dictation.unwrap();
            assert_eq!(meeting.as_deref().map(String::as_str), outcome);
            if let Some(meeting) = meeting {
                assert!(Arc::ptr_eq(&meeting, &dictation));
                assert_eq!(retry_attempts.load(Ordering::SeqCst), 0);
            } else {
                assert_eq!(dictation.as_str(), "retry");
                assert_eq!(retry_attempts.load(Ordering::SeqCst), 1);
            }
        }
    }

    #[tokio::test]
    async fn asr_cache_panicked_loader_releases_ownership_for_retry() {
        let cache = Arc::new(Mutex::new(None));
        assert!(
            get_or_try_init(&cache, || panic!("controlled loader panic"))
                .await
                .is_none()
        );
        let retry = tokio::time::timeout(
            Duration::from_secs(5),
            get_or_try_init(&cache, || Some("retry")),
        )
        .await
        .expect("a panic must not leave initialization locked")
        .unwrap();
        assert_eq!(*retry, "retry");
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
    fn finalization_warning_keeps_the_latest_transcript_snapshot() {
        let store = TranscriptEventStore::default();
        store.record(&event(8, 2, "recover this text"));

        let result = result_after_finalization(Err("flush timed out".into()), &store);

        assert_eq!(result.transcript.len(), 1);
        assert_eq!(result.transcript[0].text, "recover this text");
        assert!(result.finalization_warning.is_some());
    }

    #[test]
    fn successful_finalization_has_no_warning() {
        let store = TranscriptEventStore::default();
        let result = result_after_finalization(Ok(()), &store);

        assert!(result.transcript.is_empty());
        assert!(result.finalization_warning.is_none());
    }

    #[test]
    fn pending_process_target_is_one_shot_and_expires() {
        let state = RecordingState::new();
        *state.pending_loopback_target.lock().unwrap() = Some(PendingLoopbackTarget {
            process_id: 42,
            process_started_at: 1,
            queued_at: Instant::now(),
        });
        assert_eq!(state.take_loopback_target(|_, _| true), Ok(Some(42)));
        assert_eq!(state.take_loopback_target(|_, _| true), Ok(None));

        *state.pending_loopback_target.lock().unwrap() = Some(PendingLoopbackTarget {
            process_id: 99,
            process_started_at: 1,
            queued_at: Instant::now() - PENDING_LOOPBACK_TARGET_TTL - Duration::from_secs(1),
        });
        assert_eq!(state.take_loopback_target(|_, _| true), Ok(None));
    }

    #[test]
    fn pending_process_target_can_be_cleared_before_default_output_capture() {
        let state = RecordingState::new();
        *state.pending_loopback_target.lock().unwrap() = Some(PendingLoopbackTarget {
            process_id: 42,
            process_started_at: 1,
            queued_at: Instant::now(),
        });

        state.clear_pending_loopback_target();

        assert_eq!(state.take_loopback_target(|_, _| true), Ok(None));
    }

    #[test]
    fn replaced_process_target_errors_instead_of_capturing_another_app() {
        let state = RecordingState::new();
        *state.pending_loopback_target.lock().unwrap() = Some(PendingLoopbackTarget {
            process_id: 42,
            process_started_at: 7,
            queued_at: Instant::now(),
        });
        let result = state.take_loopback_target(|pid, started_at| {
            assert_eq!((pid, started_at), (42, 7));
            false
        });
        assert!(
            result.is_err(),
            "a stale target must not fall back to system-wide audio"
        );
        assert!(state.pending_loopback_target.lock().unwrap().is_none());
    }

    #[test]
    fn recording_activity_covers_the_full_native_lifecycle() {
        let state = RecordingState::new();
        assert!(!state.is_active());

        state.set_active(true);
        assert!(state.is_active());

        state.set_active(false);
        assert!(!state.is_active());
    }
}
