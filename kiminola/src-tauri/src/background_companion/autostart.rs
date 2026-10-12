//! Start-with-Windows registration for the Background companion: a per-user
//! `Run` entry that launches OatWave with `--background`.

/// Add (or remove) the `HKCU\...\Run\OatWave` entry and remove the former
/// `KimiNola` entry. Removing a missing entry is not an error.
#[cfg(target_os = "windows")]
pub(crate) fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    use std::mem::size_of;
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };

    let exe = std::env::current_exe().map_err(|e| format!("current exe unavailable: {e}"))?;
    let mut key = windows::Win32::System::Registry::HKEY::default();
    unsafe {
        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_SET_VALUE,
            &mut key,
        );
        if opened.0 != 0 {
            return Err(format!(
                "could not open Windows startup settings: {opened:?}"
            ));
        }

        let result = if enabled {
            let value = format!("\"{}\" --background\0", exe.display());
            let utf16: Vec<u16> = value.encode_utf16().collect();
            let bytes = std::slice::from_raw_parts(
                utf16.as_ptr() as *const u8,
                utf16.len() * size_of::<u16>(),
            );
            RegSetValueExW(key, w!("OatWave"), 0, REG_SZ, Some(bytes))
        } else {
            RegDeleteValueW(key, w!("OatWave"))
        };
        if result.0 == 0 || (!enabled && result.0 == 2) {
            let legacy_result = RegDeleteValueW(key, w!("KimiNola"));
            if legacy_result.0 != 0 && legacy_result.0 != 2 {
                let _ = RegCloseKey(key);
                return Err(format!(
                    "could not remove the previous Windows startup entry: {legacy_result:?}"
                ));
            }
        }
        let _ = RegCloseKey(key);
        if !enabled && (result.0 == 0 || result.0 == 2) {
            return Ok(());
        }
        if result.0 != 0 {
            return Err(format!(
                "could not update Windows startup settings: {result:?}"
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn set_start_with_windows(_enabled: bool) -> Result<(), String> {
    Ok(())
}
