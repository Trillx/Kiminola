mod app_lifecycle;
mod asr;
mod audio;
mod background_companion;
mod db;
mod db_safety;
mod export;
mod llm;
mod meeting_presence;
mod models;
mod migrations;
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

use background_companion::ActivationState;
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
        .manage(app_lifecycle::AppLifecycle::default())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            background_companion::handle_relaunch(app, &args);
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
            if let Err(error) = shortcuts::setup(app) {
                eprintln!("[shortcuts] startup settings unavailable: {error}");
            }
            meeting_presence::setup(app)?;
            background_companion::finish_startup(app);
            signal_startup_test_ready();
            Ok(())
        })
        .on_window_event(background_companion::handle_window_event)
        .invoke_handler(tauri::generate_handler![
            db::commands::database_status,
            db::commands::retry_database,
            db::commands::restore_database_backup,
            app_lifecycle::prepare_app_update,
            app_lifecycle::cancel_app_update,
            recording::start_recording,
            recording::stop_recording,
            recording::pause_recording,
            recording::resume_recording,
            db::commands::save_meeting,
            db::commands::list_meetings,
            db::commands::get_meeting,
            db::commands::rename_meeting,
            db::commands::list_spaces,
            db::commands::list_library_tree,
            db::commands::create_space,
            db::commands::rename_space,
            db::commands::move_library_node,
            db::commands::update_notes,
            db::commands::update_segment_text,
            db::commands::delete_segment,
            db::commands::list_templates,
            db::commands::create_template,
            db::commands::update_template,
            db::commands::delete_template,
            db::commands::search_meetings,
            db::commands::is_onboarding_complete,
            db::commands::set_onboarding_complete,
            db::commands::create_note_draft,
            db::commands::list_note_drafts,
            db::commands::get_note_draft,
            db::commands::update_note_draft,
            db::commands::update_note_draft_recovery,
            db::commands::delete_note_draft,
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
            llm::list_openrouter_models,
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
    use crate::background_companion::{ActivationDecision, ActivationIntent, ActivationState};

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
