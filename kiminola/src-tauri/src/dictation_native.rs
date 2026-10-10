//! Conservative Windows-only dictation delivery. Surrounding text is private,
//! short-lived verification data, never provider input or diagnostic output.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, LRESULT};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Security::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::*;
use windows::Win32::System::StationsAndDesktops::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::{DataExchange::*, Memory::*};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Controls::{EM_GETLIMITTEXT, EM_GETPASSWORDCHAR, EM_GETSEL};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::*;

const MAX_EDITOR_UNITS: usize = 32_000;
const CLIPBOARD_ERROR: &str = "Clipboard unavailable. Keep the text in review and try Copy again.";
static CLIPBOARD_WRITE: Mutex<()> = Mutex::new(());

const QUALIFICATION_ERROR: &str =
    "Automatic paste is not release-qualified for this editor. Review and copy the text.";
// Release qualification is separate from the native Unicode EDIT checks below.
// No named application/version has passed clipboard + WM_PASTE qualification.
// Each future entry must verify the live process's exact application/version
// and tested platform against recorded disposable-editor evidence. A class name,
// consent, compilation, or synthetic EM_REPLACESEL test is not certification.
// Keep this empty until that evidence and its identity verifier exist.
const CERTIFIED_APPLICATION_VERSIONS: &[fn(&ProcessHandle) -> bool] = &[];

fn require_release_qualification(process: &ProcessHandle) -> Result<(), String> {
    if CERTIFIED_APPLICATION_VERSIONS
        .iter()
        .any(|certifies| certifies(process))
    {
        Ok(())
    } else {
        Err(QUALIFICATION_ERROR.into())
    }
}

fn is_interrupt_message(message: u32, value: usize) -> bool {
    (message == WM_WTSSESSION_CHANGE
        && matches!(
            value as u32,
            WTS_SESSION_LOCK | WTS_SESSION_LOGOFF | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT
        ))
        || (message == WM_POWERBROADCAST && value == PBT_APMSUSPEND as usize)
}
fn permits_integrity(elevated: u32, ui_access: u32, rid: u32) -> bool {
    elevated == 0 && ui_access == 0 && (0x2000..0x3000).contains(&rid)
}

// Integer handles make the owned snapshot Send without asserting HWND's
// thread affinity away. Only SendMessageTimeout/queries use its HWND.
// The retained process handle and process creation time prevent PID reuse.
pub struct TargetSnapshot {
    edit: usize,
    foreground: usize,
    thread: u32,
    pid: u32,
    process: ProcessHandle,
    created: u64,
    before: EditorState,
    activity: u64,
    mutation: u64,
    captured: Instant,
    attempted: AtomicBool,
}

struct ProcessHandle(usize);
impl ProcessHandle {
    fn raw(&self) -> HANDLE {
        HANDLE(self.0 as *mut _)
    }
}
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.raw());
        }
    }
}

