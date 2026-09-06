//! One-shot Windows window arrangement for the detected-meeting handoff.
//!
//! The layout is deliberately applied only when the user explicitly starts a
//! recording from a meeting-presence prompt. Event hooks exist only during the
//! transition and cancel it when the user takes control.

#[path = "window_transition.rs"]
mod transition;

#[cfg(target_os = "windows")]
mod win32 {
    use std::{
        cell::{Cell, RefCell},
        mem::size_of,
        sync::{
            atomic::{AtomicU64, Ordering},
            Mutex,
        },
        time::{Duration, Instant},
    };

    use super::transition::{self, split_work_area, Bounds};
    use tauri::Manager;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::HiDpi::{
        AdjustWindowRectExForDpi, GetDpiForWindow, SetThreadDpiAwarenessContext,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, EnumWindows, GetGUIThreadInfo, GetWindowLongW, GetWindowRect,
        GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
        IsZoomed, PeekMessageW, SetWindowPlacement, SetWindowPos, ShowWindow,
        SystemParametersInfoW, EVENT_OBJECT_DESTROY, EVENT_SYSTEM_MINIMIZESTART,
        EVENT_SYSTEM_MOVESIZESTART, GUITHREADINFO, GUI_INMOVESIZE, GWL_EXSTYLE, GWL_STYLE, MSG,
        OBJID_WINDOW, PM_REMOVE, SPI_GETCLIENTAREAANIMATION, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
        SWP_NOZORDER, SW_RESTORE, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        WINDOWPLACEMENT, WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WS_EX_TOOLWINDOW,
    };

    static GENERATION: AtomicU64 = AtomicU64::new(0);
    // Only one worker may issue window operations. A superseded worker checks
    // its generation before every write, including the final placement.
    static WRITER: Mutex<()> = Mutex::new(());

    thread_local! {
        static WATCHED: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
        static CANCELLED: Cell<bool> = const { Cell::new(false) };
    }

    impl Bounds {
        fn from_rect(rect: RECT) -> Self {
            Self {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            }
        }
    }

    #[derive(Clone, Copy)]
    struct WindowCandidate {
        hwnd: HWND,
        area: i64,
    }

    struct WindowSearch {
        process_id: u32,
        candidates: Vec<WindowCandidate>,
    }

