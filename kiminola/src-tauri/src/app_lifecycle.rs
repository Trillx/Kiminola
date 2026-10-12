//! One owner for "may the process exit, hide, or update right now?".
//!
//! [`AppLifecycle`] holds the only process-wide copies of three facts:
//! - **recording**: a Capture session is starting, running or finalizing.
//!   Only `recording` writes it, through [`begin_recording`]/[`end_recording`];
//!   it is true from before capture starts until the transcript is final, so
//!   the process can never exit while a session is becoming durable.
//! - **updating**: an App update is being prepared. It holds the
//!   [`CaptureGate`] Update lease, so neither a Meeting nor Dictation can
//!   start until the update is cancelled or the app restarts.
//! - **quitting**: the user explicitly quit; windows may close for real. It
//!   holds the Quit lease until the process exits.
//!
//! Every check-and-set happens under one lock, so "recording starts" and
//! "update begins" cannot both win; the capture gate extends the same
//! exclusion to Dictation. Decisions are plain values ([`QuitDecision`],
//! [`CloseDecision`]) so they are testable without Tauri.
//!
//! Update ordering (owned here): hold the recording session lock → take the
//! Update lease (fails while recording or dictating) → suspend the database
//! (drains writes). Cancelling resumes the database before releasing the lease.

use std::future::Future;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::capture_gate::{CaptureGate, CaptureLease, CaptureOwner};
use crate::db::DbState;
use crate::db_safety::Database;
use crate::recording::RecordingState;

const UPDATE_BLOCKS_RECORDING: &str =
    "The app is preparing to update. Recording is paused until it restarts.";
