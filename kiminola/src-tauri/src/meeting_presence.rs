//! Opt-in meeting-presence hints and the background companion lifecycle.
//!
//! Detection is deliberately advisory. It requires a visible/known app
//! signal and an active Core Audio session, and none of the prompt actions
//! start recording in this module.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::db::{self, DbState};

const ENABLED_KEY: &str = "meeting_presence_enabled";
const START_WITH_WINDOWS_KEY: &str = "meeting_presence_start_with_windows";
const EVENT_STATE: &str = "meeting-presence:state";
const EVENT_PROMPT: &str = "meeting-presence:prompt";
const EVENT_ACTION: &str = "meeting-presence:action";
#[cfg(desktop)]
const PROMPT_WINDOW_LABEL: &str = "meeting-prompt";
#[cfg(target_os = "windows")]
const TOAST_APPLICATION_ID: &str = "com.kiminola.app";
const PROMPT_MESSAGE: &str = "You may be in a meeting. Want to jot notes?";
const PROMPT_NOT_RECORDING_MESSAGE: &str = "Kimi Nola is not recording.";
const INACTIVE_POLLS_BEFORE_SESSION_RESET: u8 = 2;

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
struct PendingPrompt {
    id: String,
    process_id: u32,
    app_label: String,
    confidence: MeetingPresenceConfidence,
    evidence: Vec<MeetingPresenceEvidence>,
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
    hint: Option<Detection>,
    prompt: Option<PendingPrompt>,
    sessions: HashMap<u32, DetectionSession>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Detection {
    process_id: u32,
    app_label: String,
    confidence: MeetingPresenceConfidence,
    evidence: Vec<MeetingPresenceEvidence>,
}

#[derive(Debug)]
struct DetectionSnapshot {
    detections: Vec<Detection>,
    possible_hints: Vec<Detection>,
    live_process_ids: HashSet<u32>,
}

pub struct MeetingPresenceState {
    inner: Arc<Inner>,
}

struct Inner {
    enabled: AtomicBool,
    paused: AtomicBool,
    start_with_windows: AtomicBool,
    quitting: AtomicBool,
    next_prompt_id: AtomicU64,
    data: Mutex<PresenceData>,
    #[cfg(desktop)]
    tray_status: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    #[cfg(desktop)]
    tray_toggle: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
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
                quitting: AtomicBool::new(false),
                next_prompt_id: AtomicU64::new(1),
                data: Mutex::new(PresenceData::default()),
                #[cfg(desktop)]
                tray_status: Mutex::new(None),
                #[cfg(desktop)]
                tray_toggle: Mutex::new(None),
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
            let prompt = data.prompt.as_ref().map(|prompt| MeetingPrompt {
                id: prompt.id.clone(),
                app_label: prompt.app_label.clone(),
                message: PROMPT_MESSAGE.to_string(),
                not_recording_message: PROMPT_NOT_RECORDING_MESSAGE.to_string(),
                confidence: prompt.confidence,
                evidence: prompt.evidence.clone(),
            });
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

    pub(crate) fn is_quitting(&self) -> bool {
        self.inner.quitting.load(Ordering::Relaxed)
    }

    pub fn set_quitting(&self) {
        self.inner.quitting.store(true, Ordering::Relaxed);
    }

    fn set_enabled_runtime(&self, enabled: bool) {
        self.inner.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            self.clear_runtime();
        }
        self.update_tray();
    }

