# Mic permission detection on Windows

Type: research
Status: resolved
Blocked by:

## Question

Decide how the first-run wizard detects and requests microphone permission on Windows in a Tauri 2 app.

Specifically:
- Does `cpal`'s default input device enumeration alone trigger the Windows permission dialog reliably?
- Is an explicit Windows API call (e.g., `winrt-windows.media.devices` or `IMMDevice` activation) needed to surface the system permission prompt?
- How do we distinguish between "permission not yet asked", "permission denied", and "no microphone hardware"?
- What is the fallback UX if permission is denied? (Open Windows Settings, show instructions, retry button.)
- Does the optional 3-second mic check require a temporary capture stream, and can it reuse the same cpal stream logic already in `recording.rs`?

Research the current Windows privacy permission behavior for desktop apps and the Tauri/cpal interaction, then lock the detection strategy and error-state UX.

## Resolution

Resolved by research subagent. Full findings in `.scratch/kiminola/research/05-mic-permission-detection-windows.md`.

### Decisions

- **cpal `default_input_device()` is not enough** — it only enumerates the endpoint and does not reliably trigger the Windows privacy dialog.
- **Permission request requires a real capture stream probe**. Run a short (≈3 s) cpal capture stream; the prompt (or `E_ACCESSDENIED`) appears when cpal calls `ActivateAudioInterfaceAsync` + `IAudioClient::Initialize` inside `build_input_stream`.
- **State distinction**:
  - **No hardware**: `default_input_device()` returns `None` / `input_devices()` is empty.
  - **Not yet asked**: on Windows 10 1903+, `Windows.Security.Authorization.AppCapabilityAccess.AppCapability::CheckAccess("Microphone")` returns `UserPromptRequired`.
  - **Denied**: same API returns `DeniedBySystem`/`DeniedByUser`, or the probe fails with `E_ACCESSDENIED` (`0x80070005`).
  - **Allowed**: API returns `Allowed` and the probe stream yields non-zero audio.
- **Fallback UX**: open `ms-settings:privacy-microphone`, explain the global **"Allow desktop apps to access your microphone"** toggle, and provide a Retry button.
- **3-second mic check**: reuse the existing cpal stream-building logic from `recording.rs` (`build_mic_stream` / `build_input_stream`) without the ASR/resampling/loopback pieces.