const RECORDING_BLOCKS_UPDATE: &str = "Finish and save the current recording before updating.";
const UPDATE_ALREADY_PREPARING: &str = "The app is already preparing an update.";
const RECORDING_BLOCKS_QUIT: &str = "Finish and save the current Meeting before quitting.";
const EVENT_RECORDING_QUIT_BLOCKED: &str = "recording:quit-blocked";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitDecision {
    /// Quitting is allowed and has been recorded; exit the process now.
    Exit,
    /// A Capture session is active; surface it instead of exiting.
    BlockedByRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseDecision {
    /// Closing a window keeps OatWave resident as the Background companion.
    HideToBackground,
    /// The user quit explicitly; let the window close.
    Close,
}

#[derive(Default)]
struct Gate {
    recording: bool,
    /// Held while an update is prepared; `Some` means updating.
    update: Option<CaptureLease>,
    /// Held until process exit; `Some` means quitting.
    quit: Option<CaptureLease>,
}

#[derive(Default)]
pub(crate) struct AppLifecycle {
    gate: Mutex<Gate>,
}

impl AppLifecycle {
    fn gate(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Mark a Capture session as active. Fails while an update is prepared.
    fn try_begin_recording(&self) -> Result<(), String> {
        let mut gate = self.gate();
        if gate.update.is_some() {
            return Err(UPDATE_BLOCKS_RECORDING.into());
        }
        gate.recording = true;
        Ok(())
    }

    fn finish_recording(&self) {
        self.gate().recording = false;
    }

    pub(crate) fn is_recording_active(&self) -> bool {
        self.gate().recording
    }

    /// Run the update barrier: take the Update lease (refused while recording
    /// or dictating), then `suspend`. The lease is published before awaiting
    /// so a disconnected IPC waiter cannot leave the barrier without
    /// ownership. A failed suspension releases it so capture stays usable.
    async fn prepare_update<F, Fut>(&self, capture: &CaptureGate, suspend: F) -> Result<(), String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        {
            let mut gate = self.gate();
            if gate.recording {
                return Err(RECORDING_BLOCKS_UPDATE.into());
            }
            if gate.update.is_some() {
                return Err(UPDATE_ALREADY_PREPARING.into());
            }
            gate.update = Some(capture.claim(CaptureOwner::Update)?);
        }
        if let Err(error) = suspend().await {
            self.gate().update = None;
            return Err(error);
        }
        Ok(())
    }

    /// Undo [`Self::prepare_update`]: `resume` first, then allow recording.
    async fn cancel_update<F, Fut>(&self, resume: F)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = ()>,
    {
        resume().await;
        self.gate().update = None;
    }

    /// Explicit quit request. Allowed only when not recording and `claim_quit`
    /// grants the Quit lease (no Dictation or update in flight). An allowed
    /// request keeps the lease until exit, so nothing can start in between,
    /// and is remembered so window closes are no longer hidden.
    fn request_quit(
        &self,
        claim_quit: impl FnOnce() -> Result<CaptureLease, String>,
    ) -> Result<QuitDecision, String> {
        let mut gate = self.gate();
        if gate.recording {
            return Ok(QuitDecision::BlockedByRecording);
        }
        gate.quit = Some(claim_quit()?);
        Ok(QuitDecision::Exit)
    }

    pub(crate) fn is_quitting(&self) -> bool {
        self.gate().quit.is_some()
    }

    /// What a window close request should do.
    pub(crate) fn close_requested(&self) -> CloseDecision {
        if self.is_quitting() {
            CloseDecision::Close
        } else {
            CloseDecision::HideToBackground
        }
    }
}

/// Recording-active query for native handlers (tray, toasts, overlays).
pub(crate) fn is_recording_active(app: &AppHandle) -> bool {
    app.try_state::<AppLifecycle>()
        .is_some_and(|lifecycle| lifecycle.is_recording_active())
}

/// Called by `recording` (holding its session lock) before capture starts.
/// Meeting presence receives the change as an input.
pub(crate) fn begin_recording(app: &AppHandle) -> Result<(), String> {
    app.state::<AppLifecycle>().try_begin_recording()?;
    crate::meeting_presence::recording_activity_changed(app, true);
    Ok(())
}

/// Called by `recording` after startup failure or transcript finalization.
pub(crate) fn end_recording(app: &AppHandle) {
    app.state::<AppLifecycle>().finish_recording();
    crate::meeting_presence::recording_activity_changed(app, false);
}

/// Quit unless a Meeting is recording. Dictation calls this once its own exit
/// decision is made; `review_resolved` lets that decision consume a pending
/// dictation review atomically with taking the Quit lease.
pub(crate) fn quit_when_idle(app: &AppHandle, review_resolved: bool) -> Result<(), String> {
    let capture = app.state::<CaptureGate>();
    let decision = app.state::<AppLifecycle>().request_quit(|| {
        if review_resolved {
            capture.claim_quit_after_review_resolved()
        } else {
            capture.claim(CaptureOwner::Quit)
        }
    })?;
    match decision {
        QuitDecision::Exit => {
            app.exit(0);
            Ok(())
        }
        QuitDecision::BlockedByRecording => {
            crate::background_companion::show_main_window(app);
            let _ = app.emit(EVENT_RECORDING_QUIT_BLOCKED, ());
            Err(RECORDING_BLOCKS_QUIT.into())
        }
    }
}

/// Seal the database and relaunch after a successful recovery action.
pub(crate) async fn restart_after_recovery(
    app: &AppHandle,
    database: &Database,
) -> Result<(), String> {
    database.prepare_restart().await?;
    app.restart();
}

#[tauri::command]
pub async fn prepare_app_update(
    lifecycle: State<'_, AppLifecycle>,
    recording: State<'_, RecordingState>,
    capture: State<'_, CaptureGate>,
    database: State<'_, DbState>,
) -> Result<(), String> {
    // Serialize with recording start/stop so a session cannot begin between
    // the activity check and database suspension.
    let _recording = recording.quiesce().await;
    lifecycle
        .prepare_update(&capture, || database.database.suspend())
        .await
}

#[tauri::command]
pub async fn cancel_app_update(
    lifecycle: State<'_, AppLifecycle>,
    recording: State<'_, RecordingState>,
    database: State<'_, DbState>,
) -> Result<(), String> {
    let _recording = recording.quiesce().await;
    lifecycle.cancel_update(|| database.database.resume()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quit_claim(capture: &CaptureGate) -> impl FnOnce() -> Result<CaptureLease, String> + '_ {
        || capture.claim(CaptureOwner::Quit)
    }

    #[test]
    fn quitting_is_blocked_for_the_whole_recording_lifecycle() {
        let capture = CaptureGate::default();
        let lifecycle = AppLifecycle::default();
        lifecycle.try_begin_recording().unwrap();
        assert_eq!(
            lifecycle.request_quit(quit_claim(&capture)),
            Ok(QuitDecision::BlockedByRecording)
        );
        assert!(!lifecycle.is_quitting(), "a blocked quit is not remembered");
        assert!(
            !capture.is_busy(),
            "a blocked quit takes no capture ownership"
        );
        assert_eq!(lifecycle.close_requested(), CloseDecision::HideToBackground);

        lifecycle.finish_recording();
        assert_eq!(
            lifecycle.request_quit(quit_claim(&capture)),
            Ok(QuitDecision::Exit)
        );
        assert!(lifecycle.is_quitting());
        assert_eq!(lifecycle.close_requested(), CloseDecision::Close);
    }

    #[test]
    fn closing_windows_hides_to_the_background_until_the_user_quits() {
        let capture = CaptureGate::default();
        let lifecycle = AppLifecycle::default();
        assert_eq!(lifecycle.close_requested(), CloseDecision::HideToBackground);
        assert_eq!(
            lifecycle.request_quit(quit_claim(&capture)),
            Ok(QuitDecision::Exit)
        );
        assert_eq!(lifecycle.close_requested(), CloseDecision::Close);
    }

    #[test]
    fn an_allowed_quit_keeps_capture_excluded_until_exit() {
        let capture = CaptureGate::default();
        let lifecycle = AppLifecycle::default();
        lifecycle.request_quit(quit_claim(&capture)).unwrap();
        assert!(capture.claim(CaptureOwner::Meeting).is_err());
        assert!(capture.claim(CaptureOwner::Dictation).is_err());
    }

    #[test]
    fn active_dictation_refuses_quit_without_remembering_it() {
        let capture = CaptureGate::default();
        let _dictation = capture.claim(CaptureOwner::Dictation).unwrap();
        let lifecycle = AppLifecycle::default();
        assert!(lifecycle.request_quit(quit_claim(&capture)).is_err());
        assert!(!lifecycle.is_quitting());
    }

    #[tokio::test]
    async fn an_active_recording_blocks_update_preparation() {
        let capture = CaptureGate::default();
        let lifecycle = AppLifecycle::default();
        lifecycle.try_begin_recording().unwrap();
        let mut suspended = false;
        let suspended_slot = &mut suspended;
        let error = lifecycle
            .prepare_update(&capture, move || async move {
                *suspended_slot = true;
                Ok(())
            })
            .await
            .unwrap_err();
        assert_eq!(error, RECORDING_BLOCKS_UPDATE);
        assert!(
            !suspended,
            "the database must not be suspended mid-recording"
        );
    }

    #[tokio::test]
    async fn active_dictation_blocks_update_preparation() {
        let capture = CaptureGate::default();
        let dictation = capture.claim(CaptureOwner::Dictation).unwrap();
        let lifecycle = AppLifecycle::default();
        let mut suspended = false;
        let suspended_slot = &mut suspended;
        assert!(lifecycle
            .prepare_update(&capture, move || async move {
                *suspended_slot = true;
                Ok(())
            })
            .await
            .is_err());
        assert!(
            !suspended,
            "the database must not be suspended mid-dictation"
        );

        drop(dictation);
        lifecycle
            .prepare_update(&capture, || async { Ok(()) })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_prepared_update_blocks_recording_and_dictation_until_cancelled() {
        let capture = CaptureGate::default();
        let lifecycle = AppLifecycle::default();
        lifecycle
            .prepare_update(&capture, || async { Ok(()) })
            .await
            .unwrap();
        assert_eq!(
            lifecycle.try_begin_recording().unwrap_err(),
            UPDATE_BLOCKS_RECORDING
        );
        assert!(!lifecycle.is_recording_active());
        assert!(capture.claim(CaptureOwner::Meeting).is_err());
        assert!(capture.claim(CaptureOwner::Dictation).is_err());
        assert_eq!(
            lifecycle
                .prepare_update(&capture, || async { Ok(()) })
                .await
                .unwrap_err(),
            UPDATE_ALREADY_PREPARING
        );

        let mut resumed_while_updating = None;
        let resumed_slot = &mut resumed_while_updating;
        let observed = &lifecycle;
        lifecycle
            .cancel_update(move || async move {
                *resumed_slot = Some(observed.gate().update.is_some());
            })
            .await;
        assert_eq!(
            resumed_while_updating,
            Some(true),
            "the database resumes before recording is allowed again"
        );
        lifecycle.try_begin_recording().unwrap();
        assert!(lifecycle.is_recording_active());
        capture.claim(CaptureOwner::Dictation).unwrap();
    }

    #[tokio::test]
    async fn failed_suspension_leaves_recording_available() {
        let capture = CaptureGate::default();
        let lifecycle = AppLifecycle::default();
        let error = lifecycle
            .prepare_update(&capture, || async { Err("database busy".to_string()) })
            .await
            .unwrap_err();
        assert_eq!(error, "database busy");
        lifecycle.try_begin_recording().unwrap();
        assert!(!capture.is_busy(), "the Update lease is released");
    }

    #[tokio::test]
    async fn update_barrier_suspends_a_real_database_and_cancel_resumes_it() {
        let root = std::env::temp_dir().join(format!(
            "kiminola-lifecycle-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let database = Database::new(Ok(root.join("kiminola.db")));
        database.pool().await.unwrap();
        let capture = CaptureGate::default();
        let lifecycle = AppLifecycle::default();

        lifecycle
            .prepare_update(&capture, || database.suspend())
            .await
            .unwrap();
        assert!(
            database.pool().await.is_err(),
            "writes are drained and blocked"
        );

        lifecycle.cancel_update(|| database.resume()).await;
        database.pool().await.unwrap().close().await;
        lifecycle.try_begin_recording().unwrap();
        drop(database);
        let _ = std::fs::remove_dir_all(root);
    }
}
