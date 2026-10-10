//! One owner for "may the process exit, hide, or update right now?".
//!
//! [`AppLifecycle`] holds the only process-wide copies of three facts:
//! - **recording**: a Capture session is starting, running or finalizing.
//!   Only `recording` writes it, through [`begin_recording`]/[`end_recording`];
//!   it is true from before capture starts until the transcript is final, so
//!   the process can never exit while a session is becoming durable.
//! - **updating**: an App update is being prepared. Recording cannot start
//!   until the update is cancelled or the app restarts.
//! - **quitting**: the user explicitly quit; windows may close for real.
//!
//! Every check-and-set happens under one lock, so "recording starts" and
//! "update begins" cannot both win. Decisions are plain values
//! ([`QuitDecision`], [`CloseDecision`]) so they are testable without Tauri.
//!
//! Update ordering (owned here): hold the recording session lock → mark
//! updating (fails while recording) → suspend the database (drains writes).
//! Cancelling resumes the database before clearing the updating mark.

use std::future::Future;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};

use crate::db::DbState;
use crate::db_safety::Database;
use crate::recording::RecordingState;

const UPDATE_BLOCKS_RECORDING: &str =
    "The app is preparing to update. Recording is paused until it restarts.";
const RECORDING_BLOCKS_UPDATE: &str = "Finish and save the current recording before updating.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitDecision {
    /// Quitting is allowed and has been recorded; exit the process now.
    Exit,
    /// A Capture session is active; surface it instead of exiting.
    BlockedByRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseDecision {
    /// Closing a window keeps Kimi Nola resident as the Background companion.
    HideToBackground,
    /// The user quit explicitly; let the window close.
    Close,
}

#[derive(Debug, Default)]
struct Gate {
    recording: bool,
    updating: bool,
    quitting: bool,
}

#[derive(Debug, Default)]
pub(crate) struct AppLifecycle {
    gate: Mutex<Gate>,
}

impl AppLifecycle {
    fn gate(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Mark a Capture session as active. Fails while an update is prepared.
    fn try_begin_recording(&self) -> Result<(), String> {
        let mut gate = self.gate();
        if gate.updating {
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

    /// Run the update barrier: mark updating (refused while recording), then
    /// `suspend`. A failed suspension clears the mark so recording stays usable.
    async fn prepare_update<F, Fut>(&self, suspend: F) -> Result<(), String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        {
            let mut gate = self.gate();
            if gate.recording {
                return Err(RECORDING_BLOCKS_UPDATE.into());
            }
            gate.updating = true;
        }
        if let Err(error) = suspend().await {
            self.gate().updating = false;
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
        self.gate().updating = false;
    }

    /// Explicit quit request (tray "Quit"). Allowed only when not recording;
    /// an allowed request is remembered so window closes are no longer hidden.
    pub(crate) fn request_quit(&self) -> QuitDecision {
        let mut gate = self.gate();
        if gate.recording {
            return QuitDecision::BlockedByRecording;
        }
        gate.quitting = true;
        QuitDecision::Exit
    }

    pub(crate) fn is_quitting(&self) -> bool {
        self.gate().quitting
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
    database: State<'_, DbState>,
) -> Result<(), String> {
    // Serialize with recording start/stop so a session cannot begin between
    // the activity check and database suspension.
    let _recording = recording.quiesce().await;
    lifecycle
        .prepare_update(|| database.database.suspend())
        .await
}

#[tauri::command]
pub async fn cancel_app_update(
    lifecycle: State<'_, AppLifecycle>,
    recording: State<'_, RecordingState>,
    database: State<'_, DbState>,
) -> Result<(), String> {
    let _recording = recording.quiesce().await;
    lifecycle
        .cancel_update(|| database.database.resume())
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quitting_is_blocked_for_the_whole_recording_lifecycle() {
        let lifecycle = AppLifecycle::default();
        lifecycle.try_begin_recording().unwrap();
        assert_eq!(lifecycle.request_quit(), QuitDecision::BlockedByRecording);
        assert!(!lifecycle.is_quitting(), "a blocked quit is not remembered");
        assert_eq!(lifecycle.close_requested(), CloseDecision::HideToBackground);

        lifecycle.finish_recording();
        assert_eq!(lifecycle.request_quit(), QuitDecision::Exit);
        assert!(lifecycle.is_quitting());
        assert_eq!(lifecycle.close_requested(), CloseDecision::Close);
    }

    #[test]
    fn closing_windows_hides_to_the_background_until_the_user_quits() {
        let lifecycle = AppLifecycle::default();
        assert_eq!(lifecycle.close_requested(), CloseDecision::HideToBackground);
        assert_eq!(lifecycle.request_quit(), QuitDecision::Exit);
        assert_eq!(lifecycle.close_requested(), CloseDecision::Close);
    }

    #[tokio::test]
    async fn an_active_recording_blocks_update_preparation() {
        let lifecycle = AppLifecycle::default();
        lifecycle.try_begin_recording().unwrap();
        let mut suspended = false;
        let suspended_slot = &mut suspended;
        let error = lifecycle
            .prepare_update(move || async move {
                *suspended_slot = true;
                Ok(())
            })
            .await
            .unwrap_err();
        assert_eq!(error, RECORDING_BLOCKS_UPDATE);
        assert!(!suspended, "the database must not be suspended mid-recording");
    }

    #[tokio::test]
    async fn a_prepared_update_blocks_recording_until_cancelled() {
        let lifecycle = AppLifecycle::default();
        lifecycle.prepare_update(|| async { Ok(()) }).await.unwrap();
        assert_eq!(
            lifecycle.try_begin_recording().unwrap_err(),
            UPDATE_BLOCKS_RECORDING
        );
        assert!(!lifecycle.is_recording_active());

        let mut resumed_while_updating = None;
        let resumed_slot = &mut resumed_while_updating;
        let observed = &lifecycle;
        lifecycle
            .cancel_update(move || async move {
                *resumed_slot = Some(observed.gate().updating);
            })
            .await;
        assert_eq!(
            resumed_while_updating,
            Some(true),
            "the database resumes before recording is allowed again"
        );
        lifecycle.try_begin_recording().unwrap();
        assert!(lifecycle.is_recording_active());
    }

    #[tokio::test]
    async fn failed_suspension_leaves_recording_available() {
        let lifecycle = AppLifecycle::default();
        let error = lifecycle
            .prepare_update(|| async { Err("database busy".to_string()) })
            .await
            .unwrap_err();
        assert_eq!(error, "database busy");
        lifecycle.try_begin_recording().unwrap();
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
        let lifecycle = AppLifecycle::default();

        lifecycle.prepare_update(|| database.suspend()).await.unwrap();
        assert!(database.pool().await.is_err(), "writes are drained and blocked");

        lifecycle.cancel_update(|| database.resume()).await;
        database.pool().await.unwrap().close().await;
        lifecycle.try_begin_recording().unwrap();
        drop(database);
        let _ = std::fs::remove_dir_all(root);
    }
}