fn process_security(process: HANDLE) -> Result<u64, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(|_| TARGET_ERROR)?;
        let token = ProcessHandle(token.0 as usize);
        let mut needed = 0;
        let mut elevation = TOKEN_ELEVATION::default();
        GetTokenInformation(
            token.raw(),
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of_val(&elevation) as u32,
            &mut needed,
        )
        .map_err(|_| TARGET_ERROR)?;
        let mut ui_access = 0u32;
        GetTokenInformation(
            token.raw(),
            TokenUIAccess,
            Some(&mut ui_access as *mut _ as *mut _),
            4,
            &mut needed,
        )
        .map_err(|_| TARGET_ERROR)?;
        // usize alignment is required by TOKEN_MANDATORY_LABEL's SID pointer.
        let mut label_buffer = [0usize; 64];
        GetTokenInformation(
            token.raw(),
            TokenIntegrityLevel,
            Some(label_buffer.as_mut_ptr().cast()),
            std::mem::size_of_val(&label_buffer) as u32,
            &mut needed,
        )
        .map_err(|_| TARGET_ERROR)?;
        let label = &*(label_buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        if !IsValidSid(label.Label.Sid).as_bool() {
            return Err(TARGET_ERROR.into());
        }
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return Err(TARGET_ERROR.into());
        }
        let rid = *GetSidSubAuthority(label.Label.Sid, u32::from(count - 1));
        if !permits_integrity(elevation.TokenIsElevated, ui_access, rid) {
            return Err(TARGET_ERROR.into());
        }
        let mut created = Default::default();
        let mut exit = Default::default();
        let mut kernel = Default::default();
        let mut user = Default::default();
        GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user)
            .map_err(|_| TARGET_ERROR)?;
        if exit.dwHighDateTime != 0 || exit.dwLowDateTime != 0 {
            return Err(TARGET_ERROR.into());
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

fn not_terminal(process: HANDLE) -> Result<(), String> {
    let mut path = [0u16; 32768];
    let mut count = path.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(path.as_mut_ptr()),
            &mut count,
        )
        .map_err(|_| TARGET_ERROR)?;
    }
    let path = String::from_utf16_lossy(&path[..count as usize]);
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.is_empty()
        || [
            "windowsterminal.exe",
            "windowsterminalpreview.exe",
            "conhost.exe",
            "openconsole.exe",
            "cmd.exe",
            "powershell.exe",
            "pwsh.exe",
            "mintty.exe",
            "bash.exe",
            "wsl.exe",
            "wezterm-gui.exe",
            "alacritty.exe",
            "kitty.exe",
            "putty.exe",
        ]
        .contains(&name.as_str())
    {
        return Err(TARGET_ERROR.into());
    }
    Ok(())
}

fn focused_editor() -> Result<(HWND, HWND, u32, u32), String> {
    unsafe {
        let foreground = GetForegroundWindow();
        let mut pid = 0;
        let thread = GetWindowThreadProcessId(foreground, Some(&mut pid));
        if foreground.0.is_null()
            || thread == 0
            || pid == GetCurrentProcessId()
            || !IsWindowVisible(foreground).as_bool()
        {
            return Err(TARGET_ERROR.into());
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(thread, &mut info).map_err(|_| TARGET_ERROR)?;
        let mut focus_pid = 0;
        if info.hwndFocus.0.is_null()
            || info.hwndActive != foreground
            || info.flags.0
                & (GUI_INMENUMODE.0 | GUI_INMOVESIZE.0 | GUI_POPUPMENUMODE.0 | GUI_SYSTEMMENUMODE.0)
                != 0
            || GetAncestor(info.hwndFocus, GA_ROOT) != foreground
            || GetWindowThreadProcessId(info.hwndFocus, Some(&mut focus_pid)) != thread
            || focus_pid != pid
            || !IsWindowVisible(info.hwndFocus).as_bool()
        {
            return Err(TARGET_ERROR.into());
        }
        Ok((foreground, info.hwndFocus, thread, pid))
    }
}

pub fn snapshot_target() -> Result<TargetSnapshot, String> {
    // Do not inspect foreground windows or surrounding text when this release
    // has no certified destinations. Transcription still falls back to review.
    if CERTIFIED_APPLICATION_VERSIONS.is_empty() {
        return Err(QUALIFICATION_ERROR.into());
    }
    if !desktop_is_available() || !watchers_healthy() {
        return Err(TARGET_ERROR.into());
    }
    let (foreground, edit, thread, pid) = focused_editor()?;
    let process = ProcessHandle(unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map_err(|_| TARGET_ERROR)?
            .0 as usize
    });
    let created = process_security(process.raw())?;
    // An elevated Kimi Nola must not bypass UIPI to deliver either.
    process_security(unsafe { GetCurrentProcess() })?;
    not_terminal(process.raw())?;
    require_release_qualification(&process)?;
    WATCHED_EDIT.store(edit.0 as usize, Ordering::SeqCst);
    MUTATION.fetch_add(1, Ordering::SeqCst); // A new snapshot invalidates old ones.
    for key in 0..256 {
        let mask = 1u64 << (key % 64);
        if unsafe { GetAsyncKeyState(key) } < 0 {
            ACTIVATION_KEYS[key as usize / 64].fetch_or(mask, Ordering::SeqCst);
        } else {
            ACTIVATION_KEYS[key as usize / 64].fetch_and(!mask, Ordering::SeqCst);
        }
    }
    let activity = ACTIVITY.load(Ordering::SeqCst);
    let mutation = MUTATION.load(Ordering::SeqCst);
    let before = read_editor(edit)?;
    let target = TargetSnapshot {
        edit: edit.0 as usize,
        foreground: foreground.0 as usize,
        thread,
        pid,
        process,
        created,
        before,
        activity,
        mutation,
        captured: Instant::now(),
        attempted: AtomicBool::new(false),
    };
    validate_target(&target, true)?;
    if read_editor(edit)? != target.before {
        return Err(TARGET_ERROR.into());
    }
    Ok(target)
}

