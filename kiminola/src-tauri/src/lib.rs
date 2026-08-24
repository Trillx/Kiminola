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
mod window_layout;
// Parked for now: sherpa-onnx endpointing drives the transcript, so nothing
// consumes VAD output yet. Kept compiled for reuse (e.g. a speaking indicator).
#[allow(dead_code)]
mod vad;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Emitter, Manager};

#[cfg(target_os = "windows")]
struct StartupLock(isize);

#[cfg(target_os = "windows")]
impl StartupLock {
    fn acquire() -> Result<Self, String> {
        use windows::core::w;
        use windows::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            CreateMutexW, WaitForSingleObject, INFINITE,
        };

        let handle = unsafe {
            CreateMutexW(
                None,
                false,
                w!("Local\\com.kiminola.app-startup-lock"),
            )
        }
        .map_err(|error| format!("could not create startup lock: {error}"))?;

        let wait_result = unsafe { WaitForSingleObject(handle, INFINITE) };
        if wait_result == WAIT_OBJECT_0 || wait_result == WAIT_ABANDONED {
            Ok(Self(handle.0 as isize))
        } else {
            let _ = unsafe { CloseHandle(handle) };
            Err(format!("could not acquire startup lock: {wait_result:?}"))
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for StartupLock {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::Threading::ReleaseMutex;

        let handle = HANDLE(self.0 as _);
        let _ = unsafe { ReleaseMutex(handle) };
        let _ = unsafe { CloseHandle(handle) };
    }
}

#[cfg(not(target_os = "windows"))]
struct StartupLock;

#[cfg(not(target_os = "windows"))]
impl StartupLock {
    fn acquire() -> Result<Self, String> {
        Ok(Self)
    }
}

#[derive(Default)]
struct ActivationState {
    requested: AtomicBool,
    setup_complete: AtomicBool,
}

impl ActivationState {
    fn request(&self) -> bool {
        self.requested.store(true, Ordering::SeqCst);
        self.setup_complete.load(Ordering::SeqCst)
    }

    fn finish_setup(&self) -> bool {
        self.setup_complete.store(true, Ordering::SeqCst);
        self.requested.swap(false, Ordering::SeqCst)
    }

    fn mark_handled(&self) {
        self.requested.store(false, Ordering::SeqCst);
    }
}

fn should_activate_existing_instance(args: &[String]) -> bool {
    !args.iter().any(|arg| arg == "--background")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let startup_lock = StartupLock::acquire().expect("could not coordinate application startup");

    let app = tauri::Builder::default()
        .manage(ActivationState::default())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if should_activate_existing_instance(&args) {
                let state = app.state::<ActivationState>();
                if state.request() {
                    meeting_presence::show_main_window(app);
                    state.mark_handled();
                }
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
                    if let Some(current) = app.state::<shortcuts::ShortcutState>().current.lock().unwrap().as_ref() {
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

    drop(startup_lock);
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
    use super::{should_activate_existing_instance, ActivationState};

    #[test]
    fn activation_before_setup_is_replayed_after_setup() {
        let state = ActivationState::default();

        assert!(!state.request());
        assert!(state.finish_setup());
    }

    #[test]
    fn ordinary_second_launch_activates_existing_instance() {
        let args = vec!["kiminola.exe".to_string()];

        assert!(should_activate_existing_instance(&args));
    }

    #[test]
    fn background_second_launch_keeps_existing_instance_hidden() {
        let args = vec!["kiminola.exe".to_string(), "--background".to_string()];

        assert!(!should_activate_existing_instance(&args));
    }

    #[test]
    fn bootstrap_registers_single_instance_before_setup() {
        let manifest = include_str!("../Cargo.toml");
        let source = include_str!("lib.rs");
        let dependency = ["tauri-plugin-single-", "instance"].concat();
        let registration = [".plugin(tauri_plugin_single_", "instance::init"].concat();

        assert!(
            manifest.contains(&dependency),
            "the single-instance plugin must be a runtime dependency"
        );

        let registration_index = source
            .find(&registration)
            .expect("the app builder must register the single-instance plugin");
        let setup_index = source
            .find(".setup(|app|")
            .expect("the app builder must retain its setup hook");
        assert!(
            registration_index < setup_index,
            "the single-instance plugin must be registered before setup"
        );
    }
}
