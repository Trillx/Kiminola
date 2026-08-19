use std::{
    env,
    io::{self, Read},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use windows::{
    core::{HSTRING, IInspectable, Interface, Result},
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager},
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
};

const APPLICATION_ID: &str = "com.kiminola.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptAction {
    JotNotes,
    StartRecording,
    NotNow,
}

#[derive(Debug, PartialEq, Eq)]
enum ActionResult {
    Accepted(PromptAction),
    RejectedStale,
    RejectedUnknown,
}

#[derive(Debug)]
struct PendingPrompt {
    id: String,
    active: bool,
}

impl PendingPrompt {
    fn apply(&mut self, prompt_id: &str, action: &str) -> ActionResult {
        if !self.active || self.id != prompt_id {
            return ActionResult::RejectedStale;
        }

        let action = match action {
            "notes" => PromptAction::JotNotes,
            "start" => PromptAction::StartRecording,
            "not-now" => PromptAction::NotNow,
            _ => return ActionResult::RejectedUnknown,
        };

        self.active = false;
        ActionResult::Accepted(action)
    }
}

struct WinRtApartment;

impl WinRtApartment {
    fn initialize() -> Result<Self> {
        // This scratch executable owns its thread, so it can initialize a WinRT
        // apartment directly. The production bridge must use the Tauri thread
        // lifecycle instead of copying this guard blindly.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self)
    }
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

fn main() -> Result<()> {
    let _apartment = WinRtApartment::initialize()?;
    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        Some("test-actions") => run_action_gate_tests(),
        Some("probe") => probe_notification_registration(),
        Some("show") => {
            let wait_for_activation = args.any(|arg| arg == "--wait");
            show_meeting_prompt(wait_for_activation)
        }
        _ => {
            println!("Kimi Nola Windows toast spike");
            println!("  cargo run -- test-actions");
            println!("  cargo run -- probe");
            println!("  cargo run -- show [--wait]");
            Ok(())
        }
    }
}

fn run_action_gate_tests() -> Result<()> {
    let mut prompt = PendingPrompt {
        id: "prompt-current".to_owned(),
        active: true,
    };

    assert_eq!(
        prompt.apply("prompt-current", "start"),
        ActionResult::Accepted(PromptAction::StartRecording)
    );
    assert_eq!(prompt.apply("prompt-old", "start"), ActionResult::RejectedStale);

    let mut prompt = PendingPrompt {
        id: "prompt-current".to_owned(),
        active: true,
    };
    assert_eq!(
        prompt.apply("prompt-old", "start"),
        ActionResult::RejectedStale
    );
    assert!(prompt.active, "a stale action must not consume the live prompt");
    assert_eq!(
        prompt.apply("prompt-current", "unknown"),
        ActionResult::RejectedUnknown
    );
    assert!(prompt.active, "an unknown action must not consume the live prompt");
    assert_eq!(
        prompt.apply("prompt-current", "notes"),
        ActionResult::Accepted(PromptAction::JotNotes)
    );
    assert!(!prompt.active);
    assert_eq!(
        prompt.apply("prompt-current", "start"),
        ActionResult::RejectedStale
    );

    println!("PASS: current actions are accepted once");
    println!("PASS: stale, unknown, and already-resolved actions are rejected");
    println!("PASS: this gate contains no recording/audio side effect");
    Ok(())
}

fn probe_notification_registration() -> Result<()> {
    let application_id = HSTRING::from(APPLICATION_ID);
    match ToastNotificationManager::CreateToastNotifierWithId(&application_id) {
        Ok(notifier) => {
            println!("PASS: CreateToastNotifierWithId({APPLICATION_ID}) succeeded");
            print_notification_setting(&notifier);
        }
        Err(error) => {
            println!("FAIL: CreateToastNotifierWithId({APPLICATION_ID})");
            println!("HRESULT={:#010x}", error.code().0 as u32);
            println!("message={error}");
            println!("The installed-artifact identity/shortcut registration is not proven.");
        }
    }

    match ToastNotificationManager::CreateToastNotifier() {
        Ok(notifier) => {
            println!("PASS: CreateToastNotifier() succeeded");
            print_notification_setting(&notifier);
        }
        Err(error) => {
            println!("FAIL: CreateToastNotifier()");
            println!("HRESULT={:#010x}", error.code().0 as u32);
            println!("message={error}");
        }
    }

    Ok(())
}

