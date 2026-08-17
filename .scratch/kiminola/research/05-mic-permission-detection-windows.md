# Research: Microphone permission detection on Windows for the first-run wizard

Date: 2026-08-16
Ticket: [issues/17-mic-permission-detection-on-windows.md](../issues/17-mic-permission-detection-on-windows.md)
Scope: Windows 10/11 desktop (x64 + ARM64) first-run gating for microphone access in a Tauri 2 + Rust + cpal app.

---

## TL;DR — recommendation

1. **Do not rely on `cpal::default_input_device()` alone to trigger the permission prompt.** It only calls `IMMDeviceEnumerator::GetDefaultAudioEndpoint(eCapture)`, which returns the default endpoint object without activating the capture interface. The Windows privacy prompt is triggered later, when the audio interface is actually activated/initialized.
2. **Request permission explicitly by attempting a real, short capture stream.** The cleanest approach is to build a temporary cpal input stream (≈3 seconds) in the wizard. This is the same code path as `recording.rs` and forces `ActivateAudioInterfaceAsync` + `IAudioClient::Initialize`, which is when Windows shows the system prompt (or returns `E_ACCESSDENIED` if denied).
3. **Distinguish states by combining a WinRT API check with the cpal probe:**
   - **No hardware / no default mic**: `cpal::default_input_device()` returns `None` or `input_devices()` is empty.
   - **Permission not yet asked / promptable**: `Windows.Security.Authorization.AppCapabilityAccess.AppCapability::CheckAccess("Microphone")` returns `UserPromptRequired` (Windows 10 1903+).
   - **Denied**: the same API returns `DeniedByUser`/`DeniedBySystem`, or cpal stream creation fails with `E_ACCESSDENIED` (`0x80070005`).
   - **Allowed**: the API returns `Allowed` and the probe stream delivers non-zero audio levels.
4. **Fallback UX**: if denied, open `ms-settings:privacy-microphone` (the Settings > Privacy > Microphone page), explain that desktop apps need the global **"Allow desktop apps to access your microphone"** toggle, and provide a **Retry** button that re-runs the probe.
5. **The 3-second level check can reuse the cpal stream logic from `recording.rs`.** It does not need the ASR pipeline; it only needs a short-lived `build_input_stream` with an RMS/peak level callback, then drop the stream.


---

## 1. Where exactly does the Windows privacy prompt fire?

### 1.1 What cpal `default_input_device()` does

Reading cpal's WASAPI backend (`src/host/wasapi/device.rs`):

```rust
pub fn default_input_device() -> Option<Device> {
    current_default_endpoint(Audio::eCapture).map(|_| Device::default_input())
}
```

`current_default_endpoint` wraps [`IMMDeviceEnumerator::GetDefaultAudioEndpoint`](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-immdeviceenumerator-getdefaultaudioendpoint) with `eCapture`/`eConsole`. This only retrieves the endpoint object. Microsoft docs note that if no device exists it returns `ERROR_NOT_FOUND` and `ppEndpoint = NULL`; nothing here prompts the user.

**Conclusion: device enumeration alone is NOT enough to trigger the microphone permission dialog.**

### 1.2 What does trigger the prompt

In cpal, the first real use of the device happens in `ensure_future_audio_client` / `build_audioclient`:

```rust
DeviceHandle::DefaultInput => {
    let path = Com::StringFromIID(&Audio::DEVINTERFACE_AUDIO_CAPTURE)?;
    activate_audio_interface_sync(path, activation_timeout).map_err(Error::from)?
}
```

This calls [`ActivateAudioInterfaceAsync`](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-activateaudiointerfaceasync) with the audio capture device interface GUID, which obtains an `IAudioClient`. The subsequent `IAudioClient::Initialize` call in `build_input_stream_raw_inner` is the point at which Windows enforces the privacy setting and may surface the prompt.