fn validate_target(target: &TargetSnapshot, before_dispatch: bool) -> Result<(), String> {
    if !desktop_is_available()
        || !watchers_healthy()
        || target.captured.elapsed() > Duration::from_secs(11 * 60)
        || ACTIVITY.load(Ordering::SeqCst) != target.activity
        || (before_dispatch && MUTATION.load(Ordering::SeqCst) != target.mutation)
        || WATCHED_EDIT.load(Ordering::SeqCst) != target.edit
    {
        return Err(TARGET_ERROR.into());
    }
    let (foreground, edit, thread, pid) = focused_editor()?;
    if (foreground.0 as usize, edit.0 as usize, thread, pid)
        != (target.foreground, target.edit, target.thread, target.pid)
        || process_security(target.process.raw())? != target.created
    {
        return Err(TARGET_ERROR.into());
    }
    Ok(())
}

/// Backend must hold current session delivery authority and explicit clipboard
/// consent. Windows checks and WM_PASTE are not atomic. Never retry this target.
pub fn paste_to_target(target: &TargetSnapshot, text: &str) -> Result<DeliveryOutcome, String> {
    // Snapshot creation is not delivery authority. Recheck certification before
    // touching attempt state, editor content, the clipboard or the target.
    require_release_qualification(&target.process)?;
    if target.attempted.swap(true, Ordering::SeqCst) {
        return Err(TARGET_ERROR.into());
    }
    validate_target(target, true)?;
    // Fail closed rather than synthesizing key-up events into another app.
    if (1..=254).any(|key| unsafe { GetAsyncKeyState(key) } < 0) {
        return Err(TARGET_ERROR.into());
    }
    let edit = HWND(target.edit as *mut _);
    if read_editor(edit)? != target.before {
        return Err(TARGET_ERROR.into());
    }
    let sequence = copy_text_impl(text)?;
    let outcome = deliver_editor(
        edit,
        &target.before,
        text,
        |before_dispatch| {
            validate_target(target, before_dispatch)?;
            if unsafe { GetClipboardSequenceNumber() } != sequence
                || (1..=254).any(|key| unsafe { GetAsyncKeyState(key) } < 0)
            {
                return Err(TARGET_ERROR.into());
            }
            Ok(())
        },
        |_| message(edit, WM_PASTE, 0, 0).map(|_| ()),
    )?;
    if outcome == DeliveryOutcome::Verified
        && (validate_target(target, false).is_err()
            || unsafe { GetClipboardSequenceNumber() } != sequence)
    {
        return Ok(DeliveryOutcome::Uncertain);
    }
    Ok(outcome)
}

/// Explicit user Copy also replaces the clipboard. Exclusion formats request
/// Windows history/cloud suppression; third-party clipboard readers can ignore them.
pub fn copy_text(text: &str) -> Result<(), String> {
    if !desktop_is_available() {
        return Err("Unlock Windows before copying dictation text.".into());
    }
    copy_text_impl(text).map(|_| ())
}

fn object_name(handle: HANDLE) -> Option<String> {
    let mut name = [0u16; 256];
    unsafe {
        GetUserObjectInformationW(
            handle,
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            std::mem::size_of_val(&name) as u32,
            None,
        )
        .ok()?;
    }
    let count = name.iter().position(|c| *c == 0)?;
    String::from_utf16(&name[..count]).ok()
}

