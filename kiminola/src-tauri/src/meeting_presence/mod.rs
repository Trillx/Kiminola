//! Opt-in Meeting presence hints and the Meeting prompt lifecycle.
//!
//! Detection is deliberately advisory. It requires a visible/known app
//! signal and an active Core Audio session, and no prompt action records
//! audio by itself.
//!
//! Structure:
//! - [`classifier`]: pure snapshot → candidates classification.
//! - [`probe`]: where snapshots come from (Win32 or fixture adapters).
//! - [`MeetingPresenceState`]: the episode/prompt state machine. Each app
//!   family gets one prompt per meeting episode; an episode ends after two
//!   consecutive polls without its audio, or immediately when the app exits.
//!   Prompts are single-use and stale ids are rejected. Recording activity is
//!   an input (received from `app_lifecycle`): while recording, no hints or
//!   prompts exist and every active episode is consumed.
//! - This file's Tauri glue: IPC commands, the detector loop, prompt actions
//!   shared by the in-app UI and native toasts, and event emission. Prompt
//!   *delivery* outside the main window (tray, overlay, toasts) belongs to
//!   `crate::background_companion`.

pub(crate) mod classifier;
pub(crate) mod probe;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::background_companion;
use crate::db::{meeting_store, settings, DbState};
use classifier::DetectionSnapshot;
use probe::{PresenceProbe, Win32Probe};

const ENABLED_KEY: &str = "meeting_presence_enabled";
const START_WITH_WINDOWS_KEY: &str = "meeting_presence_start_with_windows";
const EVENT_STATE: &str = "meeting-presence:state";
const EVENT_PROMPT: &str = "meeting-presence:prompt";
const EVENT_ACTION: &str = "meeting-presence:action";
pub(crate) const PROMPT_MESSAGE: &str = "You may be in a meeting. Want to jot notes?";
pub(crate) const PROMPT_NOT_RECORDING_MESSAGE: &str = "Kimi Nola is not recording.";
const INACTIVE_POLLS_BEFORE_SESSION_RESET: u8 = 2;
const POLL_INTERVAL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MeetingPresenceMode {
    Off,
    Paused,
    Detecting,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)] // Reserved for quiet single-signal hints; visible prompts are likely only.