    fn set_paused_runtime(&self, paused: bool) {
        self.inner.paused.store(paused, Ordering::Relaxed);
        if paused {
            let mut data = self.inner.data.lock().unwrap();
            data.hint = None;
            data.prompt = None;
        }
        self.update_tray();
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

    fn restore_prompt(&self, prompt: PendingPrompt) {
        let mut data = self.inner.data.lock().unwrap();
        if data.prompt.is_none()
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

    fn update_tray(&self) {
        #[cfg(desktop)]
        {
            let snapshot = self.snapshot();
            let status = match snapshot.mode {
                MeetingPresenceMode::Detecting => "Detecting locally · not recording",
                MeetingPresenceMode::Paused => "Paused",
                MeetingPresenceMode::Off => "Off",
            };
            if let Some(item) = self.inner.tray_status.lock().unwrap().as_ref() {
                let _ = item.set_text(status);
            }
            if let Some(item) = self.inner.tray_toggle.lock().unwrap().as_ref() {
                let _ = item.set_text(if snapshot.paused {
                    "Resume detection"
                } else {
                    "Pause detection"
                });
                let _ = item.set_enabled(snapshot.enabled);
            }
        }
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
    let pool = db::ensure_pool(&db.pool).await?;
    persist_bool(&pool, ENABLED_KEY, enabled).await?;
    state.set_enabled_runtime(enabled);
    emit_state(&app, &state);
    Ok(())
}

#[tauri::command]
pub async fn set_meeting_presence_paused(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    paused: bool,
) -> Result<(), String> {
    state.set_paused_runtime(paused);
    emit_state(&app, &state);
    Ok(())
}

#[tauri::command]
pub async fn set_meeting_presence_start_with_windows(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    db: State<'_, DbState>,
    enabled: bool,
) -> Result<(), String> {
    set_start_with_windows_windows(enabled)?;
    let pool = db::ensure_pool(&db.pool).await?;
    persist_bool(&pool, START_WITH_WINDOWS_KEY, enabled).await?;
    state.set_start_with_windows_runtime(enabled);
    emit_state(&app, &state);
    Ok(())
}

#[tauri::command]
pub async fn jot_notes_from_meeting_prompt(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    db: State<'_, DbState>,
    prompt_id: String,
) -> Result<i64, String> {
    let prompt = state.claim_prompt(&prompt_id)?;
    let pool = db::ensure_pool(&db.pool).await?;
    let draft_id = match db::create_note_draft_impl(&pool).await {
        Ok(id) => id,
        Err(error) => {
            state.restore_prompt(prompt);
            return Err(error);
        }
    };
    emit_state(&app, &state);
    Ok(draft_id)
}

#[tauri::command]
pub async fn start_recording_from_meeting_prompt(
    app: tauri::AppHandle,
    state: State<'_, MeetingPresenceState>,
    prompt_id: String,
) -> Result<(), String> {
    start_recording_from_prompt(&app, &state, &prompt_id)
}

fn start_recording_from_prompt(
    app: &tauri::AppHandle,
    state: &MeetingPresenceState,
    prompt_id: &str,
) -> Result<(), String> {
    let prompt = state.claim_prompt(prompt_id)?;
    crate::recording::queue_process_loopback_target(app, prompt.process_id);
    #[cfg(desktop)]
    {
        show_main_window(app);
        if let Err(error) = crate::window_layout::apply(app, prompt.process_id) {
            // Window arrangement is a convenience around the explicit start
            // action; a platform window quirk must never block recording.
            eprintln!("[window-layout] companion layout unavailable: {error}");
        }
    }
    emit_state(app, state);
    Ok(())
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

async fn persist_bool(pool: &sqlx::SqlitePool, key: &str, value: bool) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(if value { "1" } else { "0" })
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn read_bool(
    pool: &sqlx::SqlitePool,
    key: &str,
    default: bool,
) -> Result<bool, String> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(value
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(default))
}

fn emit_state(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    let _ = app.emit(EVENT_STATE, state.snapshot());
}

fn emit_prompt(app: &tauri::AppHandle, prompt: &PendingPrompt) {
    let _ = app.emit(
        EVENT_PROMPT,
        MeetingPrompt {
            id: prompt.id.clone(),
            app_label: prompt.app_label.clone(),
            message: PROMPT_MESSAGE.to_string(),
            not_recording_message: PROMPT_NOT_RECORDING_MESSAGE.to_string(),
            confidence: prompt.confidence,
            evidence: prompt.evidence.clone(),
        },
    );
}

#[cfg(desktop)]
fn should_show_background_prompt(main_visible: bool, main_minimized: bool, main_focused: bool) -> bool {
    !main_visible || main_minimized || !main_focused
}

#[cfg(desktop)]
fn main_needs_background_prompt(app: &tauri::AppHandle) -> bool {
    let Some(main) = app.get_webview_window("main") else {
        return true;
    };

    should_show_background_prompt(
        main.is_visible().unwrap_or(false),
        main.is_minimized().unwrap_or(false),
        main.is_focused().unwrap_or(false),
    )
}

#[cfg(desktop)]
pub(crate) fn sync_prompt_overlay(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    let Some(window) = app.get_webview_window(PROMPT_WINDOW_LABEL) else {
        eprintln!("[meeting-presence] prompt overlay window is unavailable");
        return;
    };

    if state.snapshot().prompt.is_none() || !main_needs_background_prompt(app) {
        let _ = window.hide();
        return;
    }

    if let (Ok(Some(monitor)), Ok(size)) = (window.current_monitor(), window.outer_size()) {
        let work_area = monitor.work_area();
        let margin = 20;
        let x = work_area.position.x + work_area.size.width as i32 - size.width as i32 - margin;
        let y = work_area.position.y + work_area.size.height as i32 - size.height as i32 - margin;
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x, y)));
    }

