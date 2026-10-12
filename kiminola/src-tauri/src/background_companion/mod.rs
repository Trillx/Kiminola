//! The Background companion: OatWave's resident shell after the main window
//! closes. It never captures audio.
//!
//! Owns:
//! - main-window activation (single-instance relaunches, tray "Open", toast
//!   actions) via [`ActivationState`] and [`show_main_window`];
//! - the close/hide policy for windows (decided by `app_lifecycle`);
//! - prompt delivery outside the main window: the always-available overlay
//!   window and native toasts ([`toast`]);
//! - the tray menu, including quit gating ([`tray`]);
//! - the start-with-Windows registration ([`autostart`]).
//!
//! Meeting prompt *state* lives in `crate::meeting_presence`; this module only
//! presents it and forwards user choices back.

pub(crate) mod autostart;
mod toast;
#[cfg(desktop)]
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager;

use crate::app_lifecycle::{AppLifecycle, CloseDecision};
use crate::meeting_presence::MeetingPresenceState;

#[cfg(desktop)]
const PROMPT_WINDOW_LABEL: &str = "meeting-prompt";

/* ---------- activation ---------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationIntent {
    Ordinary,
    Background,
}

impl ActivationIntent {
    pub(crate) fn from_args(args: &[String]) -> Self {
        if args.iter().any(|arg| arg == "--background") {
            Self::Background
        } else {
            Self::Ordinary
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationDecision {
    ShowNow,
    Queued,
    Ignored,
}

/// Replays an ordinary relaunch that arrives before setup finished; a
/// `--background` relaunch never surfaces the main window.
#[derive(Default)]
pub(crate) struct ActivationState {
    requested: AtomicBool,
    setup_complete: AtomicBool,
}

impl ActivationState {
    pub(crate) fn request(&self, intent: ActivationIntent) -> ActivationDecision {
        if intent == ActivationIntent::Background {
            return ActivationDecision::Ignored;
        }

        self.requested.store(true, Ordering::SeqCst);
        let setup_complete = self.setup_complete.load(Ordering::SeqCst);
        if setup_complete && self.requested.swap(false, Ordering::SeqCst) {
            ActivationDecision::ShowNow
        } else {
            ActivationDecision::Queued
        }
    }

    /// Returns whether a queued activation must be shown now.
    pub(crate) fn finish_setup(&self) -> bool {
        self.setup_complete.store(true, Ordering::SeqCst);
        self.requested.swap(false, Ordering::SeqCst)
    }
}

/// Single-instance callback: a second launch activates this process.
pub(crate) fn handle_relaunch(app: &tauri::AppHandle, args: &[String]) {
    let intent = ActivationIntent::from_args(args);
    if app.state::<ActivationState>().request(intent) == ActivationDecision::ShowNow {
        show_main_window(app);
    }
}

/// End of `.setup()`: start hidden for `--background`, then replay any
/// activation that arrived during setup.
pub(crate) fn finish_startup(app: &tauri::App) {
    if std::env::args().any(|arg| arg == "--background") {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    }
    if app.state::<ActivationState>().finish_setup() {
        show_main_window(app.handle());
    }
}

pub(crate) fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/* ---------- window policy ---------- */

pub(crate) fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    match event {
        tauri::WindowEvent::Focused(focused) if window.label() == "main" => {
            if let Some(state) = window.app_handle().try_state::<MeetingPresenceState>() {
                #[cfg(desktop)]
                if *focused {
                    sync_prompt_overlay(window.app_handle(), &state);
                } else {
                    notify_background_prompt(window.app_handle(), &state);
                }
                #[cfg(not(desktop))]
                let _ = (focused, state);
            }
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            let app = window.app_handle();
            if let Some(state) = app.try_state::<MeetingPresenceState>() {
                let decision = app.state::<AppLifecycle>().close_requested();
                if decision == CloseDecision::HideToBackground {
                    api.prevent_close();
                    let _ = window.hide();
                    #[cfg(desktop)]
                    notify_background_prompt(app, &state);
                    #[cfg(not(desktop))]
                    let _ = state;
                }
            }
        }
        _ => {}
    }
}

/* ---------- prompt delivery ---------- */

#[cfg(desktop)]
fn should_show_background_prompt(
    main_visible: bool,
    main_minimized: bool,
    main_focused: bool,
) -> bool {
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

fn prompt_is_deferred() -> bool {
    crate::meeting_presence::probe::foreground_defers_prompts()
}

/// Show the overlay prompt window only while there is an active prompt, the
/// main window is not in front, nothing records and no full-screen app is up.
#[cfg(desktop)]
pub(crate) fn sync_prompt_overlay(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    let Some(window) = app.get_webview_window(PROMPT_WINDOW_LABEL) else {
        eprintln!("[meeting-presence] prompt overlay window is unavailable");
        return;
    };

    if crate::app_lifecycle::is_recording_active(app)
        || state.snapshot().prompt.is_none()
        || !main_needs_background_prompt(app)
        || prompt_is_deferred()
    {
        let _ = window.hide();
        return;
    }

    if let (Ok(Some(monitor)), Ok(size)) = (window.current_monitor(), window.outer_size()) {
        let work_area = monitor.work_area();
        let margin = 20;
        let x = work_area.position.x + work_area.size.width as i32 - size.width as i32 - margin;
        let y = work_area.position.y + work_area.size.height as i32 - size.height as i32 - margin;
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x, y,
        )));
    }

    let _ = window.show();
}

/// Deliver the active prompt outside the main window: overlay plus a native
/// toast when the main window is unavailable to the user.
#[cfg(desktop)]
pub(crate) fn notify_background_prompt(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    sync_prompt_overlay(app, state);
    if state.is_paused()
        || crate::app_lifecycle::is_recording_active(app)
        || !main_needs_background_prompt(app)
        || prompt_is_deferred()
    {
        return;
    }

    if let Some(prompt) = state.current_prompt() {
        toast::show_native_prompt(app, &prompt);
    }
}

/// Mirror Meeting presence state into the tray menu.
#[cfg(desktop)]
pub(crate) fn refresh_tray(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    tray::refresh(app, state);
}

/// Install toast identity and the tray. Called from Meeting presence setup.
#[cfg(desktop)]
pub(crate) fn setup(
    app: &mut tauri::App,
    state: &MeetingPresenceState,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    if let Err(error) = toast::ensure_toast_shortcut() {
        eprintln!("[meeting-presence] toast shortcut unavailable: {error}");
    }
    tray::setup(app, state)
}

#[cfg(test)]
mod tests {
    #[cfg(desktop)]
    #[test]
    fn background_prompt_is_required_when_main_is_unavailable_to_user() {
        use super::should_show_background_prompt;
        assert!(should_show_background_prompt(false, false, false));
        assert!(should_show_background_prompt(true, true, false));
        assert!(should_show_background_prompt(true, false, false));
        assert!(!should_show_background_prompt(true, false, true));
    }
}
