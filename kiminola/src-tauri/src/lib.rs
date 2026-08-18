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
// Parked for now: sherpa-onnx endpointing drives the transcript, so nothing
// consumes VAD output yet. Kept compiled for reuse (e.g. a speaking indicator).
#[allow(dead_code)]
mod vad;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        .manage(shortcuts::ShortcutState::new())
        .plugin(tauri_plugin_opener::init())
        // Updater wiring (pubkey, endpoints, signing — SPEC §Updates) lands with
        // the packaging ticket; the plugin refuses to init without a config block.
        // .plugin(tauri_plugin_updater::Builder::new().build())
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
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if let Some(state) = window
                    .app_handle()
                    .try_state::<meeting_presence::MeetingPresenceState>()
                {
                    if !state.is_quitting() {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
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
            db::create_space,
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
