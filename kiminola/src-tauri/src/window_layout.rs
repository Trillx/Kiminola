//! One-shot Windows window arrangement for the detected-meeting handoff.
//!
//! The layout is deliberately applied only when the user explicitly starts a
//! recording from a meeting-presence prompt. There is no resize/move watcher,
//! so any later user change remains authoritative.

#[cfg(target_os = "windows")]
mod win32 {
    use std::mem::size_of;

    use tauri::{Manager, PhysicalPosition, PhysicalSize, Position, Size};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId,
        IsIconic, IsWindowVisible, IsZoomed, SetWindowPos, ShowWindow, HWND_TOP,
        SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SW_RESTORE,
    };

    const MIN_NOTES_WIDTH: i32 = 420;
    const MIN_MEETING_WIDTH: i32 = 480;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Bounds {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
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

        fn width(self) -> i32 {
            self.right.saturating_sub(self.left)
        }

        fn height(self) -> i32 {
            self.bottom.saturating_sub(self.top)
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
        let main = app
            .get_webview_window("main")
            .ok_or_else(|| "main window is unavailable".to_string())?;
        let meeting_window = find_meeting_window(meeting_process_id);

        let work_area = if let Some(candidate) = meeting_window {
            match monitor_bounds(candidate.hwnd) {
                Ok((monitor_area, work_area)) => {
                    let meeting_is_fullscreen = system_is_fullscreen_or_presentation()
                        || covers_monitor(candidate.hwnd, monitor_area);
                    let (meeting_bounds, notes_bounds) = split_work_area(work_area);
                    if meeting_is_fullscreen {
                        eprintln!("[window-layout] full-screen meeting window left untouched");
                    } else {
                        if let Err(error) = arrange_meeting_window(candidate.hwnd, meeting_bounds) {
                            eprintln!("[window-layout] meeting window arrangement skipped: {error}");
                        }
                    }
                    position_main_window(&main, notes_bounds)?;
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("[window-layout] meeting monitor unavailable: {error}");
                    fallback_work_area(app, &main)?
                }
            }
        } else {
            fallback_work_area(app, &main)?
        };

        let (_, notes_bounds) = split_work_area(work_area);
        position_main_window(&main, notes_bounds)
    }

    fn split_work_area(work_area: Bounds) -> (Bounds, Bounds) {
        let total_width = work_area.width();
        let notes_width = (total_width / 3)
            .max(MIN_NOTES_WIDTH)
            .min(total_width.saturating_sub(MIN_MEETING_WIDTH).max(1));
        let meeting_right = work_area.right.saturating_sub(notes_width);

        (
            Bounds {
                left: work_area.left,
                top: work_area.top,
                right: meeting_right,
                bottom: work_area.bottom,
            },
            Bounds {
                left: meeting_right,
                top: work_area.top,
                right: work_area.right,
                bottom: work_area.bottom,
            },
        )
    }

    fn position_main_window(main: &tauri::WebviewWindow, bounds: Bounds) -> Result<(), String> {
        main.show()
            .map_err(|error| format!("could not show main window: {error}"))?;
        main.unminimize()
            .map_err(|error| format!("could not restore main window: {error}"))?;
        main.set_size(Size::Physical(PhysicalSize::new(
            bounds.width().max(1) as u32,
            bounds.height().max(1) as u32,
        )))
        .map_err(|error| format!("could not size main window: {error}"))?;
        main.set_position(Position::Physical(PhysicalPosition::new(
            bounds.left,
            bounds.top,
        )))
        .map_err(|error| format!("could not position main window: {error}"))?;
        main.set_focus()
            .map_err(|error| format!("could not focus main window: {error}"))?;
        Ok(())
    }

    fn arrange_meeting_window(hwnd: HWND, bounds: Bounds) -> Result<(), String> {
        unsafe {
            if IsZoomed(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            SetWindowPos(
                hwnd,
                HWND_TOP,
                bounds.left,
                bounds.top,
                bounds.width().max(1),
                bounds.height().max(1),
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            )
            .map_err(|error| format!("SetWindowPos failed: {error}"))?;
        }
        Ok(())
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
            Ok((Bounds::from_rect(info.rcMonitor), Bounds::from_rect(info.rcWork)))
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
        search.candidates.into_iter().max_by_key(|window| window.area)
    }

    #[cfg(test)]
    mod tests {
        use super::{split_work_area, Bounds};

        #[test]
        fn splits_a_standard_display_into_two_thirds_and_one_third() {
            let work_area = Bounds {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            };
            let (meeting, notes) = split_work_area(work_area);

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
            let (meeting, notes) = split_work_area(work_area);

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
