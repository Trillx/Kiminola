use super::*;
use windows::core::w;
use windows::Win32::UI::Controls::*;

struct Editor(HWND);

// Clipboard tests run only in a new process attached to a private window
// station. A station owns its clipboard. Never read/save/clear WinSta0 data.
fn in_private_station(test_name: &str, body: impl FnOnce()) {
    use windows::Win32::System::StationsAndDesktops::*;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    if std::env::var_os("KIMINOLA_NATIVE_PRIVATE_TEST").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture", "--ignored"])
            .env("KIMINOLA_NATIVE_PRIVATE_TEST", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        print!("{}", String::from_utf8_lossy(&output.stdout));
        return;
    }
    unsafe {
        let old_station = GetProcessWindowStation().unwrap();
        let old_desktop = GetThreadDesktop(GetCurrentThreadId()).unwrap();
        let name: Vec<u16> = format!("KiminolaDictationFixture{}\0", std::process::id())
            .encode_utf16()
            .collect();
        let station =
            CreateWindowStationW(windows::core::PCWSTR(name.as_ptr()), 0, 0x037f, None).unwrap();
        SetProcessWindowStation(station).unwrap();
        let desktop = CreateDesktopW(
            w!("Fixture"),
            None,
            None,
            DESKTOP_CONTROL_FLAGS(0),
            0x000f01ff,
            None,
        )
        .unwrap();
        SetThreadDesktop(desktop).unwrap();
        assert_ne!(GetProcessWindowStation().unwrap(), old_station);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        SetThreadDesktop(old_desktop).unwrap();
        SetProcessWindowStation(old_station).unwrap();
        CloseDesktop(desktop).unwrap();
        CloseWindowStation(station).unwrap();
        if let Err(payload) = outcome {
            std::panic::resume_unwind(payload);
        }
    }
}

#[test]
#[ignore = "Requires permission to create a private window station; never falls back to the user clipboard"]
fn private_clipboard_copy_is_unicode_and_excludes_history_cloud() {
    in_private_station(
        "dictation_native::tests::private_clipboard_copy_is_unicode_and_excludes_history_cloud",
        || {
            use windows::Win32::Foundation::HGLOBAL;
            use windows::Win32::System::{DataExchange::*, Memory::*};
            let sequence = copy_text_impl("Synthetic 🦀\nsecond line").unwrap();
            unsafe {
                assert_eq!(sequence, GetClipboardSequenceNumber());
                OpenClipboard(None).unwrap();
                let handle = GetClipboardData(13).unwrap();
                let ptr = GlobalLock(HGLOBAL(handle.0)) as *const u16;
                assert!(!ptr.is_null());
                let expected: Vec<u16> = "Synthetic 🦀\r\nsecond line\0".encode_utf16().collect();
                assert_eq!(
                    std::slice::from_raw_parts(ptr, expected.len()),
                    expected.as_slice()
                );
                let _ = GlobalUnlock(HGLOBAL(handle.0));
                for name in [
                    w!("CanIncludeInClipboardHistory"),
                    w!("CanUploadToCloudClipboard"),
                ] {
                    let format = RegisterClipboardFormatW(name);
                    let handle = GetClipboardData(format).unwrap();
                    let ptr = GlobalLock(HGLOBAL(handle.0)) as *const u32;
                    assert_eq!(*ptr, 0);
                    let _ = GlobalUnlock(HGLOBAL(handle.0));
                }
                assert!(IsClipboardFormatAvailable(RegisterClipboardFormatW(w!(
                    "ExcludeClipboardContentFromMonitorProcessing"
                )))
                .is_ok());
                CloseClipboard().unwrap();
            }
        },
    );
}
impl Editor {
    fn new(style: u32) -> Self {
        Self(unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("EDIT"),
                w!("synthetic seed"),
                WS_POPUP | WINDOW_STYLE(style),
                -30000,
                -30000,
                300,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        })
    }
}
impl Drop for Editor {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

#[test]
fn native_editor_snapshot_rejects_sensitive_or_unsupported_styles() {
    let editor = Editor::new(ES_MULTILINE as u32);
    let state = read_editor(editor.0).expect("ordinary native EDIT is readable");
    assert_eq!(String::from_utf16(&state.text).unwrap(), "synthetic seed");
    for style in [
        ES_PASSWORD,
        ES_READONLY,
        ES_NUMBER,
        ES_UPPERCASE,
        ES_LOWERCASE,
    ] {
        assert!(read_editor(Editor::new(style as u32).0).is_err());
    }
    unsafe {
        SendMessageW(
            editor.0,
            EM_SETPASSWORDCHAR,
            WPARAM('*' as usize),
            LPARAM(0),
        );
    }
    assert!(read_editor(editor.0).is_err());
}

#[test]
fn releasing_each_chord_key_stops_hold() {
    let shortcut: Shortcut = "Ctrl+Shift+Space".parse().unwrap();
    let held = [0x11, 0x10, 0x20];
    assert!(chord_with(&shortcut, |key| held.contains(&key)));
    for released in held {
        assert!(!chord_with(&shortcut, |key| key != released && held.contains(&key)));
    }
}

#[test]
fn synthetic_activation_key_exemptions_end_on_key_up() {
    const CHILD: &str = "KIMINOLA_SYNTHETIC_KEYBOARD_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // A separate exact-test process cannot race the real monitor in the
        // rest of the suite, even when the parent runs tests in parallel.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "dictation_native::tests::synthetic_activation_key_exemptions_end_on_key_up",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        print!("{}", String::from_utf8_lossy(&output.stdout));
        return;
    }
    assert!(MONITOR_START.get().is_none());
    assert!(!WATCHERS_READY.load(Ordering::SeqCst));