fn show_meeting_prompt(wait_for_activation: bool) -> Result<()> {
    let prompt_id = format!(
        "spike-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    let xml = toast_xml(&prompt_id);
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(xml.as_str()))?;
    let notification = ToastNotification::CreateToastNotification(&document)?;
    notification.SetTag(&HSTRING::from(prompt_id.as_str()))?;

    let notifier = match ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APPLICATION_ID)) {
        Ok(notifier) => {
            println!("identity={APPLICATION_ID} registration=accepted");
            notifier
        }
        Err(error) => {
            println!("identity={APPLICATION_ID} registration=rejected");
            println!("HRESULT={:#010x} message={error}", error.code().0 as u32);
            println!("Trying the default notifier only to separate API availability from app identity.");
            ToastNotificationManager::CreateToastNotifier()?
        }
    };

    print_notification_setting(&notifier);
    let prompt = PendingPrompt {
        id: prompt_id.clone(),
        active: true,
    };
    let prompt_state = std::sync::Arc::new(std::sync::Mutex::new(prompt));
    let callback_state = prompt_state.clone();
    let activated_handler: TypedEventHandler<ToastNotification, IInspectable> =
        TypedEventHandler::new(move |_toast: &Option<ToastNotification>, args: &Option<IInspectable>| {
        let Some(args) = args else {
            println!("activation=rejected reason=missing-event-args");
            return Ok(());
        };

        let activation: ToastActivatedEventArgs = match args.cast() {
            Ok(activation) => activation,
            Err(error) => {
                println!("activation=rejected reason=unexpected-event-type error={error}");
                return Ok(());
            }
        };
        let arguments = match activation.Arguments() {
            Ok(arguments) => arguments.to_string_lossy(),
            Err(error) => {
                println!("activation=rejected reason=missing-arguments error={error}");
                return Ok(());
            }
        };
        let (prompt_id, action) = parse_arguments(&arguments);
        let result = callback_state
            .lock()
            .map(|mut prompt| prompt.apply(prompt_id, action));
        match result {
            Ok(ActionResult::Accepted(PromptAction::JotNotes)) => {
                println!("activation=accepted action=notes recording=false");
            }
            Ok(ActionResult::Accepted(PromptAction::StartRecording)) => {
                println!("activation=accepted action=start recording=NOT_STARTED_BY_SPIKE");
            }
            Ok(ActionResult::Accepted(PromptAction::NotNow)) => {
                println!("activation=accepted action=not-now recording=false");
            }
            Ok(ActionResult::RejectedStale) => {
                println!("activation=rejected reason=stale-or-resolved recording=false");
            }
            Ok(ActionResult::RejectedUnknown) => {
                println!("activation=rejected reason=unknown-action recording=false");
            }
            Err(_) => println!("activation=rejected reason=prompt-state-lock recording=false"),
        }
        Ok(())
        });
    notification.Activated(&activated_handler)?;

    let failed_handler: TypedEventHandler<ToastNotification, windows::UI::Notifications::ToastFailedEventArgs> =
        TypedEventHandler::new(
            |_toast: &Option<ToastNotification>,
             args: &Option<windows::UI::Notifications::ToastFailedEventArgs>| {
                println!("notification=failed details_present={}", args.is_some());
                Ok(())
            },
        );
    notification.Failed(&failed_handler)?;

    match notifier.Show(&notification) {
        Ok(()) => {
            println!("notification=shown prompt_id={prompt_id}");
            println!("recording=false");
        }
        Err(error) => {
            println!("notification=show-failed");
            println!("HRESULT={:#010x} message={error}", error.code().0 as u32);
            println!("recording=false");
        }
    }

    if wait_for_activation {
        println!("waiting=60s for a body/action click; press Enter to finish");
        let mut input = String::new();
        let _ = io::stdin().read_to_string(&mut input);
    } else {
        thread::sleep(Duration::from_millis(500));
    }

    Ok(())
}

fn print_notification_setting(
    notifier: &windows::UI::Notifications::ToastNotifier,
) {
    match notifier.Setting() {
        Ok(setting) => println!("notification_setting={setting:?}"),
        Err(error) => println!(
            "notification_setting=unavailable HRESULT={:#010x} message={error}",
            error.code().0 as u32
        ),
    }
}

fn toast_xml(prompt_id: &str) -> String {
    format!(
        r#"<toast launch="kiminola://meeting-prompt?prompt={prompt_id}&amp;action=open">
  <visual>
    <binding template="ToastGeneric">
      <text>Kimi Nola</text>
      <text>You may be in a meeting. Want to jot notes?</text>
      <text>Kimi Nola is not recording.</text>
    </binding>
  </visual>
  <actions>
    <action content="Jot notes" arguments="kiminola://meeting-prompt?prompt={prompt_id}&amp;action=notes" activationType="foreground" />
    <action content="Start recording" arguments="kiminola://meeting-prompt?prompt={prompt_id}&amp;action=start" activationType="foreground" />
    <action content="Not now" arguments="kiminola://meeting-prompt?prompt={prompt_id}&amp;action=not-now" activationType="foreground" />
  </actions>
</toast>"#
    )
}

fn parse_arguments(arguments: &str) -> (&str, &str) {
    let query = arguments.split_once('?').map(|(_, query)| query).unwrap_or_default();
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
    use super::{ActionResult, PendingPrompt, PromptAction};

    #[test]
    fn stale_start_does_not_consume_current_prompt() {
        let mut prompt = PendingPrompt {
            id: "current".to_owned(),
            active: true,
        };

        assert_eq!(
            prompt.apply("old", "start"),
            ActionResult::RejectedStale
        );
        assert!(prompt.active);
    }

    #[test]
    fn current_prompt_accepts_one_explicit_action() {
        let mut prompt = PendingPrompt {
            id: "current".to_owned(),
            active: true,
        };

        assert_eq!(
            prompt.apply("current", "notes"),
            ActionResult::Accepted(PromptAction::JotNotes)
        );
        assert_eq!(
            prompt.apply("current", "start"),
            ActionResult::RejectedStale
        );
    }

    #[test]
    fn unknown_action_does_not_consume_prompt() {
        let mut prompt = PendingPrompt {
            id: "current".to_owned(),
            active: true,
        };

        assert_eq!(
            prompt.apply("current", "unexpected"),
            ActionResult::RejectedUnknown
        );
        assert!(prompt.active);
    }
}