Community evidence confirms this: a [fmedia issue](https://github.com/stsaz/fmedia/issues/71) reports `IAudioClient::Initialize` returning `0x80070005` (`E_ACCESSDENIED`) when Windows microphone privacy is disabled. The Stack Overflow discussion ["Check microphone privacy setting on WPF"](https://stackoverflow.com/questions/51788371/check-microphone-privacy-setting-on-wpf) reaches the same conclusion for desktop apps: the reliable signal is a failed capture initialization, not enumeration.

**Conclusion: the prompt (or access-denied error) appears when the app actually tries to initialize a capture stream.**

### 1.3 Desktop-app permission model

Unlike UWP/Store apps, **desktop apps do not get per-app microphone toggles**. Windows controls them with a single global switch:

- Settings > Privacy & security > Microphone > **"Allow desktop apps to access your microphone"**

This is documented implicitly by the "Allow desktop apps to access your microphone" section in every Windows 10/11 microphone privacy guide (e.g., [How-To Geek](https://www.howtogeek.com/395296/fix-my-microphone-doesnt-work-on-windows-10/), [MakeUseOf](https://www.makeuseof.com/tag/fix-microphone-problems-windows-10/)) and confirmed by Microsoft's Q&A: ["Why doesn't Windows 11 allow us to set privacy settings for SPECIFIC desktop applications regarding microphone access?"](https://learn.microsoft.com/en-us/answers/questions/4102473/why-doesnt-windows-11-allow-us-to-set-privacy-sett)

Because there is no per-desktop-app entry in the privacy list, the wizard's job is to make sure the global toggle is on, not to ask for a per-app grant.


---

## 2. How to distinguish the three states

### 2.1 "No microphone hardware"

Use cpal's standard device query:

```rust
let host = cpal::default_host();
match host.default_input_device() {
    Some(_) => { /* hardware present */ }
    None => { /* no default mic; also check host.input_devices().count() == 0 */ }
}
```

If `default_input_device()` returns `None`, the machine has no enabled capture endpoint. This is a hardware/driver state, not a permission state.

### 2.2 "Permission not yet asked" / "Promptable"

For Windows 10 version 1903 (build 18362) and later, use the WinRT API [`Windows.Security.Authorization.AppCapabilityAccess`](https://learn.microsoft.com/en-us/uwp/api/windows.security.authorization.appcapabilityaccess.appcapability?view=winrt-26100):

```cpp
auto cap = Windows::Security::Authorization::AppCapabilityAccess::AppCapability::Create(L"Microphone");
auto status = cap.CheckAccess();
```

`CheckAccess` returns [`AppCapabilityAccessStatus`](https://github.com/MicrosoftDocs/winrt-api/blob/docs/windows.security.authorization.appcapabilityaccess/appcapabilityaccessstatus.md):

| Value | Meaning |
|---|---|
| `DeniedBySystem` (0) | System-wide mic access is off. |
| `NotDeclaredByApp` (1) | Capability not declared (relevant for packaged apps; desktop apps normally see a different path). |
| `DeniedByUser` (2) | User has denied access. |
| `UserPromptRequired` (3) | Not yet determined; a prompt may appear on first real use. |
| `Allowed` (4) | Access is granted. |

The docs explicitly state: *"'Checking' access will simply query your status and is guaranteed to not prompt, as such may return the status 'UserPromptRequired'."* This is the exact "not yet asked" signal.

From Rust/windows-rs, add the `Windows_Security_Authorization_AppCapabilityAccess` feature and call `AppCapability::Create(hstring!("Microphone"))?.CheckAccess()?`.

### 2.3 "Permission denied"

Two ways to detect this:

1. **Preferred (programmatic):** `CheckAccess()` returns `DeniedBySystem` or `DeniedByUser`.
2. **Fallback (works on older Windows):** attempt the cpal probe stream and catch an initialization error whose HRESULT is `E_ACCESSDENIED` (`0x80070005`). cpal currently returns this as a generic backend error string, so the fallback is best-effort by grepping for `0x80070005` / `E_ACCESSDENIED` / `Access is denied` in the error text, or by calling WASAPI directly.

Note the Microsoft Q&A caveat: ["In previous versions [before 1903], no Windows APIs can directly obtain microphone permission status... calling `IAudioClient::Initialize`, when the return value is `E_ACCESSDENIED`, judge that there is no microphone permission."](https://learn.microsoft.com/en-nz/answers/questions/811448/in-windows-system-how-to-obtain-whether-the-applic)

### 2.4 Recommended state machine for the wizard

```
1. host.default_input_device() == None?
   → NoMicHardware (show "Please connect a microphone")

2. AppCapability.CheckAccess("Microphone") available?
   a. Allowed      → skip gate, or optionally run 3s level check for confidence
   b. Denied*      → PermissionDenied (open Settings, show instructions, Retry)
   c. UserPromptRequired → run 3s probe stream; this should trigger the prompt

3. If CheckAccess unavailable (< Windows 10 1903) or inconclusive:
   → run 3s probe stream
      - success + non-zero level → Allowed
      - fails with E_ACCESSDENIED → PermissionDenied
      - fails for other reasons   → NoMicHardware or generic error
```


---

## 3. Tauri's role (none, for this)

Tauri v2's permission model is an **ACL for webview→backend commands** (see [Core Permissions](https://v2.tauri.app/reference/acl/core-permissions/)). It has no API to check or request Windows microphone privacy status. The macOS permission plugin [`tauri-plugin-macos-permissions`](https://github.com/ayangweb/tauri-plugin-macos-permissions) exists, but there is no equivalent maintained Tauri plugin for Windows.

Therefore the detection must live in the Rust backend and be exposed to the frontend via a custom Tauri command.

---

## 4. Implementing the 3-second mic level check

### 4.1 Reuse the existing cpal path

`kiminola/src-tauri/src/recording.rs` already has the exact pattern:

- `cpal::default_host().default_input_device()` to pick the mic.
- `device.default_input_config()` to get the config.
- `device.build_input_stream::<T>(config, data_callback, error_callback, None)` to open the stream.
- `stream.play()` to start capture.

The wizard's level check can reuse `build_mic_stream`/`build_input_stream` logic almost unchanged.

### 4.2 What the level check needs that recording does not

- **Shorter lifetime:** run for exactly 3 seconds, then drop the stream.
- **No resampling/ASR:** read raw device-format samples and compute RMS or peak level.
- **No loopback:** only mic.
- **No keepalive channel:** the callback can push level values to a `std::sync::mpsc` channel or update an `Arc<AtomicF32>`.

A minimal sketch (mirrors `recording.rs` conventions):

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub async fn probe_microphone_level(duration: Duration) -> Result<MicrophoneProbeResult, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no default microphone found".to_string())?;

    let config = device
        .default_input_config()
        .map_err(|e| format!("failed to get mic config: {e}"))?;

    let channels = config.config().channels as usize;
    let mut sum_squares: f64 = 0.0;
    let mut sample_count: u64 = 0;
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    let stream = device.build_input_stream(
        &config.config(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            if !running_clone.load(Ordering::Relaxed) {
                return;
            }
            for chunk in data.chunks(channels.max(1)) {
                let mono: f32 = chunk.iter().map(|&s| s).sum::<f32>() / channels.max(1) as f32;
                sum_squares += (mono as f64) * (mono as f64);
                sample_count += 1;
            }
        },
        |err| eprintln!("probe stream error: {err}"),
        None,
    ).map_err(|e| format!("failed to build probe stream: {e}"))?;

    stream.play().map_err(|e| format!("failed to play probe stream: {e}"))?;
    tokio::time::sleep(duration).await;
    running.store(false, Ordering::Relaxed);
    drop(stream);

    let rms = if sample_count > 0 {
        (sum_squares / sample_count as f64).sqrt() as f32
    } else {
        0.0
    };

    Ok(MicrophoneProbeResult {
        rms_dbfs: 20.0 * rms.max(1e-10).log10(),
        sample_count,
    })
}
```

Notes:
- The `running` flag is not strictly required because dropping `stream` stops capture, but it prevents callback work after the sleep returns.
- The `sum_squares`/`sample_count` in the closure must be captured by reference if you want to read them after drop; in production, push per-buffer RMS values through a channel and aggregate outside the callback.
- If permission is denied, `build_input_stream` or `stream.play()` will fail with a backend error. Map that to the denied state.

### 4.3 Threading note

`recording.rs` already creates cpal streams on a dedicated OS thread because `cpal::Stream` is `!Send` on Windows. The wizard probe can run on a blocking tokio task or the same audio thread. If reusing the existing `AudioThread` infrastructure is desirable, add a `Probe` variant to `AudioCommand`. However, for a one-shot 3-second check, a dedicated temporary thread is simpler and avoids coupling to the recording lifecycle.


---

## 5. Fallback UX when permission is denied

### 5.1 Open the right Settings page

Use the `ms-settings:privacy-microphone` URI. Microsoft documents this in [Launch Windows Settings](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-settings):

```rust
use tauri_plugin_opener::OpenerExt;

app.opener()
    .open_url("ms-settings:privacy-microphone", None::<&str>)
    .ok();
```

The project already depends on `tauri-plugin-opener`.

### 5.2 What to tell the user

Because desktop apps are gated by a global toggle, the instructions should be explicit:

1. Open **Settings > Privacy & security > Microphone**.
2. Turn **Microphone access** on.
3. Turn **Allow desktop apps to access your microphone** on.
4. Return to Kiminola and click **Retry**.

### 5.3 Retry behavior

The Retry button simply re-runs the probe stream. If the user enabled the toggle, the probe should now succeed. If it still fails, the app stays on the wizard screen with a more emphatic message.

---

## 6. Open questions / spike recommendations

1. **Does the first-use prompt actually appear for an unsigned desktop app?** Microsoft documentation is ambiguous about whether desktop apps get a one-time system prompt or only the global toggle. The safest assumption is: no per-app prompt, just the global toggle. Verify on a fresh Windows VM where the global toggle is on but Kiminola has never run.
2. **Does the prompt appear at `ActivateAudioInterfaceAsync` or at `IAudioClient::Initialize`?** For implementation timing, either is fine because cpal bundles both in `build_input_stream`; but if we ever want to show a spinner *before* the prompt, knowing the exact call matters.
3. **windows-rs feature name for `AppCapabilityAccess`.** The project uses `windows = "0.58"`. Verify the correct feature string (likely `Windows_Security_Authorization_AppCapabilityAccess`) in the generated docs, and confirm it is available in 0.58. If it is missing or nightly-only, fall back to the `E_ACCESSDENIED` probe method.
4. **ARM64 behavior.** The permission APIs and cpal WASAPI path are the same on ARM64, but test on Snapdragon hardware because ARM64 Windows has had WASAPI loopback quirks; while mic capture is less exotic, a smoke test is worthwhile.

---

## 7. Sources

- [cpal WASAPI device.rs source](https://raw.githubusercontent.com/RustAudio/cpal/master/src/host/wasapi/device.rs) — `default_input_device`, `ensure_future_audio_client`, `build_input_stream_raw_inner` implementation
- [Microsoft Learn: `IMMDeviceEnumerator::GetDefaultAudioEndpoint`](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-immdeviceenumerator-getdefaultaudioendpoint)
- [Microsoft Learn: `ActivateAudioInterfaceAsync`](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-activateaudiointerfaceasync)
- [Microsoft Learn: `AppCapability` class](https://learn.microsoft.com/en-us/uwp/api/windows.security.authorization.appcapabilityaccess.appcapability?view=winrt-26100)
- [Microsoft Docs: `AppCapabilityAccessStatus` enum](https://github.com/MicrosoftDocs/winrt-api/blob/docs/windows.security.authorization.appcapabilityaccess/appcapabilityaccessstatus.md)
- [Microsoft Learn: `AppCapability.CheckAccess` method](https://learn.microsoft.com/en-us/uwp/api/windows.security.authorization.appcapabilityaccess.appcapability.checkaccess?view=winrt-26100)
- [Microsoft Q&A: "In Windows system, how to obtain whether the application has microphone permission through C++ code"](https://learn.microsoft.com/en-nz/answers/questions/811448/in-windows-system-how-to-obtain-whether-the-applic) — C++/WinRT example and pre-1903 fallback discussion
- [Microsoft Learn: "How to get Microphone privacy setting programmatically"](https://learn.microsoft.com/en-us/answers/questions/14283/how-to-get-microphone-privacy-setting-programmatic) — `MediaCapture.InitializeAsync` prompt behavior
- [Stack Overflow: "Check microphone privacy setting on WPF"](https://stackoverflow.com/questions/51788371/check-microphone-privacy-setting-on-wpf) — desktop apps must probe by attempting capture
- [GitHub: fmedia issue #71 — `IAudioClient::Initialize` returns `0x80070005` when mic privacy is disabled](https://github.com/stsaz/fmedia/issues/71)
- [Tauri v2 Core Permissions](https://v2.tauri.app/reference/acl/core-permissions/) — Tauri has no Windows mic permission API
- [Microsoft Learn: Launch Windows Settings](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-settings) — `ms-settings:privacy-microphone`