    // Direct synthetic callback calls only. No hook is installed in this
    // process, no input is injected, and no user keyboard state is changed.
    fn event(code: i32, message: u32, key: u32) {
        let data = KBDLLHOOKSTRUCT {
            vkCode: key,
            flags: if matches!(message, WM_KEYUP | WM_SYSKEYUP) {
                LLKHF_UP
            } else {
                KBDLLHOOKSTRUCT_FLAGS(0)
            },
            ..Default::default()
        };
        unsafe {
            keyboard_hook(
                code,
                WPARAM(message as usize),
                LPARAM(&data as *const _ as isize),
            );
        }
    }

    for (down, up) in [(WM_KEYDOWN, WM_KEYUP), (WM_SYSKEYDOWN, WM_SYSKEYUP)] {
        let held = [0x20u32, 0xa0, 0xa2, 0xa4]; // Space, left Shift/Ctrl/Alt.
        for word in &ACTIVATION_KEYS {
            word.store(0, Ordering::SeqCst);
        }
        for key in held {
            ACTIVATION_KEYS[key as usize / 64].fetch_or(1 << (key % 64), Ordering::SeqCst);
        }
        ACTIVITY.store(0, Ordering::SeqCst);
        for key in held {
            event(HC_ACTION as i32, down, key);
            event(HC_ACTION as i32, down, key); // Original hold autorepeat.
        }
        assert_eq!(ACTIVITY.load(Ordering::SeqCst), 0);
        event(HC_ACTION as i32, down, 0x41); // Never held at activation.
        let mut expected = 1;
        assert_eq!(ACTIVITY.load(Ordering::SeqCst), expected);

        for (index, key) in held.into_iter().enumerate() {
            event(-1, up, key); // Negative hook codes must not consume a hold.
            event(HC_ACTION as i32, down, key);
            assert_eq!(ACTIVITY.load(Ordering::SeqCst), expected);
            event(HC_ACTION as i32, up, key);
            assert_eq!(ACTIVITY.load(Ordering::SeqCst), expected);
            event(HC_ACTION as i32, down, key);
            expected += 1;
            assert_eq!(
                ACTIVITY.load(Ordering::SeqCst),
                expected,
                "key {key:#x} must count as activity after message {up:#x}"
            );
            event(HC_ACTION as i32, down, key); // A later hold stays unexempt.
            expected += 1;
            event(HC_ACTION as i32, up, key);
            event(HC_ACTION as i32, up, key); // Duplicate release cannot rearm.
            event(HC_ACTION as i32, down, key);
            expected += 1;
            for still_held in &held[index + 1..] {
                event(HC_ACTION as i32, down, *still_held);
            }
            assert_eq!(ACTIVITY.load(Ordering::SeqCst), expected);
        }
        // Unrecognized messages and negative codes do not count. Out-of-range
        // key-down still fails closed, while key-up must not index the mask.
        event(HC_ACTION as i32, WM_CHAR, 0x41);
        event(-1, down, 0x41);
        event(HC_ACTION as i32, up, 256);
        assert_eq!(ACTIVITY.load(Ordering::SeqCst), expected);
        event(HC_ACTION as i32, down, 256);
        assert_eq!(ACTIVITY.load(Ordering::SeqCst), expected + 1);
    }
    assert!(MONITOR_START.get().is_none());
}