    let _ = window.show();
}

#[cfg(desktop)]
pub(crate) fn notify_background_prompt(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    sync_prompt_overlay(app, state);
    if !main_needs_background_prompt(app) {
        return;
    }

    let prompt = state.inner.data.lock().unwrap().prompt.clone();
    if let Some(prompt) = prompt {
        show_native_prompt(app, &prompt);
    }
}

fn prompt_is_current(state: &MeetingPresenceState, prompt_id: &str) -> bool {
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

fn apply_detections(
    app: &tauri::AppHandle,
    state: &MeetingPresenceState,
    snapshot: DetectionSnapshot,
) {
    if !state.inner.enabled.load(Ordering::Relaxed)
        || state.inner.paused.load(Ordering::Relaxed)
    {
        return;
    }

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
            if !active_process_ids.contains(&prompt.process_id) {
                data.prompt = None;
                changed = true;
            }
        }

        let next_hint = possible_hints.iter().find(|detection| {
            data.sessions
                .get(&detection.process_id)
                .is_none_or(|session| !session.prompted && !session.suppressed)
        });
        for detection in possible_hints.iter().chain(detections.iter()) {
            data.sessions.entry(detection.process_id).or_insert_with(|| {
                changed = true;
                DetectionSession {
                    prompted: false,
                    suppressed: false,
                    inactive_polls: 0,
                }
            });
        }

        let next_hint = next_hint.cloned();
        if data.hint != next_hint {
            data.hint = next_hint;
            changed = true;
        }
        if data.prompt.is_none() {
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
                    app_label: detection.app_label.clone(),
                    confidence: detection.confidence,
                    evidence: detection.evidence.clone(),
                };
                data.prompt = Some(prompt.clone());
                prompt_to_emit = Some(prompt);
                changed = true;
            }
        } else {
            if data.hint.take().is_some() {
                changed = true;
            }
        }
    }

    if changed {
        state.update_tray();
        emit_state(app, state);
        #[cfg(desktop)]
        sync_prompt_overlay(app, state);
    }
    if let Some(prompt) = prompt_to_emit {
        emit_prompt(app, &prompt);
        #[cfg(desktop)]
        notify_background_prompt(app, state);
    }
}