pub fn desktop_is_available() -> bool {
    if SESSION_BLOCKED.load(Ordering::SeqCst) {
        return false;
    }
    unsafe {
        let Ok(station) = GetProcessWindowStation() else {
            return false;
        };
        if object_name(HANDLE(station.0)).as_deref() != Some("WinSta0") {
            return false;
        }
        let Ok(input) = OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS.0 | DESKTOP_SWITCHDESKTOP.0),
        ) else {
            return false;
        };
        let input_name = object_name(HANDLE(input.0));
        let _ = CloseDesktop(input);
        let Ok(thread) = GetThreadDesktop(GetCurrentThreadId()) else {
            return false;
        };
        input_name.as_deref() == Some("Default") && object_name(HANDLE(thread.0)) == input_name
    }
}

static SESSION_BLOCKED: AtomicBool = AtomicBool::new(false);
static ACTIVITY: AtomicU64 = AtomicU64::new(0);
static MUTATION: AtomicU64 = AtomicU64::new(0);
static WATCHED_EDIT: AtomicUsize = AtomicUsize::new(0);
static ACTIVATION_KEYS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static WATCHERS_READY: AtomicBool = AtomicBool::new(false);
static HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static CLOCK: OnceLock<Instant> = OnceLock::new();
static MONITOR_WINDOW: AtomicUsize = AtomicUsize::new(0);
type InterruptCallback = Arc<dyn Fn() + Send + Sync>;
static INTERRUPT_CALLBACK: Mutex<Option<InterruptCallback>> = Mutex::new(None);
static MONITOR_START: OnceLock<Result<(), String>> = OnceLock::new();

fn ticks() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64
}
fn watchers_healthy() -> bool {
    WATCHERS_READY.load(Ordering::SeqCst)
        && ticks().saturating_sub(HEARTBEAT.load(Ordering::SeqCst)) < 1000
}
fn interrupt() {
    ACTIVITY.fetch_add(1, Ordering::SeqCst);
    let callback = INTERRUPT_CALLBACK
        .lock()
        .ok()
        .and_then(|value| value.clone());
    if let Some(callback) = callback {
        // Do not let a backend panic unwind through the Windows callback ABI.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback()));
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let message = wparam.0 as u32;
    if code >= 0 && matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP) {
        let key = (*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode as usize;
        if matches!(message, WM_KEYUP | WM_SYSKEYUP) {
            if key < 256 {
                // Only the original hold is exempt, not a later press of the
                // same key. Releasing one key leaves other held keys exempt.
                ACTIVATION_KEYS[key / 64].fetch_and(!(1 << (key % 64)), Ordering::SeqCst);
            }
        } else if key >= 256
            || ACTIVATION_KEYS[key / 64].load(Ordering::SeqCst) & (1 << (key % 64)) == 0
        {
            ACTIVITY.fetch_add(1, Ordering::SeqCst);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 as u32 != WM_MOUSEMOVE {
        ACTIVITY.fetch_add(1, Ordering::SeqCst);
    }
    CallNextHookEx(None, code, wparam, lparam)
}
unsafe extern "system" fn target_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND || event == EVENT_OBJECT_FOCUS {
        ACTIVITY.fetch_add(1, Ordering::SeqCst);
    } else if hwnd.0 as usize == WATCHED_EDIT.load(Ordering::SeqCst)
        && is_target_mutation(event, object)
    {
        MUTATION.fetch_add(1, Ordering::SeqCst);
    }
}

fn is_target_mutation(event: u32, object: i32) -> bool {
    if object == OBJID_CARET.0 {
        // Caret visibility changes on a timer without any user edit.
        return event == EVENT_OBJECT_LOCATIONCHANGE;
    }
    matches!(
        event,
        EVENT_OBJECT_DESTROY
            | EVENT_OBJECT_HIDE
            | EVENT_OBJECT_STATECHANGE
            | EVENT_OBJECT_VALUECHANGE
            | EVENT_OBJECT_SELECTION
            | EVENT_OBJECT_SELECTIONADD
            | EVENT_OBJECT_SELECTIONREMOVE
            | EVENT_OBJECT_SELECTIONWITHIN
            | EVENT_OBJECT_TEXTSELECTIONCHANGED
    )
}

unsafe extern "system" fn monitor_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if is_interrupt_message(message, wparam.0) {
        SESSION_BLOCKED.store(true, Ordering::SeqCst);
        interrupt();
        return LRESULT(1);
    }
    if (message == WM_WTSSESSION_CHANGE
        && matches!(
            wparam.0 as u32,
            WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT
        ))
        || (message == WM_POWERBROADCAST
            && matches!(
                wparam.0 as u32,
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND
            ))
    {
        SESSION_BLOCKED.store(false, Ordering::SeqCst); // No capture/resume callback.
    }
    if message == WM_TIMER {
        HEARTBEAT.store(ticks(), Ordering::SeqCst);
    }
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

/// Process-lifetime owned notification thread. The callback must only request
/// cancellation and return promptly, never block waiting on capture/LLM work.
pub fn start_interrupt_monitor(callback: InterruptCallback) -> Result<(), String> {
    *INTERRUPT_CALLBACK
        .lock()
        .map_err(|_| "Dictation monitor unavailable")? = Some(callback);
    MONITOR_START
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name("dictation-native-monitor".into())
                .spawn(move || {
                    let result = monitor_thread(&tx);
                    WATCHERS_READY.store(false, Ordering::SeqCst);
                    MONITOR_WINDOW.store(0, Ordering::SeqCst);
                    let _ = tx.try_send(result);
                    interrupt(); // Losing the monitor must stop active capture too.
                })
                .map_err(|_| "Could not start dictation monitor".to_string())?;
            rx.recv_timeout(Duration::from_secs(3))
                .map_err(|_| "Dictation monitor did not start".to_string())?
        })
        .clone()?;
    if MONITOR_WINDOW.load(Ordering::SeqCst) == 0 {
        return Err("Windows dictation interruption monitoring has stopped.".into());
    }
    Ok(())
}

