mod asr;
mod db;
mod export;
mod llm;
mod loopback;
mod meeting_presence;
mod models;
mod recording;
mod recording_session;
mod resampler;
mod shortcuts;
mod startup_coordination;
mod window_layout;
// Parked for now: sherpa-onnx endpointing drives the transcript, so nothing
// consumes VAD output yet. Kept compiled for reuse (e.g. a speaking indicator).
#[allow(dead_code)]
mod vad;

use std::sync::atomic::{AtomicBool, Ordering};

use startup_coordination::{
    coordinate_startup, emit_local_diagnostic, StartupCoordination, StartupCoordinationConfig,
    WindowsStartupMutex,
};
use tauri::{Emitter, Manager};

// The real-process harness opts into this PID file so it can wait for `.setup()`
// deterministically before sending native window messages.
fn signal_startup_test_ready() {
    let Some(path) = std::env::var_os("KIMINOLA_STARTUP_TEST_READY_FILE") else {
        return;
    };
    if let Err(error) = std::fs::write(&path, std::process::id().to_string()) {
        eprintln!(
            "[startup-test] could not write setup readiness signal to {}: {error}",
            std::path::Path::new(&path).display()
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivationIntent {
    Ordinary,
    Background,
}

impl ActivationIntent {
    fn from_args(args: &[String]) -> Self {
        if args.iter().any(|arg| arg == "--background") {
            Self::Background
        } else {
            Self::Ordinary
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivationDecision {
    ShowNow,
    Queued,
    Ignored,
}

#[derive(Default)]
struct ActivationState {
    requested: AtomicBool,
    setup_complete: AtomicBool,
}

impl ActivationState {
    fn request(&self, intent: ActivationIntent) -> ActivationDecision {
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

    fn finish_setup(&self) -> bool {
        self.setup_complete.store(true, Ordering::SeqCst);
        self.requested.swap(false, Ordering::SeqCst)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let startup_mutex = WindowsStartupMutex;
    let startup_config = StartupCoordinationConfig::default();
    let startup_coordination = coordinate_startup(&startup_mutex, &startup_config);
    match &startup_coordination {
        StartupCoordination::Coordinated(_lease) => {}
        StartupCoordination::Degraded(diagnostic) => emit_local_diagnostic(diagnostic),
    }

    let app = tauri::Builder::default()
        .manage(ActivationState::default())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let intent = ActivationIntent::from_args(&args);
            let state = app.state::<ActivationState>();
            let decision = state.request(intent);
            if decision == ActivationDecision::ShowNow {
                meeting_presence::show_main_window(app);
            }
        }))
        .manage(shortcuts::ShortcutState::new())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    use std::str::FromStr;
                    use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if let Some(current) = app
                        .state::<shortcuts::ShortcutState>()
                        .current
                        .lock()
                        .unwrap()
                        .as_ref()
                    {
                        if let Ok(current) = Shortcut::from_str(current) {
                            if shortcut == &current {
                                let _ = app.emit("shortcut:triggered", ());
                            }
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            recording::setup(app);
            db::setup(app);
            shortcuts::setup(app)?;
            meeting_presence::setup(app)?;
            if std::env::args().any(|arg| arg == "--background") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            if app.state::<ActivationState>().finish_setup() {
                meeting_presence::show_main_window(app.handle());
            }
            signal_startup_test_ready();
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(focused) if window.label() == "main" => {
                if let Some(state) = window
                    .app_handle()
                    .try_state::<meeting_presence::MeetingPresenceState>()
                {
                    #[cfg(desktop)]
                    if *focused {
                        meeting_presence::sync_prompt_overlay(window.app_handle(), &state);
                    } else {
                        meeting_presence::notify_background_prompt(window.app_handle(), &state);
                    }
                }
            }
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if let Some(state) = window
                    .app_handle()
                    .try_state::<meeting_presence::MeetingPresenceState>()
                {
                    if !state.is_quitting() {
                        api.prevent_close();
                        let _ = window.hide();
                        #[cfg(desktop)]
                        meeting_presence::notify_background_prompt(window.app_handle(), &state);
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            recording::start_recording,
            recording::stop_recording,
            recording::pause_recording,
            recording::resume_recording,
            db::save_meeting,
            db::list_meetings,
            db::get_meeting,
            db::rename_meeting,
            db::list_spaces,
            db::list_library_tree,
            db::create_space,
            db::rename_space,
            db::move_library_node,
            db::update_notes,
            db::update_segment_text,
            db::delete_segment,
            db::list_templates,
            db::create_template,
            db::update_template,
            db::delete_template,
            db::search_meetings,
            db::is_onboarding_complete,
            db::set_onboarding_complete,
            db::create_note_draft,
            db::list_note_drafts,
            db::get_note_draft,
            db::update_note_draft,
            db::update_note_draft_recovery,
            db::delete_note_draft,
            meeting_presence::get_meeting_presence_state,
            meeting_presence::set_meeting_presence_enabled,
            meeting_presence::set_meeting_presence_paused,
            meeting_presence::set_meeting_presence_start_with_windows,
            meeting_presence::jot_notes_from_meeting_prompt,
            meeting_presence::start_recording_from_meeting_prompt,
            meeting_presence::dismiss_meeting_prompt,
            models::download_model_pack,
            models::check_model_pack,
            models::check_microphone_permission,
            models::open_model_folder,
            llm::get_llm_config,
            llm::set_llm_config,
            llm::test_llm_config,
            llm::enhance_meeting,
            export::export_notes_markdown,
            export::export_transcript_text,
            export::save_notes_export,
            export::save_transcript_export,
            shortcuts::get_global_shortcut,
            shortcuts::set_global_shortcut,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    drop(startup_coordination);
    app.run(|_, _| {});
}

#[cfg(all(test, target_os = "windows"))]
mod windows_test_runtime {
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FreeLibrary(module: *mut c_void) -> i32;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
    }

    #[test]
    fn common_controls_v6_is_active_for_test_harness() {
        let library_name: Vec<u16> = OsStr::new("comctl32.dll")
            .encode_wide()
            .chain(Some(0))
            .collect();

        // The tray/menu dependency imports this symbol when Common Controls
        // v6 is enabled. This assertion catches a test executable that was
        // linked without the activation manifest required to resolve it.
        let module = unsafe { LoadLibraryW(library_name.as_ptr()) };
        assert!(!module.is_null(), "could not load comctl32.dll");

        let symbol = b"TaskDialogIndirect\0";
        let entry_point = unsafe { GetProcAddress(module, symbol.as_ptr()) };
        unsafe {
            FreeLibrary(module);
        }
        assert!(
            !entry_point.is_null(),
            "Common Controls v6 is not active for this test executable"
        );
    }
}

#[cfg(test)]
mod startup_wiring_tests {
    use super::{ActivationDecision, ActivationIntent, ActivationState};

    #[test]
    fn ordinary_activation_before_setup_is_replayed_after_setup() {
        let state = ActivationState::default();

        assert_eq!(
            state.request(ActivationIntent::Ordinary),
            ActivationDecision::Queued
        );
        assert!(state.finish_setup());
    }

    #[test]
    fn ordinary_activation_after_setup_shows_immediately() {
        let state = ActivationState::default();
        assert!(!state.finish_setup());

        assert_eq!(
            state.request(ActivationIntent::Ordinary),
            ActivationDecision::ShowNow
        );
    }

    #[test]
    fn background_activation_is_ignored_before_and_after_setup() {
        let state = ActivationState::default();

        assert_eq!(
            state.request(ActivationIntent::Background),
            ActivationDecision::Ignored
        );
        assert!(!state.finish_setup());
        assert_eq!(
            state.request(ActivationIntent::Background),
            ActivationDecision::Ignored
        );
    }

    #[test]
    fn activation_intent_is_derived_from_relaunch_arguments() {
        assert_eq!(
            ActivationIntent::from_args(&["kiminola.exe".to_string()]),
            ActivationIntent::Ordinary
        );
        assert_eq!(
            ActivationIntent::from_args(&["kiminola.exe".to_string(), "--background".to_string(),]),
            ActivationIntent::Background
        );
    }
}