fn start_detector(app: tauri::AppHandle, state: MeetingPresenceState) {
    tauri::async_runtime::spawn(async move {
        loop {
            if state.is_quitting() {
                break;
            }
            if state.inner.enabled.load(Ordering::Relaxed)
                && !state.inner.paused.load(Ordering::Relaxed)
            {
                match tauri::async_runtime::spawn_blocking(detect_windows).await {
                    Ok(Ok(snapshot)) if !prompt_is_deferred() => {
                        apply_detections(&app, &state, snapshot)
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => eprintln!("[meeting-presence] detection failed: {error}"),
                    Err(error) => eprintln!("[meeting-presence] detection task failed: {error}"),
                }
            }
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
    });
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    if let Err(error) = ensure_toast_shortcut() {
        eprintln!("[meeting-presence] toast shortcut unavailable: {error}");
    }

    let state = MeetingPresenceState::new();
    app.manage(state.clone());

    #[cfg(desktop)]
    setup_tray(app, state.clone())?;

    let handle = app.handle().clone();
    let load_state = state.clone();
    tauri::async_runtime::spawn(async move {
        let db_state = handle.state::<DbState>();
        match db::ensure_pool(&db_state.pool).await {
            Ok(pool) => {
                let enabled = read_bool(&pool, ENABLED_KEY, false).await.unwrap_or(false);
                let start_with_windows = read_bool(&pool, START_WITH_WINDOWS_KEY, false)
                    .await
                    .unwrap_or(false);
                load_state.inner.enabled.store(enabled, Ordering::Relaxed);
                load_state
                    .inner
                    .start_with_windows
                    .store(start_with_windows, Ordering::Relaxed);
                load_state.update_tray();
                emit_state(&handle, &load_state);
            }
            Err(error) => eprintln!("[meeting-presence] settings load failed: {error}"),
        }
    });

    start_detector(app.handle().clone(), state);
    Ok(())
}

#[cfg(desktop)]
fn setup_tray(
    app: &mut tauri::App,
    state: MeetingPresenceState,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let status = MenuItemBuilder::with_id("presence-status", "Off")
        .enabled(false)
        .build(app)?;
    let toggle = MenuItemBuilder::with_id("presence-toggle", "Pause detection")
        .enabled(false)
        .build(app)?;
    let open = MenuItemBuilder::with_id("presence-open", "Open Kimi Nola").build(app)?;
    let quit = MenuItemBuilder::with_id("presence-quit", "Quit Kimi Nola").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&status, &toggle, &open, &quit])
        .build()?;
    let icon = tray_icon()?;
    state.inner.tray_status.lock().unwrap().replace(status);
    state.inner.tray_toggle.lock().unwrap().replace(toggle);
    let event_state = state.clone();
    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("Kimi Nola")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "presence-toggle" => {
                event_state.set_paused_runtime(!event_state.inner.paused.load(Ordering::Relaxed));
                emit_state(app, &event_state);
            }
            "presence-open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "presence-quit" => {
                event_state.set_quitting();
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    state.update_tray();
    Ok(())
}

#[cfg(desktop)]
fn tray_icon() -> Result<tauri::image::Image<'static>, Box<dyn std::error::Error>> {
    use std::io::Cursor;

    // This is the hand-placed, approved sub-24px mono mark from the brand
    // system. Decode it once at startup instead of recoloring or redrawing it.
    const TRAY_ICON_BYTES: &[u8] = include_bytes!("../../branding/icons/favicon-16.png");
    let decoder = png::Decoder::new(Cursor::new(TRAY_ICON_BYTES));
    let mut reader = decoder.read_info()?;
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("approved tray asset must decode as 8-bit RGBA".into());
    }
    rgba.truncate(info.buffer_size());
    Ok(tauri::image::Image::new_owned(rgba, info.width, info.height))
}

#[cfg(target_os = "windows")]
fn ensure_toast_shortcut() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::{Interface, PCWSTR, PROPVARIANT};
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok();
    if let Err(error) = result {
        return Err(format!("COM initialization failed: {error}"));
    }

    let result = (|| {
        let appdata = std::env::var_os("APPDATA")
            .ok_or_else(|| "APPDATA is unavailable".to_string())?;
        let shortcut_dir = std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        std::fs::create_dir_all(&shortcut_dir).map_err(|error| error.to_string())?;

        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let shortcut = shortcut_dir.join("Kimi Nola.lnk");
        let executable_wide = wide(executable.as_os_str());
        let shortcut_wide = wide(shortcut.as_os_str());
        let description_wide = wide(std::ffi::OsStr::new("Local meeting notes"));

        let shell_link: IShellLinkW = unsafe {
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
        }
        .map_err(|error| error.to_string())?;
        unsafe {
            shell_link
                .SetPath(PCWSTR(executable_wide.as_ptr()))
                .map_err(|error| error.to_string())?;
            shell_link
                .SetDescription(PCWSTR(description_wide.as_ptr()))
                .map_err(|error| error.to_string())?;
        }

        let property_store: IPropertyStore = shell_link.cast().map_err(|error| error.to_string())?;
        let app_id: PROPVARIANT = TOAST_APPLICATION_ID.into();
        unsafe {
            property_store
                .SetValue(&PKEY_AppUserModel_ID, &app_id)
                .map_err(|error| error.to_string())?;
            property_store.Commit().map_err(|error| error.to_string())?;
        }

        let persist_file: IPersistFile = shell_link.cast().map_err(|error| error.to_string())?;
        unsafe {
            persist_file
                .Save(PCWSTR(shortcut_wide.as_ptr()), BOOL(1))
                .map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    })();
    unsafe { CoUninitialize() };
    result
}