fn monitor_thread(ready: &std::sync::mpsc::SyncSender<Result<(), String>>) -> Result<(), String> {
    const ERROR: &str = "Windows dictation interruption monitoring is unavailable.";
    unsafe {
        let module = GetModuleHandleW(None).map_err(|_| ERROR)?;
        let class = w!("KiminolaDictationInterruptMonitor");
        let atom = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(monitor_proc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        });
        if atom == 0 {
            return Err(ERROR.into());
        }
        // Hidden top-level, NOT HWND_MESSAGE: broadcasts include suspend only
        // for top-level windows. No activation, tray entry, or visible content.
        let window = OwnedWindow(
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class,
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                module,
                None,
            )
            .map_err(|_| ERROR)?,
        );
        WTSRegisterSessionNotification(window.0, NOTIFY_FOR_THIS_SESSION).map_err(|_| ERROR)?;
        let mut hooks = NativeHooks::default();
        hooks.keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0).ok();
        hooks.mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0).ok();
        for (low, high) in [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_OBJECT_DESTROY, EVENT_OBJECT_TEXTSELECTIONCHANGED),
        ] {
            hooks.events.push(SetWinEventHook(
                low,
                high,
                None,
                Some(target_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            ));
        }
        let complete = hooks.keyboard.is_some()
            && hooks.mouse.is_some()
            && hooks.events.iter().all(|hook| !hook.is_invalid());
        if SetTimer(window.0, 1, 200, None) == 0 {
            let _ = WTSUnRegisterSessionNotification(window.0);
            return Err(ERROR.into());
        }
        MONITOR_WINDOW.store(window.0 .0 as usize, Ordering::SeqCst);
        HEARTBEAT.store(ticks(), Ordering::SeqCst);
        WATCHERS_READY.store(complete, Ordering::SeqCst);
        // Lock/suspend monitoring remains useful if activity hooks fail. In
        // that case snapshots fail closed, but microphone capture can work.
        if ready.send(Ok(())).is_err() {
            return Err(ERROR.into());
        }
        let mut msg = MSG::default();
        loop {
            let result = GetMessageW(&mut msg, None, 0, 0).0;
            if result <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = KillTimer(window.0, 1);
        let _ = WTSUnRegisterSessionNotification(window.0);
        drop(hooks);
        drop(window);
        let _ = UnregisterClassW(class, module);
        Ok(())
    }
}

