//! The Meeting presence probe seam: where detection snapshots come from.
//!
//! - [`Win32Probe`] reads live processes (ToolHelp), visible top-level windows
//!   and active Core Audio sessions on every endpoint, then classifies them.
//! - `FixtureProbe` (tests) classifies a scripted process/window/audio table.
//!
//! Both also answer whether prompts must wait because the foreground app is
//! full-screen or Windows is in presentation mode.

use super::classifier::DetectionSnapshot;
#[cfg(target_os = "windows")]
use super::classifier::{classify_windows, ProcessInfo};
#[cfg(target_os = "windows")]
use std::collections::{HashMap, HashSet};

pub(crate) trait PresenceProbe: Send + Sync {
    /// One read-only snapshot of local meeting signals.
    fn detect(&self) -> Result<DetectionSnapshot, String>;
    /// Prompts are deferred while a full-screen app or presentation is in front.
    fn prompt_is_deferred(&self) -> bool;
}

/// Live Windows signals.
pub(crate) struct Win32Probe;

#[cfg(target_os = "windows")]
impl PresenceProbe for Win32Probe {
    fn detect(&self) -> Result<DetectionSnapshot, String> {
        let processes = enumerate_processes()?;
        let windows = enumerate_visible_windows()?;
        let active_audio = enumerate_active_audio_processes()?;
        Ok(classify_windows(
            processes,
            windows,
            active_audio,
            std::process::id(),
        ))
    }

    fn prompt_is_deferred(&self) -> bool {
        foreground_defers_prompts()
    }
}

#[cfg(not(target_os = "windows"))]
impl PresenceProbe for Win32Probe {
    fn detect(&self) -> Result<DetectionSnapshot, String> {
        Ok(DetectionSnapshot::default())
    }

    fn prompt_is_deferred(&self) -> bool {
        false
    }
}

/// Whether a full-screen foreground app or presentation mode should hold
/// prompts back. Also used by the Background companion before showing them.
#[cfg(target_os = "windows")]
pub(crate) fn foreground_defers_prompts() -> bool {
    use std::mem::size_of;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    };

    unsafe {
        if let Ok(notification_state) = SHQueryUserNotificationState() {
            if notification_state == QUNS_PRESENTATION_MODE
                || notification_state == QUNS_RUNNING_D3D_FULL_SCREEN
            {
                return true;
            }
        }

        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return false;
        }

        let mut foreground_pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        if foreground_pid == std::process::id() {
            return false;
        }

        let mut window_rect = RECT::default();
        if GetWindowRect(foreground, &mut window_rect).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
        if monitor.0.is_null() {
            return false;
        }

        let mut monitor_info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
            return false;
        }

        let monitor_rect = monitor_info.rcMonitor;
        window_rect.left <= monitor_rect.left
            && window_rect.top <= monitor_rect.top
            && window_rect.right >= monitor_rect.right
            && window_rect.bottom >= monitor_rect.bottom
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn foreground_defers_prompts() -> bool {
    false
}

#[cfg(target_os = "windows")]
fn enumerate_processes() -> Result<HashMap<u32, ProcessInfo>, String> {
    use std::mem::size_of;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|e| format!("process snapshot failed: {e}"))?;
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut processes = HashMap::new();
        let first = Process32FirstW(snapshot, &mut entry);
        if first.is_ok() {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(entry.szExeFile.len());
                processes.insert(
                    entry.th32ProcessID,
                    ProcessInfo {
                        name: String::from_utf16_lossy(&entry.szExeFile[..end]),
                        parent_id: entry.th32ParentProcessID,
                    },
                );
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        Ok(processes)
    }
}

#[cfg(target_os = "windows")]
fn enumerate_visible_windows() -> Result<HashMap<u32, Vec<String>>, String> {
    use windows::Win32::Foundation::{BOOL, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
    };

    unsafe extern "system" fn callback(
        hwnd: windows::Win32::Foundation::HWND,
        lparam: LPARAM,
    ) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        if pid == 0 {
            return BOOL(1);
        }
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return BOOL(1);
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let written = GetWindowTextW(hwnd, &mut buffer);
        let title = String::from_utf16_lossy(&buffer[..written.max(0) as usize]);
        if !title.trim().is_empty() {
            let windows = &mut *(lparam.0 as *mut HashMap<u32, Vec<String>>);
            windows.entry(pid).or_default().push(title);
        }
        BOOL(1)
    }

    let mut windows = HashMap::new();
    unsafe {
        EnumWindows(Some(callback), LPARAM(&mut windows as *mut _ as isize))
            .map_err(|e| format!("window enumeration failed: {e}"))?;
    }
    Ok(windows)
}

