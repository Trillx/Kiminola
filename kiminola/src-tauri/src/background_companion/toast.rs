//! Native Windows toast delivery for Meeting prompts.
//!
//! A toast is submitted at most once per prompt, never while recording, and
//! is retracted as soon as its prompt stops being current. Toast buttons route
//! back through `meeting_presence::apply_prompt_action`, which rejects stale
//! prompt ids. When toasts are unavailable the overlay window remains the
//! actionable fallback.

#[cfg(target_os = "windows")]
use std::time::Duration;

#[cfg(target_os = "windows")]
use tauri::Manager;

#[cfg(target_os = "windows")]
use crate::meeting_presence::{prompt_is_current, MeetingPresenceState};
use crate::meeting_presence::{PendingPrompt, PROMPT_MESSAGE, PROMPT_NOT_RECORDING_MESSAGE};

#[cfg(target_os = "windows")]
const TOAST_APPLICATION_ID: &str = "com.kiminola.app";

#[cfg(target_os = "windows")]
pub(super) fn ensure_toast_shortcut() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::{Interface, PCWSTR, PROPVARIANT};
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok();
    if let Err(error) = result {
        return Err(format!("COM initialization failed: {error}"));
    }

    let result = (|| {
        let appdata =
            std::env::var_os("APPDATA").ok_or_else(|| "APPDATA is unavailable".to_string())?;
        let shortcut_dir = std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        std::fs::create_dir_all(&shortcut_dir).map_err(|error| error.to_string())?;

        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let shortcut = shortcut_dir.join("OatWave.lnk");
        let executable_wide = wide(executable.as_os_str());
        let shortcut_wide = wide(shortcut.as_os_str());
        let description_wide = wide(std::ffi::OsStr::new("Local meeting notes"));

        let shell_link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| error.to_string())?;
        unsafe {
            shell_link
                .SetPath(PCWSTR(executable_wide.as_ptr()))
                .map_err(|error| error.to_string())?;
            shell_link
                .SetDescription(PCWSTR(description_wide.as_ptr()))
                .map_err(|error| error.to_string())?;
        }

        let property_store: IPropertyStore =
            shell_link.cast().map_err(|error| error.to_string())?;
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
pub(super) fn show_native_prompt(app: &tauri::AppHandle, prompt: &PendingPrompt) {
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
                super::sync_prompt_overlay(&app, &state);
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
pub(super) fn show_native_prompt(_app: &tauri::AppHandle, prompt: &PendingPrompt) {
    eprintln!(
        "[meeting-presence] prompt={} app={}",
        prompt.id, prompt.app_label
    );
}

#[cfg(target_os = "windows")]
fn show_native_prompt_inner(
    app: &tauri::AppHandle,
    prompt: &PendingPrompt,
) -> windows::core::Result<()> {
    use windows::core::{IInspectable, Interface, HSTRING};
    use windows::Foundation::TypedEventHandler;
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastFailedEventArgs, ToastNotification, ToastNotificationManager,
    };

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

        let notifier = match ToastNotificationManager::CreateToastNotifierWithId(&application_id) {
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
                            crate::meeting_presence::apply_prompt_action(&app, &prompt_id, &action)
                                .await
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
        let submitted = app
            .state::<MeetingPresenceState>()
            .deliver_native_prompt_once(&prompt.id, || -> windows::core::Result<bool> {
                if crate::app_lifecycle::is_recording_active(app) {
                    return Ok(false);
                }
                notifier.Show(&notification)?;
                Ok(true)
            })?;
        if !submitted {
            return Ok(());
        }
        eprintln!(
            "[meeting-presence] native toast submitted prompt={}",
            prompt.id
        );

        // Retain the handlers while actionable. Retract an obsolete toast on
        // this WinRT thread, including when recording starts from another UI.
        for _ in 0..1200 {
            std::thread::sleep(Duration::from_millis(100));
            let state = app.state::<MeetingPresenceState>();
            if crate::app_lifecycle::is_recording_active(app)
                || !prompt_is_current(&state, &prompt.id)
            {
                let _ = notifier.Hide(&notification);
                if let Ok(history) = ToastNotificationManager::History() {
                    let _ = history.RemoveGroupedTagWithId(
                        &HSTRING::from(prompt.id.as_str()),
                        &HSTRING::new(),
                        &application_id,
                    );
                }
                break;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{parse_toast_arguments, toast_xml, PROMPT_MESSAGE, PROMPT_NOT_RECORDING_MESSAGE};
    use crate::meeting_presence::PendingPrompt;

    #[test]
    fn toast_xml_shows_meeting_context_not_internal_prompt_id() {
        let prompt = PendingPrompt::for_test("meeting-prompt-1", "Microsoft Teams");

        let xml = toast_xml(&prompt);
        assert!(xml.contains("<text>Microsoft Teams</text>"));
        assert!(xml.contains(PROMPT_MESSAGE));
        assert!(xml.contains(PROMPT_NOT_RECORDING_MESSAGE));
        assert!(!xml.contains("<text>meeting-prompt-1</text>"));
        assert!(xml.contains("prompt=meeting-prompt-1"));
    }

    #[test]
    fn toast_arguments_round_trip_prompt_id_and_action() {
        assert_eq!(
            parse_toast_arguments("kiminola://meeting-prompt?prompt=meeting-prompt-7&action=start"),
            ("meeting-prompt-7", "start")
        );
        assert_eq!(parse_toast_arguments("garbage"), ("", ""));
    }
}