#[cfg(target_os = "windows")]
fn show_native_prompt(app: &tauri::AppHandle, prompt: &PendingPrompt) {
    let app = app.clone();
    let prompt = prompt.clone();
    std::thread::spawn(move || {
        if let Err(error) = show_native_prompt_inner(&app, &prompt) {
            // Unpackaged NSIS installs may not have an AppUserModelID yet. The
            // app-owned background overlay remains the actionable fallback; do
            // not focus or restore the main window from a notification error.
            eprintln!("[meeting-presence] native toast unavailable: {error}");
            #[cfg(desktop)]
            if let Some(state) = app.try_state::<MeetingPresenceState>() {
                sync_prompt_overlay(&app, &state);
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn show_native_prompt(_app: &tauri::AppHandle, prompt: &PendingPrompt) {
    eprintln!("[meeting-presence] prompt={} app={}", prompt.id, prompt.app_label);
}

#[cfg(target_os = "windows")]
fn show_native_prompt_inner(
    app: &tauri::AppHandle,
    prompt: &PendingPrompt,
) -> windows::core::Result<()> {
    use windows::core::{HSTRING, IInspectable, Interface};
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastFailedEventArgs, ToastNotification, ToastNotificationManager,
    };
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let result = (|| {
        let application_id = HSTRING::from(TOAST_APPLICATION_ID);
        if let Err(error) = unsafe { SetCurrentProcessExplicitAppUserModelID(&application_id) } {
            eprintln!(
                "[meeting-presence] process AppUserModelID could not be set HRESULT={:#010x}",
                error.code().0 as u32
            );
        }
        let document = windows::Data::Xml::Dom::XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(toast_xml(prompt)))?;
        let notification = ToastNotification::CreateToastNotification(&document)?;
        notification.SetTag(&HSTRING::from(prompt.id.as_str()))?;

        let notifier = match ToastNotificationManager::CreateToastNotifierWithId(
            &application_id,
        ) {
            Ok(notifier) => notifier,
            Err(identity_error) => {
                eprintln!(
                    "[meeting-presence] toast identity rejected HRESULT={:#010x}; trying default notifier",
                    identity_error.code().0 as u32
                );
                ToastNotificationManager::CreateToastNotifier()?
            }
        };

        let current_prompt = prompt.id.clone();
        let callback_app = app.clone();
        let activated_handler: TypedEventHandler<ToastNotification, IInspectable> =
            TypedEventHandler::new(
                move |_toast: &Option<ToastNotification>, args: &Option<IInspectable>| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let activation: ToastActivatedEventArgs = args.cast()?;
                    let arguments = activation.Arguments()?.to_string_lossy();
                    let (prompt_id, action) = parse_toast_arguments(&arguments);
                    if prompt_id != current_prompt {
                        eprintln!("[meeting-presence] rejected stale native toast action");
                        return Ok(());
                    }
                    let app = callback_app.clone();
                    let prompt_id = prompt_id.to_string();
                    let action = action.to_string();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) =
                            apply_native_prompt_action(&app, &prompt_id, &action).await
                        {
                            eprintln!("[meeting-presence] native toast action rejected: {error}");
                        }
                    });
                    Ok(())
                },
            );
        notification.Activated(&activated_handler)?;
        let failed_handler: TypedEventHandler<ToastNotification, ToastFailedEventArgs> =
            TypedEventHandler::new(
                move |_toast: &Option<ToastNotification>, args: &Option<ToastFailedEventArgs>| {
                    eprintln!(
                        "[meeting-presence] native toast failed details_present={}",
                        args.is_some()
                    );
                    Ok(())
                },
            );
        notification.Failed(&failed_handler)?;
        eprintln!(
            "[meeting-presence] native toast submitted prompt={} setting={:?}",
            prompt.id,
            notifier.Setting().ok()
        );
        notifier.Show(&notification)?;

        // Keep the WinRT notification and handler alive long enough for a
        // normal toast click. Prompt validity is still checked against the
        // in-memory state before any action is applied.
        std::thread::sleep(Duration::from_secs(120));
        Ok::<(), windows::core::Error>(())
    })();
    unsafe { RoUninitialize() };
    result
}