#[derive(Default)]
struct NativeHooks {
    keyboard: Option<HHOOK>,
    mouse: Option<HHOOK>,
    events: Vec<HWINEVENTHOOK>,
}
impl Drop for NativeHooks {
    fn drop(&mut self) {
        unsafe {
            for hook in [self.keyboard, self.mouse].into_iter().flatten() {
                let _ = UnhookWindowsHookEx(hook);
            }
            for hook in &self.events {
                if !hook.is_invalid() {
                    let _ = UnhookWinEvent(*hook);
                }
            }
        }
    }
}

struct OwnedWindow(HWND);
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
struct ClipboardOpen;
impl ClipboardOpen {
    fn close(self) -> Result<(), String> {
        unsafe {
            CloseClipboard().map_err(|_| CLIPBOARD_ERROR)?;
        }
        std::mem::forget(self);
        Ok(())
    }
}
impl Drop for ClipboardOpen {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
struct GlobalBlock(HGLOBAL);
impl GlobalBlock {
    fn new(bytes: &[u8]) -> Result<Self, String> {
        unsafe {
            let block = Self(GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|_| CLIPBOARD_ERROR)?);
            let ptr = GlobalLock(block.0) as *mut u8;
            if ptr.is_null() {
                return Err(CLIPBOARD_ERROR.into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            let _ = GlobalUnlock(block.0);
            Ok(block)
        }
    }
    fn publish(self, format: u32) -> Result<(), String> {
        unsafe {
            SetClipboardData(format, HANDLE(self.0 .0)).map_err(|_| CLIPBOARD_ERROR)?;
        }
        std::mem::forget(self); // Windows owns a successfully published HGLOBAL.
        Ok(())
    }
}
impl Drop for GlobalBlock {
    fn drop(&mut self) {
        unsafe {
            let _ = GlobalFree(self.0);
        }
    }
}

fn copy_text_impl(text: &str) -> Result<u32, String> {
    let units = clipboard_units(text)?;
    let bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    let text_block = GlobalBlock::new(&bytes)?;
    let names = [
        w!("CanIncludeInClipboardHistory"),
        w!("CanUploadToCloudClipboard"),
        w!("ExcludeClipboardContentFromMonitorProcessing"),
    ];
    let mut exclusions = Vec::new();
    for name in names {
        let format = unsafe { RegisterClipboardFormatW(name) };
        if format == 0 {
            return Err(CLIPBOARD_ERROR.into());
        }
        exclusions.push((format, GlobalBlock::new(&0u32.to_le_bytes())?));
    }
    let _serial = CLIPBOARD_WRITE.lock().map_err(|_| CLIPBOARD_ERROR)?;
    // A real owner is required: OpenClipboard(NULL) + EmptyClipboard would
    // leave a NULL owner and can make SetClipboardData fail.
    let owner = OwnedWindow(unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )
        .map_err(|_| CLIPBOARD_ERROR)?
    });
    unsafe {
        OpenClipboard(owner.0).map_err(|_| CLIPBOARD_ERROR)?;
    }
    let open = ClipboardOpen;
    unsafe {
        EmptyClipboard().map_err(|_| CLIPBOARD_ERROR)?;
    }
    // Publish the exclusions before the Unicode payload. No old formats are
    // read, retained, logged or claimed to be restored.
    for (format, block) in exclusions {
        block.publish(format)?;
    }
    // CF_UNICODETEXT
    text_block.publish(13)?;
    // We still hold the clipboard open, so this reads only the payload we
    // just published, never a prior user's clipboard value.
    unsafe {
        let handle = HGLOBAL(GetClipboardData(13).map_err(|_| CLIPBOARD_ERROR)?.0);
        if GlobalSize(handle) < bytes.len() {
            return Err(CLIPBOARD_ERROR.into());
        }
        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            return Err(CLIPBOARD_ERROR.into());
        }
        let matches = std::slice::from_raw_parts(ptr, units.len()) == units.as_slice();
        let _ = GlobalUnlock(handle);
        if !matches {
            return Err(CLIPBOARD_ERROR.into());
        }
    }
    // Sample while we still own the open clipboard. Sampling only after
    // CloseClipboard could accidentally bless another process's replacement.
    let sequence = unsafe { GetClipboardSequenceNumber() };
    open.close()?;
    if unsafe { GetClipboardSequenceNumber() } != sequence {
        return Err(CLIPBOARD_ERROR.into());
    }
    Ok(sequence)
}