#[test]
fn clipboard_payload_preserves_unicode_normalizes_lines_and_rejects_nul() {
    assert_eq!(
        clipboard_units("🦀\nnext\rlast\r\n").unwrap(),
        "🦀\r\nnext\r\nlast\r\n\0"
            .encode_utf16()
            .collect::<Vec<_>>()
    );
    assert!(clipboard_units("a\0b").is_err());
    assert!(clipboard_units("").is_err());
}

#[test]
fn lock_disconnect_and_suspend_interrupt_but_resume_never_starts_capture() {
    for value in [
        WTS_SESSION_LOCK,
        WTS_SESSION_LOGOFF,
        WTS_CONSOLE_DISCONNECT,
        WTS_REMOTE_DISCONNECT,
    ] {
        assert!(is_interrupt_message(WM_WTSSESSION_CHANGE, value as usize));
    }
    assert!(is_interrupt_message(
        WM_POWERBROADCAST,
        PBT_APMSUSPEND as usize
    ));
    assert!(!is_interrupt_message(
        WM_WTSSESSION_CHANGE,
        WTS_SESSION_UNLOCK as usize
    ));
    assert!(!is_interrupt_message(
        WM_POWERBROADCAST,
        PBT_APMRESUMEAUTOMATIC as usize
    ));
}

#[test]
fn only_non_elevated_medium_integrity_editors_are_eligible() {
    assert!(permits_integrity(0, 0, 0x2000));
    for args in [
        (1, 0, 0x2000),
        (0, 1, 0x2000),
        (0, 0, 0x3000),
        (0, 0, 0x4000),
        (0, 0, 0x1000),
        (0, 0, 0),
    ] {
        assert!(!permits_integrity(args.0, args.1, args.2));
    }
}