fn toast_xml(prompt: &PendingPrompt) -> String {
    let app_label = xml_escape(&prompt.app_label);
    let prompt_message = xml_escape(PROMPT_MESSAGE);
    let not_recording_message = xml_escape(PROMPT_NOT_RECORDING_MESSAGE);

    format!(
        r#"<toast duration="long" launch="kiminola://meeting-prompt?prompt={}&amp;action=open">
  <visual>
    <binding template="ToastGeneric">
      <text>{}</text>
      <text>{}</text>
      <text>{}</text>
    </binding>
  </visual>
  <actions>
    <action content="Jot notes" arguments="kiminola://meeting-prompt?prompt={}&amp;action=notes" activationType="foreground" />
    <action content="Start recording" arguments="kiminola://meeting-prompt?prompt={}&amp;action=start" activationType="foreground" />
    <action content="Not now" arguments="kiminola://meeting-prompt?prompt={}&amp;action=not-now" activationType="foreground" />
  </actions>
</toast>"#,
        prompt.id,
        app_label,
        prompt_message,
        not_recording_message,
        prompt.id,
        prompt.id,
        prompt.id,
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn parse_toast_arguments(arguments: &str) -> (&str, &str) {
    let query = arguments
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default();
    let mut prompt_id = "";
    let mut action = "";
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "prompt" => prompt_id = value,
            "action" => action = value,
            _ => {}
        }
    }
    (prompt_id, action)
}

#[cfg(desktop)]
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(target_os = "windows")]
async fn apply_native_prompt_action(
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
            show_main_window(app);
            Ok(())
        }
        "notes" => {
            let prompt = state.claim_prompt(prompt_id)?;
            show_main_window(app);
            let db_state = app.state::<DbState>();
            let pool = db::ensure_pool(&db_state.pool).await?;
            let draft_id = match db::create_note_draft_impl(&pool).await {
                Ok(id) => id,
                Err(error) => {
                    state.restore_prompt(prompt);
                    return Err(error);
                }
            };
            emit_state(app, &state);
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

#[cfg(target_os = "windows")]
fn detect_windows() -> Result<DetectionSnapshot, String> {
    let processes = enumerate_processes()?;
    let windows = enumerate_visible_windows()?;
    let active_audio = enumerate_active_audio_processes()?;
    let current_pid = std::process::id();
    let mut detections = Vec::new();
    let mut possible_hints = Vec::new();
    let mut possible_seen = HashSet::new();
    let mut seen = HashSet::new();

    for (&pid, process_name) in &processes {
        if pid == current_pid || is_kiminola_process(process_name) {
            continue;
        }
        let titles = windows.get(&pid).cloned().unwrap_or_default();
        let label = friendly_app_label(process_name, &titles).to_string();
        if label != "another app" && possible_seen.insert(pid) {
            possible_hints.push(Detection {
                process_id: pid,
                app_label: label,
                confidence: MeetingPresenceConfidence::Possible,
                evidence: vec![MeetingPresenceEvidence::AppOrVisibleWindow],
            });
        }
    }

    for pid in active_audio {
        if pid == current_pid {
            continue;
        }
        let Some(process_name) = processes.get(&pid) else {
            continue;
        };
        let titles = windows.get(&pid).cloned().unwrap_or_default();
        if titles.is_empty() && !is_known_meeting_process(process_name) {
            continue;
        }
        if is_kiminola_process(process_name) {
            continue;
        }
        let label = friendly_app_label(process_name, &titles).to_string();
        if seen.insert(pid) {
            detections.push(Detection {
                process_id: pid,
                app_label: label,
                confidence: MeetingPresenceConfidence::Likely,
                evidence: vec![
                    MeetingPresenceEvidence::AppOrVisibleWindow,
                    MeetingPresenceEvidence::ActiveCoreAudio,
                ],
            });
        }
    }
    Ok(DetectionSnapshot {
        detections,
        possible_hints,
        live_process_ids: processes.keys().copied().collect(),
    })
}

#[cfg(target_os = "windows")]
fn prompt_is_deferred() -> bool {
    use std::mem::size_of;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    };

    unsafe {
        if let Ok(notification_state) = SHQueryUserNotificationState() {
            if notification_state == QUNS_PRESENTATION_MODE
                || notification_state == QUNS_RUNNING_D3D_FULL_SCREEN
            {
                return true;
            }
        }

        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return false;
        }

        let mut foreground_pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        if foreground_pid == std::process::id() {
            return false;
        }

        let mut window_rect = RECT::default();
        if GetWindowRect(foreground, &mut window_rect).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
        if monitor.0.is_null() {
            return false;
        }

        let mut monitor_info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
            return false;
        }

        let monitor_rect = monitor_info.rcMonitor;
        window_rect.left <= monitor_rect.left
            && window_rect.top <= monitor_rect.top
            && window_rect.right >= monitor_rect.right
            && window_rect.bottom >= monitor_rect.bottom
    }
}

