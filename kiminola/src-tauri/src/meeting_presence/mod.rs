//! Opt-in Meeting presence hints and the Meeting prompt lifecycle.
//!
//! Detection is deliberately advisory. It requires a known meeting app (or a
//! visible generic app with both input and output audio, sustained across
//! polls) and an active Core Audio session, and no prompt action records
//! audio by itself.
//!
//! Structure:
//! - [`classifier`]: pure snapshot → candidates classification.
//! - [`probe`]: where snapshots come from (Win32 or fixture adapters).
//! - [`MeetingPresenceState`]: the episode/prompt state machine. Each app
//!   family gets one prompt per meeting episode; an episode ends after two
//!   consecutive polls without its audio, or immediately when the app exits.
//!   Prompts are single-use and stale ids are rejected; a recycled PID starts
//!   a new episode. Recording activity is an input (received from
//!   `app_lifecycle` and Dictation): while recording, no hints or prompts
//!   exist and every active episode is consumed. Pausing hides prompts but
//!   keeps observing episode endings.
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

pub(crate) use probe::process_instance_is_current;

const ENABLED_KEY: &str = "meeting_presence_enabled";
const START_WITH_WINDOWS_KEY: &str = "meeting_presence_start_with_windows";
const EVENT_STATE: &str = "meeting-presence:state";
const EVENT_PROMPT: &str = "meeting-presence:prompt";
const EVENT_ACTION: &str = "meeting-presence:action";
const EVENT_ERROR: &str = "meeting-presence:error";
pub(crate) const PROMPT_MESSAGE: &str = "You may be in a meeting. Want to jot notes?";
pub(crate) const PROMPT_NOT_RECORDING_MESSAGE: &str = "OatWave is not recording.";
const INACTIVE_POLLS_BEFORE_SESSION_RESET: u8 = 2;
/// Generic (non-targeted) apps must qualify on consecutive complete polls.
const GENERIC_POLLS_BEFORE_PROMPT: u8 = 2;
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
    // Creation time of `process_id` when readable; guards against PID reuse.
    process_started_at: Option<u64>,
    targeted: bool,
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
            process_started_at: Some(1),
            targeted: true,
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
    /// Consecutive complete polls on which this family qualified.
    qualified_polls: u8,
    process_started_at: Option<u64>,
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
            hint: if paused { None } else { hint },
            prompt: if paused { None } else { prompt },
        }
    }

    /// The recording activity this state machine last received.
    pub(crate) fn recording_active(&self) -> bool {
        self.inner.data.lock().unwrap().recording_active
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::Relaxed)
    }

    /// Whether the detector polls. Pause hides prompts but still observes
    /// episode endings; only disabling detection stops local metadata polling.
    fn is_enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::Relaxed)
    }

    fn set_enabled_runtime(&self, enabled: bool) {
        self.inner.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            self.clear_runtime();
        }
    }

    fn set_paused_runtime(&self, paused: bool) {
        let mut data = self.inner.data.lock().unwrap();
        let was_paused = self.inner.paused.swap(paused, Ordering::Relaxed);
        if was_paused && !paused {
            // Resume unanswered prompts, not stale notification actions.
            if let Some(prompt) = data.prompt.as_mut() {
                prompt.id = format!(
                    "meeting-prompt-{}",
                    self.inner.next_prompt_id.fetch_add(1, Ordering::Relaxed)
                );
            }
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

    /// A failed poll proves nothing about audio: restart onset and gap counts.
    fn note_detection_failure(&self) {
        for session in self.inner.data.lock().unwrap().sessions.values_mut() {
            session.qualified_polls = 0;
            session.inactive_polls = 0;
        }
    }

    #[cfg(test)]
    fn set_recording_active(&self, active: bool) {
        self.update_recording_active(|| active);
    }

    /// `active` is evaluated under the state lock so it cannot race a poll.
    fn update_recording_active(&self, active: impl FnOnce() -> bool) {
        let mut data = self.inner.data.lock().unwrap();
        let active = active();
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
        if self.inner.paused.load(Ordering::Relaxed) {
            return Err("meeting prompts are paused".to_string());
        }
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
            && data
                .sessions
                .get(&prompt.process_id)
                .is_some_and(|session| {
                    session.prompted && session.process_started_at == prompt.process_started_at
                })
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
        if self.inner.paused.load(Ordering::Relaxed)
            || !should_submit_native_prompt(&data, prompt_id)
        {
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
    if state.inner.paused.load(Ordering::Relaxed) {
        return false;
    }
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
    let allow_prompt = allow_prompt && !state.inner.paused.load(Ordering::Relaxed);
    let DetectionSnapshot {
        detections,
        possible_hints,
        live_process_instances,
        active_audio_process_ids,
        audio_scan_complete,
    } = snapshot;
    let qualified_process_ids: HashSet<u32> = detections
        .iter()
        .map(|detection| detection.process_id)
        .collect();
    let mut prompt_to_emit = None;
    let mut changed = false;
    {
        let mut data = state.inner.data.lock().unwrap();
        data.sessions.retain(|process_id, session| {
            live_process_instances
                .get(process_id)
                .is_some_and(|current| {
                    match (session.process_started_at, *current) {
                        (Some(old), Some(new)) if old != new => false,
                        (None, Some(new)) => {
                            // A later readable creation time establishes the identity
                            // that future polls must compare before preserving state.
                            session.process_started_at = Some(new);
                            changed = true;
                            true
                        }
                        // An unreadable current creation time is unknown, not proof
                        // that a previously identified process was replaced.
                        _ => true,
                    }
                })
        });

        if let Some(prompt) = data.prompt.as_mut() {
            if prompt.process_started_at.is_none() {
                if let Some(started_at) = live_process_instances
                    .get(&prompt.process_id)
                    .copied()
                    .flatten()
                {
                    prompt.process_started_at = Some(started_at);
                    changed = true;
                }
            }
        }

        for (process_id, session) in data.sessions.iter_mut() {
            let active = active_audio_process_ids.contains(process_id);
            if (audio_scan_complete || active) && update_session_activity(session, active) {
                changed = true;
            }
            if !audio_scan_complete {
                session.inactive_polls = 0;
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
        for process_id in possible_hints
            .iter()
            .chain(detections.iter())
            .map(|d| d.process_id)
            .chain(active_audio_process_ids.iter().copied())
        {
            data.sessions.entry(process_id).or_insert_with(|| {
                changed = true;
                DetectionSession {
                    prompted: false,
                    suppressed: false,
                    inactive_polls: 0,
                    qualified_polls: 0,
                    process_started_at: live_process_instances.get(&process_id).copied().flatten(),
                }
            });
        }

        for (process_id, session) in data.sessions.iter_mut() {
            session.qualified_polls =
                if audio_scan_complete && qualified_process_ids.contains(process_id) {
                    session.qualified_polls.saturating_add(1)
                } else {
                    0
                };
        }

        if data.recording_active {
            changed |= data.prompt.take().is_some();
            changed |= data.hint.take().is_some();
            // Track audio gaps normally, but consume every active episode seen
            // during capture so stopping does not immediately prompt again.
            for process_id in &active_audio_process_ids {
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
        // A generic audio app must not monopolize the single prompt slot when
        // a recognized meeting app joins later. Leave the old episode consumed
        // so it cannot immediately return, and retire its action ID.
        if allow_prompt
            && data.prompt.as_ref().is_some_and(|p| !p.targeted)
            && detections.iter().any(|detection| {
                detection.targeted
                    && data
                        .sessions
                        .get(&detection.process_id)
                        .is_some_and(|session| !session.prompted && !session.suppressed)
            })
        {
            data.prompt = None;
            changed = true;
        }
        if data.prompt.is_none() && allow_prompt {
            let next_detection = detections.iter().find(|detection| {
                data.sessions
                    .get(&detection.process_id)
                    .is_some_and(|session| {
                        !session.prompted
                            && !session.suppressed
                            && (detection.targeted
                                || session.qualified_polls >= GENERIC_POLLS_BEFORE_PROMPT)
                    })
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
                    process_started_at: live_process_instances
                        .get(&detection.process_id)
                        .copied()
                        .flatten(),
                    targeted: detection.targeted,
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
/// `None` when detection was turned off while the probe ran. A paused
/// detector still tracks episodes but never creates or shows prompts.
fn poll(
    state: &MeetingPresenceState,
    probe: &dyn PresenceProbe,
) -> Result<Option<(bool, Option<PendingPrompt>)>, String> {
    let snapshot = probe.detect()?;
    if !state.is_enabled() {
        return Ok(None);
    }
    let allow_prompt = !probe.prompt_is_deferred();
    Ok(Some(update_detections(state, snapshot, allow_prompt)))
}

/* ---------- Tauri glue ---------- */

/// Recording activity input, delivered by `app_lifecycle` (Meetings) and
/// Dictation before capture startup and after startup failure or
/// finalization. Any busy capture lease counts as active, so a repeated
/// Meeting stop cannot re-enable prompts during Dictation; release the
/// finished lease before calling this with `false`.
pub(crate) fn recording_activity_changed(app: &tauri::AppHandle, active: bool) {
    if let Some(state) = app.try_state::<MeetingPresenceState>() {
        state.update_recording_active(|| {
            active
                || app
                    .try_state::<crate::capture_gate::CaptureGate>()
                    .is_some_and(|gate| gate.is_busy())
        });
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
    if state.inner.paused.load(Ordering::Relaxed)
        || data.recording_active
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

/// Pause or resume prompts (IPC and tray). Resuming re-delivers an
/// unanswered prompt under a fresh id; pausing retracts background prompts.
pub(crate) fn set_paused(app: &tauri::AppHandle, state: &MeetingPresenceState, paused: bool) {
    state.set_paused_runtime(paused);
    publish(app, state);
    #[cfg(desktop)]
    background_companion::notify_background_prompt(app, state);
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
            // Pause hides prompts, but still observes episode endings. Only
            // disabling detection stops local process/audio metadata polling.
            if state.is_enabled() {
                let poll_state = state.clone();
                let poll_probe = Arc::clone(&probe);
                let polled =
                    tauri::async_runtime::spawn_blocking(move || poll(&poll_state, &*poll_probe))
                        .await;
                match polled {
                    Ok(Ok(Some((changed, prompt)))) => deliver_poll(&app, &state, changed, prompt),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        state.note_detection_failure();
                        eprintln!("[meeting-presence] detection failed: {error}");
                    }
                    Err(error) => {
                        state.note_detection_failure();
                        eprintln!("[meeting-presence] detection task failed: {error}");
                    }
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
                // Refresh the executable path after an installed or portable upgrade.
                if let Err(error) =
                    background_companion::autostart::set_start_with_windows(start_with_windows)
                {
                    eprintln!("[meeting-presence] startup registration failed: {error}");
                }
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
/// (only while the detected process instance still exists) and arrange the
/// Companion layout. The frontend then starts recording.
fn start_recording_from_prompt(
    app: &tauri::AppHandle,
    state: &MeetingPresenceState,
    prompt_id: &str,
) -> Result<(), String> {
    let prompt = state.claim_prompt(prompt_id)?;
    let target = match validated_capture_target(&prompt, process_instance_is_current) {
        Ok(target) => target,
        Err(error) => {
            // The prompt has been consumed, so surface the returned explanation
            // independently of the card that represented it.
            background_companion::show_main_window(app);
            let _ = app.emit(
                EVENT_ERROR,
                serde_json::json!({ "prompt_id": prompt.id, "message": error }),
            );
            emit_state(app, state);
            return Err(error);
        }
    };
    emit_state(app, state);
    if let Some((process_id, started_at)) = target {
        crate::recording::queue_process_loopback_target(app, process_id, started_at);
    } else {
        crate::recording::clear_process_loopback_target(app);
    }
    #[cfg(desktop)]
    {
        if target.is_some() {
            if let Err(error) = crate::window_layout::apply(
                app,
                prompt.window_process_id.unwrap_or(prompt.process_id),
            ) {
                // Window arrangement is a convenience around the explicit start
                // action; a platform window quirk must never block recording.
                eprintln!("[window-layout] companion layout unavailable: {error}");
                background_companion::show_main_window(app);
            }
        } else {
            // An unreadable identity cannot safely select or arrange a PID.
            background_companion::show_main_window(app);
        }
    }
    emit_state(app, state);
    Ok(())
}

/// The process-loopback target for a claimed prompt. `Ok(None)` means the
/// process identity was never readable, so capture uses the safe
/// default-output path; an identified process that exited (or whose PID was
/// reused) is an error.
fn validated_capture_target(
    prompt: &PendingPrompt,
    is_current: impl FnOnce(u32, u64) -> bool,
) -> Result<Option<(u32, u64)>, String> {
    let Some(started_at) = prompt.process_started_at else {
        // If Windows denied process-query access when the prompt was created,
        // retain the useful advisory prompt but use the safe default-output path.
        return Ok(None);
    };
    if !is_current(prompt.process_id, started_at) {
        return Err(
            "The detected app is no longer available. Please start a new recording manually."
                .to_string(),
        );
    }
    Ok(Some((prompt.process_id, started_at)))
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
    #[cfg(desktop)]
    background_companion::notify_background_prompt(&app, &state);
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
    use std::collections::HashMap;

    use super::classifier::{classify_windows, AudioActivity, ProcessIdentity, ProcessInfo};
    use super::probe::{detect_audio_fixture, detect_fixture, FixtureProbe};
    use super::{
        poll, prompt_is_current, update_session_activity, DetectionSession,
        MeetingPresenceConfidence, MeetingPresenceEvidence, MeetingPresenceMode,
        MeetingPresenceState, PendingPrompt,
    };

    #[test]
    fn recycled_pid_retires_old_actions_and_dismissal() {
        for dismissed in [false, true] {
            let state = MeetingPresenceState::new();
            let snapshot = |started_at| {
                classify_windows(
                    HashMap::from([(
                        10,
                        ProcessInfo {
                            name: "ms-teams.exe".into(),
                            parent_id: 0,
                            identity: Some(ProcessIdentity {
                                executable_path: "C:/Fixture/ms-teams.exe".into(),
                                started_at,
                            }),
                        },
                    )]),
                    HashMap::from([(10, vec!["Teams".into()])]),
                    HashMap::from([(
                        10,
                        AudioActivity {
                            input: true,
                            output: true,
                        },
                    )]),
                    999,
                )
            };
            let first = super::update_detections(&state, snapshot(1), true)
                .1
                .unwrap();
            if dismissed {
                state.claim_prompt(&first.id).unwrap();
            }
            let next = super::update_detections(&state, snapshot(2), true).1;
            assert!(
                state.claim_prompt(&first.id).is_err(),
                "PID reuse must retire the old action"
            );
            assert!(
                next.is_some(),
                "suppression must not transfer to the replacement process"
            );
        }
    }

    #[test]
    fn initially_unknown_process_identity_is_reconciled_before_pid_reuse() {
        for dismissed in [false, true] {
            let state = MeetingPresenceState::new();
            let snapshot = |started_at| {
                let mut snapshot =
                    detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], &[10]);
                snapshot.live_process_instances.insert(10, started_at);
                snapshot
            };

            let first = super::update_detections(&state, snapshot(None), true)
                .1
                .expect("targeted app should prompt");
            assert_eq!(
                state.inner.data.lock().unwrap().sessions[&10].process_started_at,
                None
            );

            super::update_detections(&state, snapshot(Some(1)), true);
            {
                let data = state.inner.data.lock().unwrap();
                assert_eq!(data.sessions[&10].process_started_at, Some(1));
                assert_eq!(data.prompt.as_ref().unwrap().process_started_at, Some(1));
            }
            if dismissed {
                state.claim_prompt(&first.id).unwrap();
            }

            let replacement = super::update_detections(&state, snapshot(Some(2)), true)
                .1
                .expect("replacement process should get a fresh prompt");
            assert!(state.claim_prompt(&first.id).is_err());
            assert_ne!(replacement.id, first.id);
        }
    }

    #[test]
    fn prompt_target_validation_falls_back_only_when_identity_was_unknown() {
        let prompt = |process_started_at| PendingPrompt {
            id: "prompt".into(),
            process_id: 10,
            process_started_at,
            targeted: true,
            window_process_id: Some(10),
            app_label: "Microsoft Teams".into(),
            confidence: MeetingPresenceConfidence::Likely,
            evidence: vec![MeetingPresenceEvidence::ActiveCoreAudio],
        };

        assert_eq!(
            super::validated_capture_target(&prompt(None), |_, _| false).unwrap(),
            None,
            "unknown identity should use safe default-output capture"
        );
        assert!(super::validated_capture_target(&prompt(Some(1)), |_, _| false).is_err());
        assert_eq!(
            super::validated_capture_target(&prompt(Some(1)), |pid, started_at| {
                pid == 10 && started_at == 1
            })
            .unwrap(),
            Some((10, 1))
        );
    }

    #[test]
    fn incomplete_audio_scans_cannot_reset_dismissal_or_confirm_generic_onset() {
        let state = MeetingPresenceState::new();
        let snapshot = |active, complete| {
            let mut snapshot = detect_fixture(
                &[(10, 0, "call-app.exe")],
                &[(10, "Call")],
                if active { &[10] } else { &[] },
            );
            snapshot.audio_scan_complete = complete;
            snapshot
        };
        super::update_detections(&state, snapshot(true, true), true);
        state.note_detection_failure();
        assert!(super::update_detections(&state, snapshot(true, true), true)
            .1
            .is_none());
        assert!(
            super::update_detections(&state, snapshot(true, false), true)
                .1
                .is_none()
        );
        assert!(super::update_detections(&state, snapshot(true, true), true)
            .1
            .is_none());
        let first = super::update_detections(&state, snapshot(true, true), true)
            .1
            .unwrap();
        state.claim_prompt(&first.id).unwrap();
        super::update_detections(&state, snapshot(false, false), true);
        super::update_detections(&state, snapshot(false, false), true);
        super::update_detections(&state, snapshot(true, true), true);
        assert!(super::update_detections(&state, snapshot(true, true), true)
            .1
            .is_none());
    }

    #[test]
    fn hidden_generic_window_does_not_end_a_dismissed_audio_episode() {
        let state = MeetingPresenceState::new();
        let snapshot = |visible| {
            detect_fixture(
                &[(10, 0, "call-app.exe")],
                if visible { &[(10, "Call")] } else { &[] },
                &[10],
            )
        };
        super::update_detections(&state, snapshot(true), true);
        let prompt = super::update_detections(&state, snapshot(true), true)
            .1
            .unwrap();
        state.claim_prompt(&prompt.id).unwrap();
        super::update_detections(&state, snapshot(false), true);
        super::update_detections(&state, snapshot(false), true);
        super::update_detections(&state, snapshot(true), true);
        assert!(
            super::update_detections(&state, snapshot(true), true)
                .1
                .is_none(),
            "window visibility is not audio inactivity"
        );
    }

    #[test]
    fn meet_title_does_not_promote_playback_only_browser_audio() {
        let state = MeetingPresenceState::new();
        let snapshot = |input| {
            detect_audio_fixture(
                &[(10, 0, "chrome.exe"), (11, 10, "chrome.exe")],
                &[(10, "Google Meet - idle tab")],
                &[(11, input, true)],
            )
        };
        for _ in 0..3 {
            assert!(
                super::update_detections(&state, snapshot(false), true)
                    .1
                    .is_none(),
                "a meeting title does not prove which tab is playing audio"
            );
        }
        assert!(super::update_detections(&state, snapshot(true), true)
            .1
            .is_none());
        assert!(super::update_detections(&state, snapshot(true), true)
            .1
            .is_some());
    }

    #[test]
    fn generic_onset_is_debounced_and_losing_one_direction_does_not_reprompt() {
        let state = MeetingPresenceState::new();
        let snapshot = |input, output| {
            detect_audio_fixture(
                &[(10, 0, "call-app.exe")],
                &[(10, "Call")],
                &[(10, input, output)],
            )
        };
        assert!(super::update_detections(&state, snapshot(true, true), true)
            .1
            .is_none());
        super::update_detections(&state, snapshot(false, false), true);
        assert!(super::update_detections(&state, snapshot(true, true), true)
            .1
            .is_none());
        let first = super::update_detections(&state, snapshot(true, true), true)
            .1
            .unwrap();
        state.claim_prompt(&first.id).unwrap();
        // Some apps close input on mute but keep output running. This is still
        // the same episode, not an invitation to nag when input returns.
        for (input, output) in [(false, true), (false, true), (true, true), (true, true)] {
            assert!(
                super::update_detections(&state, snapshot(input, output), true)
                    .1
                    .is_none()
            );
        }
        super::update_detections(&state, snapshot(false, false), true);
        super::update_detections(&state, snapshot(false, false), true);
        assert!(super::update_detections(&state, snapshot(true, true), true)
            .1
            .is_none());
        let next = super::update_detections(&state, snapshot(true, true), true)
            .1
            .unwrap();
        assert_ne!(next.id, first.id);
    }

    #[test]
    fn teams_supersedes_an_unanswered_generic_prompt_without_repeating_it() {
        let state = MeetingPresenceState::new();
        let generic = || detect_fixture(&[(1, 0, "call-app.exe")], &[(1, "Call")], &[1]);
        super::update_detections(&state, generic(), true);
        super::update_detections(&state, generic(), true);
        let old_id = state.snapshot().prompt.unwrap().id;
        let both = || {
            detect_fixture(
                &[(1, 0, "call-app.exe"), (10, 0, "ms-teams.exe")],
                &[(1, "Call"), (10, "Teams")],
                &[1, 10],
            )
        };
        let (_, prompt) = super::update_detections(&state, both(), true);
        let prompt = prompt.expect("Teams must not wait behind an unrelated unanswered prompt");
        assert_eq!(prompt.app_label, "Microsoft Teams");
        assert!(state.claim_prompt(&old_id).is_err());
        state.claim_prompt(&prompt.id).unwrap();
        let (_, next) = super::update_detections(&state, both(), true);
        assert!(
            next.is_none(),
            "the superseded generic episode must not nag again"
        );
    }

    #[test]
    fn pause_hides_and_resume_restores_an_unanswered_prompt_with_a_fresh_id() {
        let state = MeetingPresenceState::new();
        state.set_enabled_runtime(true);
        let snapshot = || detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], &[10]);
        let (_, prompt) = super::update_detections(&state, snapshot(), true);
        let old_id = prompt.unwrap().id;
        state.set_paused_runtime(true);
        assert!(state.snapshot().prompt.is_none());
        assert!(!prompt_is_current(&state, &old_id));
        assert!(state.claim_prompt(&old_id).is_err());
        super::update_detections(&state, snapshot(), false);
        assert!(state.snapshot().prompt.is_none());
        state.set_paused_runtime(false);
        let resumed = state
            .snapshot()
            .prompt
            .expect("an unanswered prompt must survive pause");
        assert_ne!(resumed.id, old_id);
        assert!(state.claim_prompt(&old_id).is_err());
        state.claim_prompt(&resumed.id).unwrap();
        assert!(super::update_detections(&state, snapshot(), true)
            .1
            .is_none());
    }

    #[test]
    fn pause_preserves_dismissal_and_tracks_meetings_ending_while_paused() {
        let state = MeetingPresenceState::new();
        state.set_enabled_runtime(true);
        let snapshot =
            |audio: &[u32]| detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], audio);
        let first = super::update_detections(&state, snapshot(&[10]), true)
            .1
            .unwrap();
        state.claim_prompt(&first.id).unwrap();
        state.set_paused_runtime(true);
        super::update_detections(&state, snapshot(&[10]), true);
        state.set_paused_runtime(false);
        assert!(
            super::update_detections(&state, snapshot(&[10]), true)
                .1
                .is_none(),
            "resume must respect Not now for the same episode"
        );
        state.set_paused_runtime(true);
        super::update_detections(&state, snapshot(&[]), true);
        super::update_detections(&state, snapshot(&[]), true);
        assert!(
            super::update_detections(&state, snapshot(&[10]), true)
                .1
                .is_none(),
            "a new episode cannot interrupt while paused"
        );
        state.set_paused_runtime(false);
        let next = super::update_detections(&state, snapshot(&[10]), true)
            .1
            .unwrap();
        assert_ne!(next.id, first.id);
    }

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
            qualified_polls: 0,
            process_started_at: Some(1),
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
                    qualified_polls: 0,
                    process_started_at: Some(1),
                },
            );
            data.prompt = Some(PendingPrompt {
                id: "prompt-current".into(),
                process_id: 42,
                process_started_at: Some(1),
                targeted: true,
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
            audio: vec![(11, true, true)],
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
        let (_, prompt) = poll(&state, &probe)
            .unwrap()
            .expect("a paused detector still observes episodes");
        assert!(prompt.is_none(), "a paused detector never prompts");
        assert!(state.snapshot().prompt.is_none());

        state.set_enabled_runtime(false);
        assert!(
            poll(&state, &probe).unwrap().is_none(),
            "a disabled detector ignores the probe result"
        );
    }
}