fn clipboard_units(text: &str) -> Result<Vec<u16>, String> {
    if text.is_empty() || text.contains('\0') || text.len() > 128_000 {
        return Err("Text is empty or exceeds the safe clipboard limit.".into());
    }
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n");
    let mut units: Vec<u16> = normalized.encode_utf16().collect();
    if units.len() > MAX_EDITOR_UNITS {
        return Err("Text exceeds the safe clipboard limit.".into());
    }
    units.push(0);
    Ok(units)
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Verified,
    Uncertain,
}

fn deliver_editor(
    hwnd: HWND,
    before: &EditorState,
    text: &str,
    guard: impl Fn(bool) -> Result<(), String>,
    dispatch: impl FnOnce(&[u16]) -> Result<(), String>,
) -> Result<DeliveryOutcome, String> {
    let units = clipboard_units(text)?;
    let insertion = &units[..units.len() - 1];
    if before.style & ES_MULTILINE as u32 == 0
        && insertion.iter().any(|unit| [10, 13].contains(unit))
    {
        return Err(TARGET_ERROR.into());
    }
    let start = before.selection.0 as usize;
    let end = before.selection.1 as usize;
    let mut expected = before.text[..start].to_vec();
    expected.extend_from_slice(insertion);
    expected.extend_from_slice(&before.text[end..]);
    if expected.len() > MAX_EDITOR_UNITS
        || expected.len() > before.limit
        || String::from_utf16(&expected).is_err()
    {
        return Err(TARGET_ERROR.into());
    }
    guard(true)?;
    if read_editor(hwnd)? != *before {
        return Err(TARGET_ERROR.into());
    }
    guard(true)?;
    // A timeout/error after dispatch may mean the target did paste. Never
    // convert it to a retryable pre-dispatch error or dispatch a second paste.
    if dispatch(&units).is_err() {
        return Ok(DeliveryOutcome::Uncertain);
    }
    let caret = (start + insertion.len()) as u32;
    match read_editor(hwnd) {
        Ok(after)
            if after.text == expected
                && after.selection == (caret, caret)
                && after.style == before.style
                && after.limit == before.limit
                && guard(false).is_ok() =>
        {
            Ok(DeliveryOutcome::Verified)
        }
        _ => Ok(DeliveryOutcome::Uncertain),
    }
}
const TARGET_ERROR: &str =
    "The original editor is unavailable, changed, or unsupported. Review and copy the text.";

fn message(hwnd: HWND, id: u32, wparam: usize, lparam: isize) -> Result<usize, String> {
    let mut result = 0;
    let status = unsafe {
        SendMessageTimeoutW(
            hwnd,
            id,
            WPARAM(wparam),
            LPARAM(lparam),
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            200,
            Some(&mut result),
        )
    };
    if status.0 == 0 {
        Err(TARGET_ERROR.into())
    } else {
        Ok(result)
    }
}

#[derive(PartialEq, Eq)]
struct EditorState {
    text: Vec<u16>,
    selection: (u32, u32),
    style: u32,
    limit: usize,
}