#[cfg(not(target_os = "windows"))]
fn detect_windows() -> Result<DetectionSnapshot, String> {
    Ok(DetectionSnapshot {
        detections: Vec::new(),
        possible_hints: Vec::new(),
        live_process_ids: HashSet::new(),
    })
}

#[cfg(not(target_os = "windows"))]
fn prompt_is_deferred() -> bool {
    false
}

fn is_kiminola_process(process_name: &str) -> bool {
    let name = process_name.to_ascii_lowercase();
    name == "kiminola" || name == "kiminola.exe" || name.contains("kiminola")
}

fn is_known_meeting_process(process_name: &str) -> bool {
    let name = process_name
        .rsplit_once('\\')
        .map(|(_, name)| name)
        .unwrap_or(process_name)
        .trim_end_matches(".exe")
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "granola"
            | "zoom"
            | "zoomphone"
            | "teams"
            | "ms-teams"
            | "msteams"
            | "webex"
            | "webexmta"
            | "webexhost"
    )
}

fn friendly_app_label(process_name: &str, window_titles: &[String]) -> &'static str {
    let name = process_name
        .rsplit_once('\\')
        .map(|(_, name)| name)
        .unwrap_or(process_name)
        .trim_end_matches(".exe")
        .to_ascii_lowercase();
    match name.as_str() {
        "granola" => "Granola",
        "zoom" | "zoomphone" => "Zoom",
        "teams" | "ms-teams" | "msteams" => "Microsoft Teams",
        "webex" | "webexmta" | "webexhost" => "Webex",
        "chrome" | "msedge" | "firefox" | "brave" => {
            if window_titles.iter().any(|title| {
                let title = title.to_ascii_lowercase();
                title.contains("google meet") || title.contains("meet.google.com")
            }) {
                "Google Meet"
            } else {
                "another app"
            }
        }
        _ => "another app",
    }
}

#[cfg(target_os = "windows")]
fn enumerate_processes() -> Result<HashMap<u32, String>, String> {
    use std::mem::size_of;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|e| format!("process snapshot failed: {e}"))?;
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut processes = HashMap::new();
        let first = Process32FirstW(snapshot, &mut entry);
        if first.is_ok() {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(entry.szExeFile.len());
                processes.insert(
                    entry.th32ProcessID,
                    String::from_utf16_lossy(&entry.szExeFile[..end]),
                );
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        Ok(processes)
    }
}

#[cfg(target_os = "windows")]
fn enumerate_visible_windows() -> Result<HashMap<u32, Vec<String>>, String> {
    use windows::Win32::Foundation::{BOOL, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
    };

    unsafe extern "system" fn callback(
        hwnd: windows::Win32::Foundation::HWND,
        lparam: LPARAM,
    ) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        if pid == 0 {
            return BOOL(1);
        }
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return BOOL(1);
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let written = GetWindowTextW(hwnd, &mut buffer);
        let title = String::from_utf16_lossy(&buffer[..written.max(0) as usize]);
        if !title.trim().is_empty() {
            let windows = &mut *(lparam.0 as *mut HashMap<u32, Vec<String>>);
            windows.entry(pid).or_default().push(title);
        }
        BOOL(1)
    }

    let mut windows = HashMap::new();
    unsafe {
        EnumWindows(
            Some(callback),
            LPARAM(&mut windows as *mut _ as isize),
        )
        .map_err(|e| format!("window enumeration failed: {e}"))?;
    }
    Ok(windows)
}