#[test]
fn native_delivery_verifies_exact_text_and_caret_without_enter() {
    let editor = Editor::new(ES_MULTILINE as u32);
    unsafe {
        SendMessageW(editor.0, EM_SETSEL, WPARAM(10), LPARAM(14));
    }
    let before = read_editor(editor.0).unwrap();
    let result = deliver_editor(
        editor.0,
        &before,
        "🦀\nnext",
        |_| Ok(()),
        |units| {
            // Test-only insertion into our hidden editor, not keyboard injection
            // or a real clipboard. Production uses exactly one WM_PASTE.
            unsafe {
                SendMessageW(
                    editor.0,
                    EM_REPLACESEL,
                    WPARAM(1),
                    LPARAM(units.as_ptr() as isize),
                );
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result, DeliveryOutcome::Verified);
    let after = read_editor(editor.0).unwrap();
    assert_eq!(
        String::from_utf16(&after.text).unwrap(),
        "synthetic 🦀\r\nnext"
    );
    assert_eq!(after.selection, (18, 18));
}

#[test]
fn native_delivery_focus_loss_after_dispatch_is_uncertain_and_never_retried() {
    let editor = Editor::new(ES_MULTILINE as u32);
    let before = read_editor(editor.0).unwrap();
    let calls = std::cell::Cell::new(0);
    let outcome = deliver_editor(
        editor.0,
        &before,
        "synthetic",
        |before_dispatch| {
            if before_dispatch {
                Ok(())
            } else {
                Err(TARGET_ERROR.into())
            }
        },
        |units| {
            calls.set(calls.get() + 1);
            unsafe {
                SendMessageW(
                    editor.0,
                    EM_REPLACESEL,
                    WPARAM(1),
                    LPARAM(units.as_ptr() as isize),
                );
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(outcome, DeliveryOutcome::Uncertain);
    assert_eq!(calls.get(), 1);
}

#[test]
fn changed_selection_content_and_guard_never_dispatch() {
    for change in 0..3 {
        let editor = Editor::new(ES_MULTILINE as u32);
        let before = read_editor(editor.0).unwrap();
        unsafe {
            match change {
                0 => {
                    SendMessageW(editor.0, EM_SETSEL, WPARAM(1), LPARAM(3));
                }
                1 => {
                    SendMessageW(
                        editor.0,
                        WM_SETTEXT,
                        WPARAM(0),
                        LPARAM(w!("changed synthetic").as_ptr() as isize),
                    );
                }
                _ => {}
            }
        }
        let outcome = deliver_editor(
            editor.0,
            &before,
            "synthetic",
            |_| {
                if change == 2 {
                    Err(TARGET_ERROR.into())
                } else {
                    Ok(())
                }
            },
            |_| panic!("preflight failure must never dispatch"),
        );
        assert!(outcome.is_err());
    }
}

#[test]
fn native_delivery_rejection_timeout_and_partial_insertion_are_uncertain() {
    for action in 0..3 {
        let editor = Editor::new(ES_MULTILINE as u32);
        let before = read_editor(editor.0).unwrap();
        let calls = std::cell::Cell::new(0);
        let result = deliver_editor(
            editor.0,
            &before,
            "synthetic",
            |_| Ok(()),
            |_| {
                calls.set(calls.get() + 1);
                if action == 0 {
                    return Err("simulated timeout".into());
                }
                if action == 1 {
                    unsafe {
                        SendMessageW(
                            editor.0,
                            EM_REPLACESEL,
                            WPARAM(1),
                            LPARAM(w!("partial").as_ptr() as isize),
                        );
                    }
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, DeliveryOutcome::Uncertain);
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn single_line_newlines_and_editor_limits_fail_before_dispatch() {
    let editor = Editor::new(0);
    let before = read_editor(editor.0).unwrap();
    assert!(deliver_editor(
        editor.0,
        &before,
        "line\nline",
        |_| Ok(()),
        |_| panic!("newline must not dispatch")
    )
    .is_err());
    unsafe {
        SendMessageW(editor.0, EM_SETLIMITTEXT, WPARAM(14), LPARAM(0));
    }
    let before = read_editor(editor.0).unwrap();
    assert!(deliver_editor(
        editor.0,
        &before,
        "too long",
        |_| Ok(()),
        |_| panic!("limit must not dispatch")
    )
    .is_err());
}

#[test]
fn native_monitor_delivers_synthetic_lock_suspend_and_refuses_dead_monitor() {
    let calls = Arc::new(AtomicUsize::new(0));
    let sink = calls.clone();
    start_interrupt_monitor(Arc::new(move || {
        sink.fetch_add(1, Ordering::SeqCst);
    }))
    .unwrap();
    let window = HWND(MONITOR_WINDOW.load(Ordering::SeqCst) as *mut _);
    assert!(!window.0.is_null());
    assert!(!unsafe { IsWindowVisible(window) }.as_bool());
    message(window, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 0).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!desktop_is_available());
    assert!(snapshot_target().is_err()); // Returns before inspecting any user editor.
    assert!(copy_text("synthetic must not copy while locked").is_err());
    message(window, WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK as usize, 0).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    message(window, WM_POWERBROADCAST, PBT_APMSUSPEND as usize, 0).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    message(
        window,
        WM_POWERBROADCAST,
        PBT_APMRESUMEAUTOMATIC as usize,
        0,
    )
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    unsafe {
        PostMessageW(window, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while MONITOR_WINDOW.load(Ordering::SeqCst) != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(MONITOR_WINDOW.load(Ordering::SeqCst), 0);
    assert!(!watchers_healthy());
    assert!(start_interrupt_monitor(Arc::new(|| {})).is_err());
}

#[test]
fn release_qualification_blocks_public_snapshot_before_editor_inspection() {
    // No monitor or foreground editor is required. The release gate must run
    // before native eligibility checks, even with an unavailable desktop.
    assert_eq!(
        snapshot_target().err().as_deref(),
        Some("Automatic paste is not release-qualified for this editor. Review and copy the text.")
    );
}

#[test]
fn release_qualification_blocks_public_delivery_without_consuming_attempt() {
    // Only test-owned handles and synthetic text. This deliberately constructs
    // a snapshot privately so a missing snapshot gate cannot be our only guard.
    let editor = Editor::new(ES_MULTILINE as u32);
    let pid = unsafe { GetCurrentProcessId() };
    let process = ProcessHandle(unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .unwrap()
            .0 as usize
    });
    let target = TargetSnapshot {
        edit: editor.0 .0 as usize,
        foreground: editor.0 .0 as usize,
        thread: unsafe { GetCurrentThreadId() },
        pid,
        created: process_security(process.raw()).unwrap(),
        process,
        before: read_editor(editor.0).unwrap(),
        activity: ACTIVITY.load(Ordering::SeqCst),
        mutation: MUTATION.load(Ordering::SeqCst),
        captured: Instant::now(),
        attempted: AtomicBool::new(false),
    };
    for attempted in [false, true] {
        target.attempted.store(attempted, Ordering::SeqCst);
        assert_eq!(
            paste_to_target(&target, "must remain in review").err().as_deref(),
            Some("Automatic paste is not release-qualified for this editor. Review and copy the text.")
        );
        // Rejection must precede even the single-attempt state change, hence
        // also all editor validation, clipboard publication and WM_PASTE.
        assert_eq!(target.attempted.load(Ordering::SeqCst), attempted);
        assert!(read_editor(editor.0).unwrap() == target.before);
    }
}

#[test]
fn target_snapshot_is_send_without_exposing_native_handles_or_surrounding_text() {
    fn assert_send<T: Send>() {}
    assert_send::<TargetSnapshot>();
}

// This is a subprocess entry point, never an interactive editor. It has no
// visible window, activation, clipboard, keyboard injection, or user data.
#[test]
#[ignore = "Subprocess fixture entry point, exercised by cross_process_native_editor"]
fn owned_editor_subprocess() {
    if std::env::var_os("KIMINOLA_OWNED_EDITOR_CHILD").is_none() {
        return;
    }
    use std::io::Write;
    let editor = Editor::new(ES_MULTILINE as u32);
    println!("OWNED_EDITOR={}", editor.0 .0 as usize);
    std::io::stdout().flush().unwrap();
    let mut msg = MSG::default();
    unsafe {
        while IsWindow(editor.0).as_bool() && GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[test]
fn cross_process_native_editor_reads_and_verifies_only_disposable_text() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "dictation_native::tests::owned_editor_subprocess",
                "--ignored",
                "--nocapture",
            ])
            .env("KIMINOLA_OWNED_EDITOR_CHILD", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(handle) = line.strip_prefix("OWNED_EDITOR=") {
                let _ = tx.send(handle.parse::<usize>().unwrap());
                break;
            }
        }
    });
    let edit = HWND(rx.recv_timeout(Duration::from_secs(5)).unwrap() as *mut _);
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(edit, Some(&mut pid));
    }
    assert_eq!(pid, child.0.id());
    assert!(!unsafe { IsWindowVisible(edit) }.as_bool());
    let process = ProcessHandle(unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .unwrap()
            .0 as usize
    });
    // This fixture is expected to run unelevated, like the application.
    assert!(process_security(process.raw()).is_ok());
    not_terminal(process.raw()).unwrap();
    message(edit, EM_SETSEL, 10, 14).unwrap();
    let before = read_editor(edit).unwrap();
    assert_eq!(before.selection, (10, 14));
    let outcome = deliver_editor(
        edit,
        &before,
        "foreign 🦀",
        |_| Ok(()),
        |units| message(edit, EM_REPLACESEL, 1, units.as_ptr() as isize).map(|_| ()),
    )
    .unwrap();
    assert_eq!(outcome, DeliveryOutcome::Verified);
    let after = read_editor(edit).unwrap();
    assert_eq!(
        String::from_utf16(&after.text).unwrap(),
        "synthetic foreign 🦀"
    );
    message(edit, EM_SETREADONLY, 1, 0).unwrap();
    assert!(read_editor(edit).is_err());
    unsafe {
        PostMessageW(edit, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
    }
}

#[test]
fn caret_blink_is_not_a_target_edit_but_movement_and_destruction_are() {
    assert!(!is_target_mutation(EVENT_OBJECT_HIDE, OBJID_CARET.0));
    assert!(!is_target_mutation(EVENT_OBJECT_SHOW, OBJID_CARET.0));
    assert!(is_target_mutation(
        EVENT_OBJECT_LOCATIONCHANGE,
        OBJID_CARET.0
    ));
    assert!(is_target_mutation(EVENT_OBJECT_DESTROY, OBJID_WINDOW.0));
    assert!(is_target_mutation(EVENT_OBJECT_VALUECHANGE, OBJID_CLIENT.0));
    assert!(is_target_mutation(
        EVENT_OBJECT_TEXTSELECTIONCHANGED,
        OBJID_CLIENT.0
    ));
}