pub enum MeetingPresenceConfidence {
    Possible,
    Likely,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeetingPresenceEvidence {
    AppOrVisibleWindow,
    ActiveCoreAudio,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingPrompt {
    pub id: String,
    pub app_label: String,
    pub message: String,
    pub not_recording_message: String,
    pub confidence: MeetingPresenceConfidence,
    pub evidence: Vec<MeetingPresenceEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingPresenceHint {
    pub app_label: String,
    pub confidence: MeetingPresenceConfidence,
    pub evidence: Vec<MeetingPresenceEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingPresenceSnapshot {
    pub enabled: bool,
    pub paused: bool,
    pub start_with_windows: bool,
    pub mode: MeetingPresenceMode,
    pub hint: Option<MeetingPresenceHint>,
    pub prompt: Option<MeetingPrompt>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingPrompt {
    pub(crate) id: String,
    // Stable app-family root for episode suppression and process-loopback capture.
    process_id: u32,
    window_process_id: Option<u32>,
    pub(crate) app_label: String,
    confidence: MeetingPresenceConfidence,
    evidence: Vec<MeetingPresenceEvidence>,
}

#[cfg(test)]
impl PendingPrompt {
    pub(crate) fn for_test(id: &str, app_label: &str) -> Self {
        Self {
            id: id.into(),
            process_id: 42,
            window_process_id: Some(42),
            app_label: app_label.into(),
            confidence: MeetingPresenceConfidence::Likely,
            evidence: Vec::new(),
        }
    }
}

impl PendingPrompt {
    fn to_ipc(&self) -> MeetingPrompt {
        MeetingPrompt {
            id: self.id.clone(),
            app_label: self.app_label.clone(),
            message: PROMPT_MESSAGE.to_string(),
            not_recording_message: PROMPT_NOT_RECORDING_MESSAGE.to_string(),
            confidence: self.confidence,
            evidence: self.evidence.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct DetectionSession {
    // Suppression and episode activity survive a prompt; labels and evidence
    // are kept on the current detection/prompt and are never retained here.
    prompted: bool,
    suppressed: bool,
    inactive_polls: u8,
}

#[derive(Default)]
struct PresenceData {
    /// Input from `app_lifecycle`; read under this lock so an in-flight poll
    /// cannot recreate a prompt that recording startup just cleared.
    recording_active: bool,
    last_notified_prompt: Option<String>,
    hint: Option<classifier::Detection>,
    prompt: Option<PendingPrompt>,
    sessions: HashMap<u32, DetectionSession>,
}

pub struct MeetingPresenceState {
    inner: Arc<Inner>,
}

struct Inner {
    enabled: AtomicBool,
    paused: AtomicBool,
    start_with_windows: AtomicBool,
    next_prompt_id: AtomicU64,
    data: Mutex<PresenceData>,
}

impl Clone for MeetingPresenceState {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl MeetingPresenceState {
    fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                enabled: AtomicBool::new(false),
                paused: AtomicBool::new(false),
                start_with_windows: AtomicBool::new(false),
                next_prompt_id: AtomicU64::new(1),
                data: Mutex::new(PresenceData::default()),
            }),
        }
    }

    pub fn snapshot(&self) -> MeetingPresenceSnapshot {
        let (hint, prompt) = {
            let data = self.inner.data.lock().unwrap();
            let hint = data.hint.as_ref().map(|hint| MeetingPresenceHint {
                app_label: hint.app_label.clone(),
                confidence: hint.confidence,
                evidence: hint.evidence.clone(),
            });
            let prompt = data.prompt.as_ref().map(PendingPrompt::to_ipc);
            (hint, prompt)
        };
        let enabled = self.inner.enabled.load(Ordering::Relaxed);
        let paused = self.inner.paused.load(Ordering::Relaxed);
        let mode = if !enabled {
            MeetingPresenceMode::Off
        } else if paused {
            MeetingPresenceMode::Paused
        } else {
            MeetingPresenceMode::Detecting
        };
        MeetingPresenceSnapshot {
            enabled,
            paused,
            start_with_windows: self.inner.start_with_windows.load(Ordering::Relaxed),
            mode,
            hint,
            prompt,
        }
    }

    /// The recording activity this state machine last received.
    pub(crate) fn recording_active(&self) -> bool {
        self.inner.data.lock().unwrap().recording_active
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::Relaxed)
    }

    fn is_detecting(&self) -> bool {
        self.inner.enabled.load(Ordering::Relaxed) && !self.inner.paused.load(Ordering::Relaxed)
    }

    fn set_enabled_runtime(&self, enabled: bool) {
        self.inner.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            self.clear_runtime();
        }
    }

    fn set_paused_runtime(&self, paused: bool) {
        self.inner.paused.store(paused, Ordering::Relaxed);
        if paused {
            let mut data = self.inner.data.lock().unwrap();
            data.hint = None;
            data.prompt = None;
        }
    }

    fn set_start_with_windows_runtime(&self, enabled: bool) {
        self.inner
            .start_with_windows
            .store(enabled, Ordering::Relaxed);
    }

    fn clear_runtime(&self) {
        let mut data = self.inner.data.lock().unwrap();
        data.hint = None;
        data.prompt = None;
        data.sessions.clear();
    }

    fn set_recording_active(&self, active: bool) {
        let mut data = self.inner.data.lock().unwrap();
        data.recording_active = active;
        if active {
            data.prompt = None;
            data.hint = None;
            for session in data.sessions.values_mut() {
                session.prompted = true;
                session.suppressed = true;
            }
        }
    }

    /// Take the active prompt for an action. Stale or already-used ids fail and
    /// a claimed prompt's episode is suppressed.
    fn claim_prompt(&self, prompt_id: &str) -> Result<PendingPrompt, String> {
        let mut data = self.inner.data.lock().unwrap();
        let prompt = data
            .prompt
            .take()
            .ok_or_else(|| "meeting prompt is no longer active".to_string())?;
        if prompt.id != prompt_id {
            data.prompt = Some(prompt);
            return Err("meeting prompt is stale".to_string());
        }
        data.hint = None;
        if let Some(session) = data.sessions.get_mut(&prompt.process_id) {
            session.prompted = true;
            session.suppressed = true;
        }
        Ok(prompt)
    }

    /// Compensate a claimed prompt whose action failed, unless the world moved
    /// on (recording started, app exited, detection off, or a newer prompt).
    fn restore_prompt(&self, prompt: PendingPrompt) {
        let mut data = self.inner.data.lock().unwrap();
        if !data.recording_active
            && data.prompt.is_none()
            && data.sessions.contains_key(&prompt.process_id)
            && self.inner.enabled.load(Ordering::Relaxed)
            && !self.inner.paused.load(Ordering::Relaxed)
        {
            if let Some(session) = data.sessions.get_mut(&prompt.process_id) {
                session.suppressed = false;
            }
            data.prompt = Some(prompt);
        }
    }

    pub(crate) fn current_prompt(&self) -> Option<PendingPrompt> {
        self.inner.data.lock().unwrap().prompt.clone()
    }

    /// Submit a native notification for `prompt_id` at most once, and only
    /// while it is still the active prompt and nothing is recording. `show`
    /// runs under the state lock so recording startup cannot interleave; it
    /// returns whether it actually showed the notification.
    #[cfg(target_os = "windows")]
    pub(crate) fn deliver_native_prompt_once<E>(
        &self,
        prompt_id: &str,
        show: impl FnOnce() -> Result<bool, E>,
    ) -> Result<bool, E> {
        let mut data = self.inner.data.lock().unwrap();
        if !should_submit_native_prompt(&data, prompt_id) {
            return Ok(false);
        }
        let shown = show()?;
        if shown {
            data.last_notified_prompt = Some(prompt_id.to_string());
        }
        Ok(shown)
    }
}

#[cfg(target_os = "windows")]
fn should_submit_native_prompt(data: &PresenceData, prompt_id: &str) -> bool {
    !data.recording_active
        && data
            .prompt
            .as_ref()
            .is_some_and(|prompt| prompt.id == prompt_id)
        && data.last_notified_prompt.as_deref() != Some(prompt_id)
}

pub(crate) fn prompt_is_current(state: &MeetingPresenceState, prompt_id: &str) -> bool {
    state
        .inner
        .data
        .lock()
        .unwrap()
        .prompt
        .as_ref()
        .is_some_and(|prompt| prompt.id == prompt_id)
}

fn update_session_activity(session: &mut DetectionSession, active: bool) -> bool {
    if active {
        session.inactive_polls = 0;
        return false;
    }

    if !session.prompted && !session.suppressed {
        return false;
    }

    session.inactive_polls = session.inactive_polls.saturating_add(1);
    if session.inactive_polls < INACTIVE_POLLS_BEFORE_SESSION_RESET {
        return false;
    }

    session.prompted = false;
    session.suppressed = false;
    session.inactive_polls = 0;
    true
}

/// Advance the episode state machine with one detection snapshot. Returns
/// whether the visible state changed and the newly created prompt, if any.
fn update_detections(
    state: &MeetingPresenceState,
    snapshot: DetectionSnapshot,
    allow_prompt: bool,
) -> (bool, Option<PendingPrompt>) {
    let DetectionSnapshot {
        detections,
        possible_hints,
        live_process_ids,
    } = snapshot;
    let active_process_ids: HashSet<u32> = detections
        .iter()
        .map(|detection| detection.process_id)
        .collect();
    let mut prompt_to_emit = None;
    let mut changed = false;
    {
        let mut data = state.inner.data.lock().unwrap();
        data.sessions
            .retain(|process_id, _| live_process_ids.contains(process_id));

        for (process_id, session) in data.sessions.iter_mut() {
            if update_session_activity(session, active_process_ids.contains(process_id)) {
                changed = true;
            }
        }

        if let Some(prompt) = data.prompt.as_ref() {
            // Keep an unanswered prompt through one transient audio gap. The
            // episode resets after two inactive polls, or immediately on exit.
            if data
                .sessions
                .get(&prompt.process_id)
                .is_none_or(|session| !session.prompted)
            {
                data.prompt = None;
                changed = true;
            }
        }
        if let Some(prompt) = data.prompt.as_mut() {
            if let Some(detection) = detections
                .iter()
                .find(|d| d.process_id == prompt.process_id)
            {
                // A meeting window may appear after its background audio session.
                prompt.window_process_id = detection.window_process_id;
            }
        }

        let next_hint = possible_hints.iter().find(|detection| {
            data.sessions
                .get(&detection.process_id)
                .is_none_or(|session| !session.prompted && !session.suppressed)
        });
        for detection in possible_hints.iter().chain(detections.iter()) {
            data.sessions
                .entry(detection.process_id)
                .or_insert_with(|| {
                    changed = true;
                    DetectionSession {
                        prompted: false,
                        suppressed: false,
                        inactive_polls: 0,
                    }
                });
        }

        if data.recording_active {
            changed |= data.prompt.take().is_some();
            changed |= data.hint.take().is_some();
            // Track audio gaps normally, but consume every active episode seen
            // during capture so stopping does not immediately prompt again.
            for process_id in &active_process_ids {
                if let Some(session) = data.sessions.get_mut(process_id) {
                    session.prompted = true;
                    session.suppressed = true;
                }
            }
            return (changed, None);
        }

        let next_hint = next_hint.cloned();
        if data.hint != next_hint {
            data.hint = next_hint;
            changed = true;
        }
        if data.prompt.is_none() && allow_prompt {
            let next_detection = detections.iter().find(|detection| {
                data.sessions
                    .get(&detection.process_id)
                    .is_some_and(|session| !session.prompted && !session.suppressed)
            });
            if let Some(detection) = next_detection {
                if let Some(session) = data.sessions.get_mut(&detection.process_id) {
                    session.prompted = true;
                }
                data.hint = None;
                let prompt = PendingPrompt {
                    id: format!(
                        "meeting-prompt-{}",
                        state.inner.next_prompt_id.fetch_add(1, Ordering::Relaxed)
                    ),
                    process_id: detection.process_id,
                    window_process_id: detection.window_process_id,
                    app_label: detection.app_label.clone(),
                    confidence: detection.confidence,
                    evidence: detection.evidence.clone(),
                };
                data.prompt = Some(prompt.clone());
                prompt_to_emit = Some(prompt);
                changed = true;
            }
        } else if data.prompt.is_some() && data.hint.take().is_some() {
            changed = true;
        }
    }

    (changed, prompt_to_emit)
}

/// One detector poll through the probe seam.
/// `None` when detection was turned off or paused while the probe ran.
fn poll(
    state: &MeetingPresenceState,
    probe: &dyn PresenceProbe,
) -> Result<Option<(bool, Option<PendingPrompt>)>, String> {
    let snapshot = probe.detect()?;
    if !state.is_detecting() {
        return Ok(None);
    }
    let allow_prompt = !probe.prompt_is_deferred();
    Ok(Some(update_detections(state, snapshot, allow_prompt)))
}

/* ---------- Tauri glue ---------- */

/// Recording activity input, delivered by `app_lifecycle` before capture
/// startup and after startup failure or finalization.
pub(crate) fn recording_activity_changed(app: &tauri::AppHandle, active: bool) {
    if let Some(state) = app.try_state::<MeetingPresenceState>() {
        state.set_recording_active(active);
        publish(app, &state);
        #[cfg(desktop)]
        background_companion::sync_prompt_overlay(app, &state);
    }
}

/// Push the current state to the tray and to every webview.
fn publish(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    #[cfg(desktop)]
    background_companion::refresh_tray(app, state);
    emit_state(app, state);
}

fn emit_state(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    let _ = app.emit(EVENT_STATE, state.snapshot());
}

fn emit_prompt(app: &tauri::AppHandle, state: &MeetingPresenceState, prompt: &PendingPrompt) {
    let data = state.inner.data.lock().unwrap();
    if data.recording_active
        || crate::app_lifecycle::is_recording_active(app)
        || !data
            .prompt
            .as_ref()
            .is_some_and(|current| current.id == prompt.id)
    {
        return;
    }
    // Keep the lock through emission so recording startup clears this event next.
    let _ = app.emit(EVENT_PROMPT, prompt.to_ipc());
}

pub(crate) fn set_paused(app: &tauri::AppHandle, state: &MeetingPresenceState, paused: bool) {
    state.set_paused_runtime(paused);
    publish(app, state);
}

/// Publish the outcome of one detector poll.
fn deliver_poll(
    app: &tauri::AppHandle,
    state: &MeetingPresenceState,
    changed: bool,
    prompt_to_emit: Option<PendingPrompt>,
) {
    if changed {
        publish(app, state);
    }
    // Foreground/full-screen state can change without changing audio activity.
    #[cfg(desktop)]
    background_companion::sync_prompt_overlay(app, state);
    if let Some(prompt) = prompt_to_emit {
        emit_prompt(app, state, &prompt);
        #[cfg(desktop)]
        background_companion::notify_background_prompt(app, state);
    }
}

fn start_detector(
    app: tauri::AppHandle,
    state: MeetingPresenceState,
    probe: Arc<dyn PresenceProbe>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            if app
                .state::<crate::app_lifecycle::AppLifecycle>()
                .is_quitting()
            {
                break;
            }
            if state.is_detecting() {
                let poll_state = state.clone();
                let poll_probe = Arc::clone(&probe);
                let polled =
                    tauri::async_runtime::spawn_blocking(move || poll(&poll_state, &*poll_probe))
                        .await;
                match polled {
                    Ok(Ok(Some((changed, prompt)))) => deliver_poll(&app, &state, changed, prompt),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => eprintln!("[meeting-presence] detection failed: {error}"),
                    Err(error) => eprintln!("[meeting-presence] detection task failed: {error}"),
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let state = MeetingPresenceState::new();
    app.manage(state.clone());

    #[cfg(desktop)]
    background_companion::setup(app, &state)?;

    let handle = app.handle().clone();
    let load_state = state.clone();
    tauri::async_runtime::spawn(async move {
        let db_state = handle.state::<DbState>();
        match db_state.pool().await {
            Ok(pool) => {
                let enabled = settings::get_flag(&pool, ENABLED_KEY, false)
                    .await
                    .unwrap_or(false);
                let start_with_windows = settings::get_flag(&pool, START_WITH_WINDOWS_KEY, false)
                    .await
                    .unwrap_or(false);
                load_state.inner.enabled.store(enabled, Ordering::Relaxed);
                load_state.set_start_with_windows_runtime(start_with_windows);
                publish(&handle, &load_state);
            }
            Err(error) => eprintln!("[meeting-presence] settings load failed: {error}"),
        }
    });

    start_detector(app.handle().clone(), state, Arc::new(Win32Probe));
    Ok(())
}

/// Create a Note draft for a claimed prompt, restoring the prompt on failure.
async fn jot_notes(
    app: &tauri::AppHandle,
    state: &MeetingPresenceState,
    prompt: PendingPrompt,
) -> Result<i64, String> {
    let created = match app.state::<DbState>().pool().await {
        Ok(pool) => meeting_store::create_note_draft(&pool, None).await,
        Err(error) => Err(error),
    };
    match created {
        Ok(draft_id) => {
            emit_state(app, state);
            Ok(draft_id)
        }
        Err(error) => {
            state.restore_prompt(prompt);
            Err(error)
        }
    }
}

/// Hand a prompt's meeting over to recording: remember its capture target
/// and arrange the Companion layout. The frontend then starts recording.
fn start_recording_from_prompt(
    app: &tauri::AppHandle,
    state: &MeetingPresenceState,
    prompt_id: &str,
) -> Result<(), String> {
    let prompt = state.claim_prompt(prompt_id)?;
    crate::recording::queue_process_loopback_target(app, prompt.process_id);
    #[cfg(desktop)]
    {
        if let Err(error) =
            crate::window_layout::apply(app, prompt.window_process_id.unwrap_or(prompt.process_id))
        {
            // Window arrangement is a convenience around the explicit start
            // action; a platform window quirk must never block recording.
            eprintln!("[window-layout] companion layout unavailable: {error}");
            background_companion::show_main_window(app);
        }
    }
    emit_state(app, state);
    Ok(())
}

/// Apply a prompt action chosen outside the main window (a native toast).
/// Actions: `open`, `notes`, `start`, `not-now`.
pub(crate) async fn apply_prompt_action(
    app: &tauri::AppHandle,
    prompt_id: &str,
    action: &str,
) -> Result<(), String> {
    let state = app.state::<MeetingPresenceState>();
    match action {
        "open" => {
            if !prompt_is_current(&state, prompt_id) {
                return Err("meeting prompt is stale".to_string());
            }
            background_companion::show_main_window(app);
            Ok(())
        }
        "notes" => {
            let prompt = state.claim_prompt(prompt_id)?;
            background_companion::show_main_window(app);
            let draft_id = jot_notes(app, &state, prompt).await?;
            let _ = app.emit(
                EVENT_ACTION,
                serde_json::json!({ "action": "notes", "draft_id": draft_id }),
            );
            Ok(())
        }
        "start" => {
            start_recording_from_prompt(app, &state, prompt_id)?;
            let _ = app.emit(EVENT_ACTION, serde_json::json!({ "action": "start" }));
            Ok(())
        }
        "not-now" => {
            state.claim_prompt(prompt_id)?;
            emit_state(app, &state);
            Ok(())
        }
        _ => Err("unknown meeting prompt action".to_string()),
    }
}

#[tauri::command]
pub async fn get_meeting_presence_state(
    state: State<'_, MeetingPresenceState>,
) -> Result<MeetingPresenceSnapshot, String> {
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn set_meeting_presence_enabled(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    db: State<'_, DbState>,
    enabled: bool,
) -> Result<(), String> {
    settings::set_flag(&db.pool().await?, ENABLED_KEY, enabled).await?;
    state.set_enabled_runtime(enabled);
    publish(&app, &state);
    Ok(())
}

#[tauri::command]
pub async fn set_meeting_presence_paused(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    paused: bool,
) -> Result<(), String> {
    set_paused(&app, &state, paused);
    Ok(())
}

#[tauri::command]
pub async fn set_meeting_presence_start_with_windows(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    db: State<'_, DbState>,
    enabled: bool,
) -> Result<(), String> {
    background_companion::autostart::set_start_with_windows(enabled)?;
    settings::set_flag(&db.pool().await?, START_WITH_WINDOWS_KEY, enabled).await?;
    state.set_start_with_windows_runtime(enabled);
    emit_state(&app, &state);
    Ok(())
}

#[tauri::command]
pub async fn jot_notes_from_meeting_prompt(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    prompt_id: String,
) -> Result<i64, String> {
    let prompt = state.claim_prompt(&prompt_id)?;
    jot_notes(&app, &state, prompt).await
}

#[tauri::command]
pub async fn start_recording_from_meeting_prompt(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    prompt_id: String,
) -> Result<(), String> {
    start_recording_from_prompt(&app, &state, &prompt_id)
}

#[tauri::command]
pub async fn dismiss_meeting_prompt(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    prompt_id: String,
) -> Result<(), String> {
    state.claim_prompt(&prompt_id)?;
    emit_state(&app, &state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::probe::{detect_fixture, FixtureProbe};
    use super::{
        poll, prompt_is_current, update_session_activity, DetectionSession,
        MeetingPresenceConfidence, MeetingPresenceEvidence, MeetingPresenceMode,
        MeetingPresenceState, PendingPrompt,
    };

    #[test]
    #[cfg(target_os = "windows")]
    fn native_prompt_is_not_resubmitted_or_shown_after_recording_starts() {
        let state = MeetingPresenceState::new();
        let snapshot = detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], &[10]);
        let (_, prompt) = super::update_detections(&state, snapshot, true);
        let id = prompt.unwrap().id;
        {
            let mut data = state.inner.data.lock().unwrap();
            assert!(super::should_submit_native_prompt(&data, &id));
            data.last_notified_prompt = Some(id.clone());
            assert!(!super::should_submit_native_prompt(&data, &id));
            data.last_notified_prompt = None;
        }
        state.set_recording_active(true);
        assert!(!super::should_submit_native_prompt(
            &state.inner.data.lock().unwrap(),
            &id
        ));
    }

    #[test]
    fn recording_suppresses_new_meeting_prompts_and_hints() {
        let state = MeetingPresenceState::new();
        state.set_recording_active(true);
        for audio in [vec![], vec![10]] {
            let snapshot = detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], &audio);
            let (_, prompt) = super::update_detections(&state, snapshot, true);
            assert!(prompt.is_none());
            assert!(state.snapshot().prompt.is_none());
            assert!(state.snapshot().hint.is_none());
        }
    }

    #[test]
    fn recording_start_invalidates_pending_prompt_and_prevents_restore() {
        let state = MeetingPresenceState::new();
        state.set_enabled_runtime(true);
        let snapshot = detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], &[10]);
        let (_, prompt) = super::update_detections(&state, snapshot, true);
        let prompt = prompt.unwrap();
        state.set_recording_active(true);
        assert!(state.snapshot().prompt.is_none());
        assert!(!prompt_is_current(&state, &prompt.id));
        assert!(state.claim_prompt(&prompt.id).is_err());
        state.restore_prompt(prompt);
        assert!(state.snapshot().prompt.is_none());
    }

    #[test]
    fn recording_audio_gaps_do_not_reprompt_and_later_meetings_rearm() {
        let state = MeetingPresenceState::new();
        let snapshot =
            |audio: &[u32]| detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], audio);
        let (_, first) = super::update_detections(&state, snapshot(&[10]), true);
        let first = first.unwrap();
        state.claim_prompt(&first.id).unwrap();
        state.set_recording_active(true);
        for audio in [vec![], vec![], vec![10]] {
            let (_, prompt) = super::update_detections(&state, snapshot(&audio), true);
            assert!(prompt.is_none());
        }
        state.set_recording_active(false);
        let (_, prompt) = super::update_detections(&state, snapshot(&[10]), true);
        assert!(
            prompt.is_none(),
            "same meeting must remain suppressed after recording"
        );
        super::update_detections(&state, snapshot(&[]), true);
        super::update_detections(&state, snapshot(&[]), true);
        let (_, prompt) = super::update_detections(&state, snapshot(&[10]), true);
        assert_ne!(prompt.unwrap().id, first.id);
    }

    #[test]
    fn a_single_audio_gap_does_not_discard_an_unanswered_prompt() {
        let state = MeetingPresenceState::new();
        let snapshot =
            |audio: &[u32]| detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], audio);
        let (_, prompt) = super::update_detections(&state, snapshot(&[10]), true);
        let prompt_id = prompt.unwrap().id;
        super::update_detections(&state, snapshot(&[]), true);
        assert!(prompt_is_current(&state, &prompt_id));
        let (_, next) = super::update_detections(&state, snapshot(&[10]), true);
        assert!(next.is_none());
        assert!(prompt_is_current(&state, &prompt_id));
    }

    #[test]
    fn deferred_meetings_prompt_when_allowed_and_rearm_after_leaving() {
        let state = MeetingPresenceState::new();
        let snapshot =
            |audio: &[u32]| detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], audio);
        let (_, prompt) = super::update_detections(&state, snapshot(&[10]), false);
        assert!(prompt.is_none());
        let (_, prompt) = super::update_detections(&state, snapshot(&[10]), true);
        let first_id = prompt.unwrap().id;
        state.claim_prompt(&first_id).unwrap();
        super::update_detections(&state, snapshot(&[]), false);
        super::update_detections(&state, snapshot(&[]), false);
        let (_, prompt) = super::update_detections(&state, snapshot(&[10]), true);
        assert_ne!(prompt.unwrap().id, first_id);
    }

    #[test]
    fn audio_helper_restart_does_not_repeat_a_dismissed_prompt() {
        let state = MeetingPresenceState::new();
        let snapshot = |helper| {
            detect_fixture(
                &[(10, 0, "ms-teams.exe"), (helper, 10, "msedgewebview2.exe")],
                &[(10, "Teams")],
                &[helper],
            )
        };
        let (_, prompt) = super::update_detections(&state, snapshot(11), true);
        state.claim_prompt(&prompt.unwrap().id).unwrap();
        let (_, prompt) = super::update_detections(&state, snapshot(12), true);
        assert!(prompt.is_none());
        assert!(state.snapshot().prompt.is_none());
    }

    #[test]
    fn two_audio_gaps_clear_the_prompt_and_allow_a_new_meeting() {
        let state = MeetingPresenceState::new();
        let snapshot =
            |audio: &[u32]| detect_fixture(&[(10, 0, "Zoom.exe")], &[(10, "Zoom")], audio);
        let (_, prompt) = super::update_detections(&state, snapshot(&[10]), true);
        let first_id = prompt.unwrap().id;
        super::update_detections(&state, snapshot(&[]), true);
        super::update_detections(&state, snapshot(&[]), true);
        assert!(!prompt_is_current(&state, &first_id));
        assert!(state.claim_prompt(&first_id).is_err());
        let (_, prompt) = super::update_detections(&state, snapshot(&[10]), true);
        assert_ne!(prompt.unwrap().id, first_id);
    }

    #[test]
    fn exiting_the_app_immediately_invalidates_its_prompt() {
        let state = MeetingPresenceState::new();
        let (_, prompt) = super::update_detections(
            &state,
            detect_fixture(&[(10, 0, "Zoom.exe")], &[(10, "Zoom")], &[10]),
            true,
        );
        let prompt_id = prompt.unwrap().id;
        super::update_detections(&state, detect_fixture(&[], &[], &[]), true);
        assert!(!prompt_is_current(&state, &prompt_id));
    }

    #[test]
    fn a_late_meeting_window_updates_the_existing_prompt_target() {
        let state = MeetingPresenceState::new();
        let processes = &[(10, 0, "Zoom.exe"), (11, 10, "Zoom.exe")];
        let (_, prompt) =
            super::update_detections(&state, detect_fixture(processes, &[], &[10]), true);
        let prompt_id = prompt.unwrap().id;
        super::update_detections(
            &state,
            detect_fixture(processes, &[(11, "Zoom Meeting")], &[10]),
            true,
        );
        let prompt = state.claim_prompt(&prompt_id).unwrap();
        assert_eq!(prompt.process_id, 10);
        assert_eq!(prompt.window_process_id, Some(11));
    }

    #[test]
    fn suppressed_session_rearms_after_meeting_audio_gap() {
        let mut session = DetectionSession {
            prompted: true,
            suppressed: true,
            inactive_polls: 0,
        };

        assert!(!update_session_activity(&mut session, false));
        assert!(session.suppressed);
        assert!(update_session_activity(&mut session, false));
        assert!(!session.prompted);
        assert!(!session.suppressed);
    }

    #[test]
    fn snapshots_start_off_and_without_a_prompt() {
        let state = MeetingPresenceState::new();
        let snapshot = state.snapshot();
        assert_eq!(snapshot.mode, MeetingPresenceMode::Off);
        assert!(!snapshot.enabled);
        assert!(snapshot.prompt.is_none());
    }

    #[test]
    fn prompt_actions_are_single_use_and_reject_stale_ids() {
        let state = MeetingPresenceState::new();
        {
            let mut data = state.inner.data.lock().unwrap();
            data.sessions.insert(
                42,
                DetectionSession {
                    prompted: true,
                    suppressed: false,
                    inactive_polls: 0,
                },
            );
            data.prompt = Some(PendingPrompt {
                id: "prompt-current".into(),
                process_id: 42,
                window_process_id: Some(42),
                app_label: "Granola".into(),
                confidence: MeetingPresenceConfidence::Likely,
                evidence: vec![
                    MeetingPresenceEvidence::AppOrVisibleWindow,
                    MeetingPresenceEvidence::ActiveCoreAudio,
                ],
            });
        }

        assert!(prompt_is_current(&state, "prompt-current"));
        assert!(!prompt_is_current(&state, "prompt-old"));
        assert!(state.claim_prompt("prompt-old").is_err());
        assert!(state.claim_prompt("prompt-current").is_ok());
        assert!(!prompt_is_current(&state, "prompt-current"));
        assert!(state.claim_prompt("prompt-current").is_err());
    }

    #[test]
    fn probe_polls_create_prompts_only_when_the_foreground_allows_them() {
        let state = MeetingPresenceState::new();
        state.set_enabled_runtime(true);
        let mut probe = FixtureProbe {
            processes: vec![(10, 0, "ms-teams.exe"), (11, 10, "msedgewebview2.exe")],
            windows: vec![(10, "Microsoft Teams")],
            audio: vec![11],
            deferred: true,
        };
        let (changed, prompt) = poll(&state, &probe).unwrap().expect("detecting");
        assert!(changed, "the new episode is tracked while deferred");
        assert!(prompt.is_none());
        assert!(state.snapshot().prompt.is_none());

        probe.deferred = false;
        let (_, prompt) = poll(&state, &probe).unwrap().expect("detecting");
        let prompt = prompt.expect("prompt once the foreground allows it");
        let snapshot = state.snapshot();
        let shown = snapshot.prompt.expect("prompt is visible in the snapshot");
        assert_eq!(shown.id, prompt.id);
        assert_eq!(shown.app_label, "Microsoft Teams");
        assert_eq!(
            shown.evidence,
            vec![
                MeetingPresenceEvidence::AppOrVisibleWindow,
                MeetingPresenceEvidence::ActiveCoreAudio
            ]
        );

        state.set_paused_runtime(true);
        assert!(
            poll(&state, &probe).unwrap().is_none(),
            "a paused detector ignores the probe result"
        );
    }
}