#[cfg(target_os = "windows")]
fn enumerate_active_audio_processes() -> Result<HashSet<u32>, String> {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eConsole, eRender, AudioSessionStateActive,
        IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| format!("COM init failed: {e}"))?;
        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("audio enumerator failed: {e}"))?;
            let mut pids = HashSet::new();
            for (flow, role) in [(eRender, eConsole), (eCapture, eCommunications)] {
                let Ok(device) = enumerator.GetDefaultAudioEndpoint(flow, role) else {
                    continue;
                };
                let Ok(manager): Result<IAudioSessionManager2, _> =
                    device.Activate(CLSCTX_ALL, None)
                else {
                    continue;
                };
                let Ok(sessions) = manager.GetSessionEnumerator() else {
                    continue;
                };
                let Ok(count) = sessions.GetCount() else {
                    continue;
                };
                for index in 0..count {
                    let Ok(control) = sessions.GetSession(index) else {
                        continue;
                    };
                    let Ok(control2) = control.cast::<windows::Win32::Media::Audio::IAudioSessionControl2>()
                    else {
                        continue;
                    };
                    if control.GetState().ok() == Some(AudioSessionStateActive) {
                        if let Ok(pid) = control2.GetProcessId() {
                            if pid != 0 {
                                pids.insert(pid);
                            }
                        }
                    }
                }
            }
            Ok::<HashSet<u32>, String>(pids)
        })();
        CoUninitialize();
        result
    }
}

#[cfg(target_os = "windows")]
fn set_start_with_windows_windows(enabled: bool) -> Result<(), String> {
    use std::mem::size_of;
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };

    let exe = std::env::current_exe().map_err(|e| format!("current exe unavailable: {e}"))?;
    let mut key = windows::Win32::System::Registry::HKEY::default();
    unsafe {
        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_SET_VALUE,
            &mut key,
        );
        if opened.0 != 0 {
            return Err(format!("could not open Windows startup settings: {opened:?}"));
        }

        let result = if enabled {
            let value = format!("\"{}\" --background\0", exe.display());
            let utf16: Vec<u16> = value.encode_utf16().collect();
            let bytes = std::slice::from_raw_parts(
                utf16.as_ptr() as *const u8,
                utf16.len() * size_of::<u16>(),
            );
            RegSetValueExW(key, w!("KimiNola"), 0, REG_SZ, Some(bytes))
        } else {
            RegDeleteValueW(key, w!("KimiNola"))
        };
        let _ = RegCloseKey(key);
        if !enabled && (result.0 == 0 || result.0 == 2) {
            return Ok(());
        }
        if result.0 != 0 {
            return Err(format!("could not update Windows startup settings: {result:?}"));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn set_start_with_windows_windows(_enabled: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        friendly_app_label, is_known_meeting_process, DetectionSession, MeetingPresenceConfidence,
        MeetingPresenceEvidence, MeetingPresenceMode, MeetingPresenceState, PendingPrompt,
        prompt_is_current, should_show_background_prompt, toast_xml, update_session_activity,
        PROMPT_MESSAGE,
        PROMPT_NOT_RECORDING_MESSAGE,
    };

    #[cfg(desktop)]
    #[test]
    fn background_prompt_is_required_when_main_is_unavailable_to_user() {
        assert!(should_show_background_prompt(false, false, false));
        assert!(should_show_background_prompt(true, true, false));
        assert!(should_show_background_prompt(true, false, false));
        assert!(!should_show_background_prompt(true, false, true));
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
    fn toast_xml_shows_meeting_context_not_internal_prompt_id() {
        let prompt = PendingPrompt {
            id: "meeting-prompt-1".to_string(),
            process_id: 42,
            app_label: "Microsoft Teams".to_string(),
            confidence: MeetingPresenceConfidence::Likely,
            evidence: Vec::new(),
        };

        let xml = toast_xml(&prompt);
        assert!(xml.contains("<text>Microsoft Teams</text>"));
        assert!(xml.contains(PROMPT_MESSAGE));
        assert!(xml.contains(PROMPT_NOT_RECORDING_MESSAGE));
        assert!(!xml.contains("<text>meeting-prompt-1</text>"));
        assert!(xml.contains("prompt=meeting-prompt-1"));
    }

    #[test]
    fn known_apps_use_friendly_labels() {
        assert_eq!(friendly_app_label("Granola.exe", &[]), "Granola");
        assert_eq!(friendly_app_label("teams.exe", &[]), "Microsoft Teams");
        assert_eq!(
            friendly_app_label("chrome.exe", &["Google Meet · Project sync".into()]),
            "Google Meet"
        );
    }

    #[test]
    fn unknown_apps_never_expose_process_names() {
        assert_eq!(friendly_app_label("some-private-app.exe", &["Room".into()]), "another app");
        assert!(!is_known_meeting_process("some-private-app.exe"));
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
}