fn read_editor(hwnd: HWND) -> Result<EditorState, String> {
    unsafe {
        let mut class = [0u16; 64];
        let count = GetClassNameW(hwnd, &mut class);
        let mut real_class = [0u16; 64];
        let real_count = RealGetWindowClassW(hwnd, &mut real_class);
        let is_edit = |value: &[u16]| String::from_utf16_lossy(value).eq_ignore_ascii_case("Edit");
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        if !IsWindow(hwnd).as_bool()
            || !IsWindowUnicode(hwnd).as_bool()
            || !windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(hwnd).as_bool()
            || count <= 0
            || real_count == 0
            || !is_edit(&class[..count as usize])
            || !is_edit(&real_class[..real_count as usize])
            || style
                & (ES_PASSWORD
                    | ES_READONLY
                    | ES_NUMBER
                    | ES_UPPERCASE
                    | ES_LOWERCASE
                    | ES_OEMCONVERT) as u32
                != 0
            || message(hwnd, EM_GETPASSWORDCHAR, 0, 0)? != 0
        {
            return Err(TARGET_ERROR.into());
        }
        let length = message(hwnd, WM_GETTEXTLENGTH, 0, 0)?;
        if length > MAX_EDITOR_UNITS {
            return Err(TARGET_ERROR.into());
        }
        let mut buffer = vec![0u16; length + 1];
        let copied = message(hwnd, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize)?;
        if copied != length || message(hwnd, WM_GETTEXTLENGTH, 0, 0)? != length {
            return Err(TARGET_ERROR.into());
        }
        buffer.truncate(length);
        if String::from_utf16(&buffer).is_err() {
            return Err(TARGET_ERROR.into());
        }
        let selection = message(hwnd, EM_GETSEL, 0, 0)? as u32;
        let selection = (selection & 0xffff, selection >> 16);
        if selection.0 > selection.1 || selection.1 as usize > length {
            return Err(TARGET_ERROR.into());
        }
        Ok(EditorState {
            text: buffer,
            selection,
            style,
            limit: message(hwnd, EM_GETLIMITTEXT, 0, 0)?,
        })
    }
}

pub fn chord_is_held(shortcut: &Shortcut) -> bool {
    chord_with(shortcut, |key| unsafe { GetAsyncKeyState(key as i32) < 0 })
}

fn chord_with(shortcut: &Shortcut, down: impl Fn(u16) -> bool) -> bool {
    let supported = Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT | Modifiers::SUPER;
    if !(shortcut.mods - supported).is_empty() {
        return false;
    }
    let Some(key) = primary_vk(shortcut.key) else {
        return false;
    };
    down(key)
        && (!shortcut.mods.contains(Modifiers::CONTROL) || down(0x11))
        && (!shortcut.mods.contains(Modifiers::SHIFT) || down(0x10))
        && (!shortcut.mods.contains(Modifiers::ALT) || down(0x12))
        && (!shortcut.mods.contains(Modifiers::SUPER) || down(0x5b) || down(0x5c))
}

// Mirrors the pinned global-hotkey 0.8 Windows mapping for ordinary keys.
// Unknown/media keys fail closed instead of leaving hold capture running.
fn primary_vk(key: Code) -> Option<u16> {
    use Code::*;
    let groups: &[(&[Code], u16)] = &[
        (
            &[
                KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN,
                KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
            ],
            0x41,
        ),
        (
            &[
                Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
            ],
            0x30,
        ),
        (
            &[
                F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18,
                F19, F20, F21, F22, F23, F24,
            ],
            0x70,
        ),
        (
            &[
                Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8,
                Numpad9,
            ],
            0x60,
        ),
    ];
    for (keys, first) in groups {
        if let Some(index) = keys.iter().position(|candidate| *candidate == key) {
            return Some(first + index as u16);
        }
    }
    Some(match key {
        Backspace => 0x08,
        Tab => 0x09,
        Enter | NumpadEnter => 0x0d,
        Escape => 0x1b,
        Space => 0x20,
        PageUp => 0x21,
        PageDown => 0x22,
        End => 0x23,
        Home => 0x24,
        ArrowLeft => 0x25,
        ArrowUp => 0x26,
        ArrowRight => 0x27,
        ArrowDown => 0x28,
        Insert => 0x2d,
        Delete => 0x2e,
        NumpadMultiply => 0x6a,
        NumpadAdd => 0x6b,
        NumpadSubtract => 0x6d,
        NumpadDecimal => 0x6e,
        NumpadDivide => 0x6f,
        Semicolon => 0xba,
        Equal => 0xbb,
        Comma => 0xbc,
        Minus => 0xbd,
        Period => 0xbe,
        Slash => 0xbf,
        Backquote => 0xc0,
        BracketLeft => 0xdb,
        Backslash => 0xdc,
        BracketRight => 0xdd,
        Quote => 0xde,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "dictation_native_tests.rs"]
mod tests;