#[cfg(target_os = "windows")]
fn enumerate_active_audio_processes() -> Result<HashSet<u32>, String> {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eAll, AudioSessionStateActive, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| format!("COM init failed: {e}"))?;
        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("audio enumerator failed: {e}"))?;
            let mut pids = HashSet::new();
            // Calls can use an explicitly selected headset/communications device
            // instead of either Windows default. Inspect render and capture on
            // every active endpoint, deduplicating sessions by process ID.
            let devices = enumerator
                .EnumAudioEndpoints(eAll, DEVICE_STATE_ACTIVE)
                .map_err(|e| format!("audio endpoint enumeration failed: {e}"))?;
            let device_count = devices
                .GetCount()
                .map_err(|e| format!("audio endpoint count failed: {e}"))?;
            for device_index in 0..device_count {
                let Ok(device) = devices.Item(device_index) else {
                    continue;
                };
                let Ok(manager): Result<IAudioSessionManager2, _> =
                    device.Activate(CLSCTX_ALL, None)
                else {
                    continue;
                };
                let Ok(sessions) = manager.GetSessionEnumerator() else {
                    continue;
                };
                let Ok(count) = sessions.GetCount() else {
                    continue;
                };
                for index in 0..count {
                    let Ok(control) = sessions.GetSession(index) else {
                        continue;
                    };
                    let Ok(control2) =
                        control.cast::<windows::Win32::Media::Audio::IAudioSessionControl2>()
                    else {
                        continue;
                    };
                    if control.GetState().ok() == Some(AudioSessionStateActive) {
                        if let Ok(pid) = control2.GetProcessId() {
                            if pid != 0 {
                                pids.insert(pid);
                            }
                        }
                    }
                }
            }
            Ok::<HashSet<u32>, String>(pids)
        })();
        CoUninitialize();
        result
    }
}

/// Scripted signals: `(pid, parent_pid, executable)`, `(pid, window title)`
/// and the PIDs with active audio, classified as if Kimi Nola were PID 999.
#[cfg(test)]
pub(crate) struct FixtureProbe {
    pub(crate) processes: Vec<(u32, u32, &'static str)>,
    pub(crate) windows: Vec<(u32, &'static str)>,
    pub(crate) audio: Vec<u32>,
    pub(crate) deferred: bool,
}

#[cfg(test)]
impl PresenceProbe for FixtureProbe {
    fn detect(&self) -> Result<DetectionSnapshot, String> {
        use super::classifier::{classify_windows, ProcessInfo};
        use std::collections::HashMap;

        let mut windows: HashMap<u32, Vec<String>> = HashMap::new();
        for &(pid, title) in &self.windows {
            windows.entry(pid).or_default().push(title.into());
        }
        Ok(classify_windows(
            self.processes
                .iter()
                .map(|&(pid, parent_id, name)| {
                    (
                        pid,
                        ProcessInfo {
                            name: name.into(),
                            parent_id,
                        },
                    )
                })
                .collect(),
            windows,
            self.audio.iter().copied().collect(),
            999,
        ))
    }

    fn prompt_is_deferred(&self) -> bool {
        self.deferred
    }
}

/// Classify a scripted table through the fixture probe.
#[cfg(test)]
pub(crate) fn detect_fixture(
    processes: &[(u32, u32, &'static str)],
    windows: &[(u32, &'static str)],
    audio: &[u32],
) -> DetectionSnapshot {
    FixtureProbe {
        processes: processes.to_vec(),
        windows: windows.to_vec(),
        audio: audio.to_vec(),
        deferred: false,
    }
    .detect()
    .expect("fixture detection")
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "read-only live Windows probe; run explicitly while joining/leaving a test call"]
    fn inspect_live_windows_detection() {
        let processes = super::enumerate_processes().expect("process enumeration");
        let windows = super::enumerate_visible_windows().expect("window enumeration");
        let audio = super::enumerate_active_audio_processes().expect("audio endpoint enumeration");
        println!(
            "Local signals: {} processes, {} window owners, {} active audio processes",
            processes.len(),
            windows.len(),
            audio.len()
        );
        let result = super::classify_windows(processes, windows, audio, std::process::id());
        for detection in &result.detections {
            println!(
                "Likely: {}; visible window: {}",
                detection.app_label,
                detection.window_process_id.is_some()
            );
        }
        for hint in &result.possible_hints {
            if !result
                .detections
                .iter()
                .any(|d| d.process_id == hint.process_id)
            {
                println!("Possible only: {}", hint.app_label);
            }
        }
        println!(
            "Prompt deferred by full-screen/presentation mode: {}",
            super::foreground_defers_prompts()
        );
    }
}