    pub(super) fn apply(app: &tauri::AppHandle, meeting_process_id: u32) -> Result<(), String> {
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        let app = app.clone();
        std::thread::Builder::new()
            .name("companion-transition".into())
            .spawn(move || {
                let _writer = WRITER
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if !current(generation) {
                    return;
                }
                // This dedicated worker uses physical screen coordinates on every
                // monitor. No DPI context leaks to Tauri's UI thread.
                unsafe {
                    let _ =
                        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
                }
                if let Err(error) = arrange(&app, meeting_process_id, generation) {
                    eprintln!("[window-layout] companion transition unavailable: {error}");
                    if current(generation) {
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.unminimize();
                            let _ = main.set_focus();
                        }
                    }
                }
            })
            .map(|_| ())
            .map_err(|error| format!("could not start window transition: {error}"))
    }

    fn current(generation: u64) -> bool {
        GENERATION.load(Ordering::SeqCst) == generation
    }

    fn arrange(
        app: &tauri::AppHandle,
        meeting_process_id: u32,
        generation: u64,
    ) -> Result<(), String> {
        let main = app
            .get_webview_window("main")
            .ok_or_else(|| "main window is unavailable".to_string())?;
        let main_hwnd = HWND(main.hwnd().map_err(|error| error.to_string())?.0);
        let meeting_window = find_meeting_window(meeting_process_id);
        let mut meeting = None;
        let mut dpi = unsafe { GetDpiForWindow(main_hwnd) }.max(96);
        let work_area = if let Some(candidate) =
            meeting_window.filter(|candidate| candidate.hwnd != main_hwnd)
        {
            match monitor_bounds(candidate.hwnd) {
                Ok((monitor_area, work_area)) => {
                    dpi = unsafe { GetDpiForWindow(candidate.hwnd) }.max(96);
                    let meeting_is_fullscreen = system_is_fullscreen_or_presentation()
                        || (!unsafe { IsZoomed(candidate.hwnd) }.as_bool()
                            && covers_monitor(candidate.hwnd, monitor_area));
                    if meeting_is_fullscreen {
                        eprintln!("[window-layout] full-screen meeting window left untouched");
                    } else {
                        meeting = Some(candidate.hwnd);
                    }
                    work_area
                }
                Err(error) => {
                    eprintln!("[window-layout] meeting monitor unavailable: {error}");
                    fallback_work_area(app, &main)?
                }
            }
        } else {
            fallback_work_area(app, &main)?
        };
        let (meeting_bounds, notes_bounds) =
            split_work_area(work_area, minimum_notes_width(main_hwnd, dpi), dpi);
        let mut watched = vec![main_hwnd];
        watched.extend(meeting);
        let _hooks = Hooks::install(&watched)?;
        if interrupted(generation, &watched, false) {
            return Ok(());
        }

        let mut moves = Vec::new();
        // WINDOWPLACEMENT installs the normal rectangle as part of revealing
        // a hidden/minimized window, so its old rectangle never flashes first.
        if unsafe { !IsWindowVisible(main_hwnd).as_bool() || IsIconic(main_hwnd).as_bool() } {
            reveal_at(main_hwnd, notes_bounds, work_area)?;
        } else {
            restore(main_hwnd);
            moves.push(Movement {
                hwnd: main_hwnd,
                start: window_bounds(main_hwnd)?,
                target: notes_bounds,
            });
        }
        if interrupted(generation, &watched, false) {
            return Ok(());
        }
        if let Some(hwnd) = meeting {
            restore(hwnd);
            if let Ok(start) = window_bounds(hwnd) {
                moves.push(Movement {
                    hwnd,
                    start,
                    target: meeting_bounds,
                });
            }
        }
        if interrupted(generation, &watched, true) {
            return Ok(());
        }
        let _ = main.set_focus();
        // The notes window is already visible. A late positioning failure must
        // not enter the startup fallback and reopen/refocus a window the user
        // may just have closed or hidden.
        if let Err(error) = animate(&moves, &watched, generation, animations_enabled()) {
            eprintln!("[window-layout] transition stopped: {error}");
        }
        Ok(())
    }

    struct Movement {
        hwnd: HWND,
        start: Bounds,
        target: Bounds,
    }

    fn animate(
        moves: &[Movement],
        watched: &[HWND],
        generation: u64,
        enabled: bool,
    ) -> Result<(), String> {
        let started = Instant::now();
        loop {
            let cancelled = interrupted(generation, watched, true);
            let Some(progress) =
                transition::progress(started.elapsed(), enabled, cancelled, current(generation))
            else {
                return Ok(());
            };
            for movement in moves {
                if interrupted(generation, watched, true) {
                    return Ok(());
                }
                // Both windows use the same sample. Synchronous calls on this
                // worker allow at most one outstanding write, with no queued
                // animation frames. Late frames sample the current clock.
                set_bounds(
                    movement.hwnd,
                    movement.start.interpolate(movement.target, progress),
                )?;
            }
            if progress >= 1.0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    fn set_bounds(hwnd: HWND, bounds: Bounds) -> Result<(), String> {
        unsafe {
            SetWindowPos(
                hwnd,
                None,
                bounds.left,
                bounds.top,
                bounds.width().max(1),
                bounds.height().max(1),
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER,
            )
            .map_err(|error| format!("SetWindowPos failed: {error}"))?;
        }
        Ok(())
    }

    fn window_bounds(hwnd: HWND) -> Result<Bounds, String> {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|error| error.to_string())?;
        Ok(Bounds::from_rect(rect))
    }

    fn restore(hwnd: HWND) {
        unsafe {
            if IsZoomed(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
        }
    }

    fn minimum_notes_width(hwnd: HWND, dpi: u32) -> i32 {
        let width = (420.0 * f64::from(dpi) / 96.0).ceil() as i32;
        let mut rect = RECT {
            right: width,
            ..Default::default()
        };
        unsafe {
            if AdjustWindowRectExForDpi(
                &mut rect,
                WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32),
                false,
                WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32),
                dpi,
            )
            .is_ok()
            {
                return rect.right - rect.left;
            }
        }
        width
    }

    fn reveal_at(hwnd: HWND, bounds: Bounds, work_area: Bounds) -> Result<(), String> {
        // Normal app WINDOWPLACEMENT uses workspace coordinates. Tool windows
        // use screen coordinates. Account for taskbars at the top or left.
        let (monitor, _) = monitor_bounds_for_rect(bounds)?;
        let tool = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 } & WS_EX_TOOLWINDOW.0 != 0;
        let dx = if tool {
            0
        } else {
            work_area.left - monitor.left
        };
        let dy = if tool { 0 } else { work_area.top - monitor.top };
        let placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            showCmd: SW_SHOWNOACTIVATE.0 as u32,
            rcNormalPosition: RECT {
                left: bounds.left - dx,
                top: bounds.top - dy,
                right: bounds.right - dx,
                bottom: bounds.bottom - dy,
            },
            ..Default::default()
        };
        unsafe { SetWindowPlacement(hwnd, &placement) }
            .map_err(|error| format!("could not reveal notes: {error}"))
    }

    fn animations_enabled() -> bool {
        let mut enabled = BOOL(0);
        unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&mut enabled as *mut BOOL).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .is_ok()
                && enabled.as_bool()
        }
    }

    struct Hooks(Vec<HWINEVENTHOOK>);

    impl Hooks {
        fn install(windows: &[HWND]) -> Result<Self, String> {
            WATCHED.with(|watched| *watched.borrow_mut() = windows.to_vec());
            CANCELLED.with(|cancelled| cancelled.set(false));
            let mut hooks = Self(Vec::new());
            for event in [
                EVENT_SYSTEM_MOVESIZESTART,
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_OBJECT_DESTROY,
            ] {
                let hook = unsafe {
                    SetWinEventHook(
                        event,
                        event,
                        None,
                        Some(on_event),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    )
                };
                if hook.0.is_null() {
                    return Err("could not observe user window control".into());
                }
                hooks.0.push(hook);
            }
            Ok(hooks)
        }
    }

    impl Drop for Hooks {
        fn drop(&mut self) {
            for hook in self.0.drain(..) {
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            }
            WATCHED.with(|watched| watched.borrow_mut().clear());
        }
    }

    unsafe extern "system" fn on_event(
        _: HWINEVENTHOOK,
        event: u32,
        hwnd: HWND,
        object: i32,
        _: i32,
        _: u32,
        _: u32,
    ) {
        if event == EVENT_OBJECT_DESTROY && object != OBJID_WINDOW.0 {
            return;
        }
        if WATCHED.with(|watched| watched.borrow().contains(&hwnd)) {
            CANCELLED.with(|cancelled| cancelled.set(true));
        }
    }

    fn interrupted(generation: u64, watched: &[HWND], require_visible: bool) -> bool {
        // OUTOFCONTEXT hooks deliver to their installing thread's message loop.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
        }
        if !current(generation) || CANCELLED.with(Cell::get) {
            return true;
        }
        watched.iter().any(|&hwnd| unsafe {
            if !IsWindow(hwnd).as_bool()
                || (require_visible
                    && (!IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool()))
            {
                return true;
            }
            let thread = GetWindowThreadProcessId(hwnd, None);
            let mut info = GUITHREADINFO {
                cbSize: size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            GetGUIThreadInfo(thread, &mut info).is_ok() && info.flags.contains(GUI_INMOVESIZE)
        })
    }

    fn monitor_bounds_for_rect(bounds: Bounds) -> Result<(Bounds, Bounds), String> {
        use windows::Win32::Graphics::Gdi::MonitorFromRect;
        let rect = RECT {
            left: bounds.left,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.bottom,
        };
        unsafe {
            let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return Err("no destination monitor".into());
            }
            Ok((
                Bounds::from_rect(info.rcMonitor),
                Bounds::from_rect(info.rcWork),
            ))
        }
    }

    fn fallback_work_area(
        app: &tauri::AppHandle,
        main: &tauri::WebviewWindow,
    ) -> Result<Bounds, String> {
        let monitor = main
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten())
            .ok_or_else(|| "no display is available for companion layout".to_string())?;
        let work_area = monitor.work_area();
        Ok(Bounds {
            left: work_area.position.x,
            top: work_area.position.y,
            right: work_area.position.x + work_area.size.width as i32,
            bottom: work_area.position.y + work_area.size.height as i32,
        })
    }

    fn monitor_bounds(hwnd: HWND) -> Result<(Bounds, Bounds), String> {
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if monitor.0.is_null() {
                return Err("MonitorFromWindow returned no monitor".to_string());
            }
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return Err("GetMonitorInfoW failed".to_string());
            }
            Ok((
                Bounds::from_rect(info.rcMonitor),
                Bounds::from_rect(info.rcWork),
            ))
        }
    }

    fn covers_monitor(hwnd: HWND, monitor_area: Bounds) -> bool {
        unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return false;
            }
            rect.left <= monitor_area.left
                && rect.top <= monitor_area.top
                && rect.right >= monitor_area.right
                && rect.bottom >= monitor_area.bottom
        }
    }

    fn system_is_fullscreen_or_presentation() -> bool {
        unsafe {
            let Ok(state) = SHQueryUserNotificationState() else {
                return false;
            };
            state == QUNS_PRESENTATION_MODE || state == QUNS_RUNNING_D3D_FULL_SCREEN
        }
    }

    fn find_meeting_window(process_id: u32) -> Option<WindowCandidate> {
        unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let search = &mut *(lparam.0 as *mut WindowSearch);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }

            let mut window_process_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut window_process_id));
            if window_process_id != search.process_id || GetWindowTextLengthW(hwnd) <= 0 {
                return BOOL(1);
            }

            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return BOOL(1);
            }
            let width = i64::from(rect.right.saturating_sub(rect.left).max(0));
            let height = i64::from(rect.bottom.saturating_sub(rect.top).max(0));
            if width > 0 && height > 0 {
                search.candidates.push(WindowCandidate {
                    hwnd,
                    area: width * height,
                });
            }
            BOOL(1)
        }

        let mut search = WindowSearch {
            process_id,
            candidates: Vec::new(),
        };
        unsafe {
            if EnumWindows(
                Some(callback),
                LPARAM(&mut search as *mut WindowSearch as isize),
            )
            .is_err()
            {
                return None;
            }
        }
        search
            .candidates
            .into_iter()
            .max_by_key(|window| window.area)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::core::w;
        use windows::Win32::UI::Accessibility::NotifyWinEvent;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_POPUP, WS_VISIBLE,
        };

        // Native fixtures stay offscreen, have no taskbar entry, never activate,
        // and are destroyed on their creating test thread.
        struct Fixture(HWND);
        impl Fixture {
            fn new() -> Self {
                Self(unsafe {
                    CreateWindowExW(
                        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                        w!("STATIC"),
                        w!("Kiminola transition test"),
                        WS_POPUP | WS_VISIBLE,
                        -30000,
                        -30000,
                        800,
                        600,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()
                })
            }
            fn movement(&self) -> Movement {
                let start = window_bounds(self.0).unwrap();
                Movement {
                    hwnd: self.0,
                    start,
                    target: Bounds {
                        left: start.left + 200,
                        right: start.right + 100,
                        ..start
                    },
                }
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }

        #[test]
        fn native_pair_finishes_at_exact_rectangles_and_unhooks() {
            let _writer = WRITER.lock().unwrap();
            let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            let first = Fixture::new();
            let second = Fixture::new();
            let watched = [first.0, second.0];
            let hooks = Hooks::install(&watched).unwrap();
            let moves = [first.movement(), second.movement()];
            animate(&moves, &watched, generation, true).unwrap();
            for movement in &moves {
                assert_eq!(window_bounds(movement.hwnd).unwrap(), movement.target);
            }
            drop(hooks);
            assert!(WATCHED.with(|watched| watched.borrow().is_empty()));
        }

        #[test]
        fn native_move_start_on_either_window_cancels_without_snapping() {
            let _writer = WRITER.lock().unwrap();
            for index in 0..2 {
                let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
                let first = Fixture::new();
                let second = Fixture::new();
                let watched = [first.0, second.0];
                let _hooks = Hooks::install(&watched).unwrap();
                let moves = [first.movement(), second.movement()];
                let hwnd = watched[index].0 as usize;
                let notifier = std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(40));
                    unsafe {
                        NotifyWinEvent(
                            EVENT_SYSTEM_MOVESIZESTART,
                            HWND(hwnd as *mut _),
                            OBJID_WINDOW.0,
                            0,
                        );
                    }
                });
                animate(&moves, &watched, generation, true).unwrap();
                notifier.join().unwrap();
                assert!(
                    CANCELLED.with(Cell::get),
                    "native hook did not receive move-start"
                );
                for movement in &moves {
                    assert_ne!(window_bounds(movement.hwnd).unwrap(), movement.target);
                }
            }
        }

        #[test]
        fn native_replacement_abandons_the_old_target() {
            let _writer = WRITER.lock().unwrap();
            let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            let window = Fixture::new();
            let watched = [window.0];
            let _hooks = Hooks::install(&watched).unwrap();
            let moves = [window.movement()];
            let replacer = std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(40));
                GENERATION.fetch_add(1, Ordering::SeqCst);
            });
            animate(&moves, &watched, generation, true).unwrap();
            replacer.join().unwrap();
            assert_ne!(window_bounds(window.0).unwrap(), moves[0].target);
            let replacement = [window.movement()];
            animate(
                &replacement,
                &watched,
                GENERATION.load(Ordering::SeqCst),
                false,
            )
            .unwrap();
            assert_eq!(window_bounds(window.0).unwrap(), replacement[0].target);
        }

        #[test]
        fn destroyed_window_cancels_even_reduced_motion() {
            let _writer = WRITER.lock().unwrap();
            let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            let window = Fixture::new();
            let other = Fixture::new();
            let watched = [window.0, other.0];
            let _hooks = Hooks::install(&watched).unwrap();
            let moves = [window.movement(), other.movement()];
            let before = window_bounds(window.0).unwrap();
            drop(other);
            animate(&moves, &watched, generation, false).unwrap();
            assert_eq!(window_bounds(window.0).unwrap(), before);
        }

        #[test]
        fn splits_a_standard_display_into_two_thirds_and_one_third() {
            let work_area = Bounds {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            };
            let (meeting, notes) = split_work_area(work_area, 420, 96);

            assert_eq!(meeting.width(), 1280);
            assert_eq!(notes.width(), 640);
            assert_eq!(meeting.height(), 1080);
            assert_eq!(notes.left, meeting.right);
        }

        #[test]
        fn keeps_notes_usable_on_a_narrow_display() {
            let work_area = Bounds {
                left: 0,
                top: 0,
                right: 1024,
                bottom: 768,
            };
            let (meeting, notes) = split_work_area(work_area, 420, 96);

            assert_eq!(notes.width(), 420);
            assert_eq!(meeting.width(), 604);
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn apply(app: &tauri::AppHandle, meeting_process_id: u32) -> Result<(), String> {
    win32::apply(app, meeting_process_id)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn apply(_app: &tauri::AppHandle, _meeting_process_id: u32) -> Result<(), String> {
    Ok(())
}
