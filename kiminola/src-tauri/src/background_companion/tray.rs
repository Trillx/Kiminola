//! Tray menu: detection status, pause/resume, open, and quit. Quit is gated:
//! during a Meeting Capture session it surfaces the main window and emits
//! `recording:quit-blocked` instead of exiting; otherwise Dictation resolves
//! any pending review before the capture-gated quit.

use std::sync::Mutex;

use tauri::menu::MenuItem;
use tauri::{Emitter, Manager, Wry};

use crate::meeting_presence::{self, MeetingPresenceMode, MeetingPresenceState};

const EVENT_RECORDING_QUIT_BLOCKED: &str = "recording:quit-blocked";

#[derive(Default)]
struct TrayItems {
    status: Mutex<Option<MenuItem<Wry>>>,
    toggle: Mutex<Option<MenuItem<Wry>>>,
}

pub(super) fn setup(
    app: &mut tauri::App,
    state: &MeetingPresenceState,
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
    let items = TrayItems::default();
    items.status.lock().unwrap().replace(status);
    items.toggle.lock().unwrap().replace(toggle);
    app.manage(items);
    let event_state = state.clone();
    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("Kimi Nola")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "presence-toggle" => {
                meeting_presence::set_paused(app, &event_state, !event_state.is_paused());
            }
            "presence-open" => super::show_main_window(app),
            "presence-quit" => {
                if crate::app_lifecycle::is_recording_active(app) {
                    super::show_main_window(app);
                    let _ = app.emit(EVENT_RECORDING_QUIT_BLOCKED, ());
                    return;
                }
                // Dictation may need an exit decision (pending review) first;
                // it then quits through the capture-gated lifecycle path.
                tauri::async_runtime::spawn(crate::dictation::request_quit(app.clone()));
            }
            _ => {}
        })
        .build(app)?;
    refresh(app.handle(), state);
    Ok(())
}

pub(super) fn refresh(app: &tauri::AppHandle, state: &MeetingPresenceState) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let snapshot = state.snapshot();
    let status = match snapshot.mode {
        MeetingPresenceMode::Detecting if state.recording_active() => {
            "Detecting locally · recording"
        }
        MeetingPresenceMode::Detecting => "Detecting locally · not recording",
        MeetingPresenceMode::Paused => "Paused",
        MeetingPresenceMode::Off => "Off",
    };
    if let Some(item) = items.status.lock().unwrap().as_ref() {
        let _ = item.set_text(status);
    }
    if let Some(item) = items.toggle.lock().unwrap().as_ref() {
        let _ = item.set_text(if snapshot.paused {
            "Resume detection"
        } else {
            "Pause detection"
        });
        let _ = item.set_enabled(snapshot.enabled);
    };
}

fn tray_icon() -> Result<tauri::image::Image<'static>, Box<dyn std::error::Error>> {
    use std::io::Cursor;

    // This is the hand-placed, approved sub-24px mono mark from the brand
    // system. Decode it once at startup instead of recoloring or redrawing it.
    const TRAY_ICON_BYTES: &[u8] = include_bytes!("../../../branding/icons/favicon-16.png");
    let decoder = png::Decoder::new(Cursor::new(TRAY_ICON_BYTES));
    let mut reader = decoder.read_info()?;
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("approved tray asset must decode as 8-bit RGBA".into());
    }
    rgba.truncate(info.buffer_size());
    Ok(tauri::image::Image::new_owned(
        rgba,
        info.width,
        info.height,
    ))
}
