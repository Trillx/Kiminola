# Meeting detection validation

Validation on 2026-09-06:

These results cover the detector revision tested in this task. The concurrent
`Fix meeting prompt state` task subsequently began extending the same module
with recording-time suppression; rerun the combined suite after that work ends.

- ARM64 library suite: 87 passed, three live-only probes skipped. This includes
  all 26 detector/prompt regression tests.
- x64 library type check passed. All 26 detector/prompt tests also passed as an
  x64 executable under Windows ARM64 emulation using clean x64 native DLLs.
- The live Windows probe succeeded and saw Teams as a possible hint with no
  active audio processes. This is an idle-state check, not a live-call result.
- Live meeting prompts, headset switching, window placement, and recording
  still need the acceptance checks below. No installed app was updated.

The local x64 native cache contained ARM64 DLLs, and generated DLL symlinks
caused a copy-to-self build failure. The affected debug/examples symlinks were
preserved with `.before-meeting-detection` suffixes. x64 execution was validated
with architecture-checked DLLs freshly extracted from the existing x64 archive
under `kiminola/src-tauri/target/meeting-detection-x64-validation/`. Repair or
recreate the normal x64 native cache before using it for packaging.

Detection is an advisory local signal. Opening Teams or Zoom without active
audio should produce a quiet possible hint, not a recording prompt. Built-in
speakers and microphones, USB devices, and Bluetooth headsets are supported
inputs to detection. A headset is optional.

## Implementation

- Match helper audio to the meeting application's process ancestry. Keep
  independent instances separate and exclude Kimi Nola's own descendants.
- Use the process-family root for episode suppression and process-loopback
  recording. Resolve the visible window separately for Companion layout.
- Enumerate all active Windows render and capture endpoints, not only defaults.
- Normalize executable basenames before matching, including uppercase `.EXE`.
- Keep an unanswered prompt through one inactive poll. Two inactive polls or
  process exit end the episode. A restarted audio helper does not create a new
  episode while its parent application remains active.
- Continue episode tracking while full-screen or presentation mode defers
  prompts. Opening or closing full-screen mode updates the prompt overlay.

The browser signal remains coarse: a Google Meet title plus browser-family
audio is not proof that the audio belongs to that tab. The spec's generic
visible-app/audio fallback can also match non-meeting audio. Detection does not
start recording automatically.

## Automated checks

Run from `kiminola/src-tauri` in PowerShell with prepared native dependencies:

```powershell
$nativeLib = Join-Path (Get-Location) 'sherpa-onnx-v1.13.5-win-arm64-shared-MD-Release-lib/lib'
$env:SHERPA_ONNX_LIB_DIR = $nativeLib
$env:ORT_LIB_PATH = $nativeLib
$env:ORT_PREFER_DYNAMIC_LINK = '1'
$env:PATH = 'C:/Program Files/LLVM/bin;' + $nativeLib + ';' + $env:PATH
cargo test --lib
```

For x64, use the matching `win-x64` library directory and pass
`--target x86_64-pc-windows-msvc`. Do not mix ARM64 and x64 DLL directories.

## Live read-only probe

With the same environment, this invokes the actual Windows collectors and
classifier without opening Kimi Nola or starting recording:

```powershell
cargo test --lib meeting_presence::tests::inspect_live_windows_detection -- --ignored --exact --nocapture
```

It prints signal counts, coarse application labels, whether a visible window
was found, and whether full-screen mode defers prompts. It does not print window
titles, process names, URLs, or process IDs. It does not prove notification UI,
window placement, or recording behavior in the desktop app.

## Live call acceptance

Run a desktop build containing these changes with Meeting detection enabled and
unpaused. Check Teams and Zoom separately. Repeat with built-in audio and an
explicitly selected non-default device when one is available.

| Action | Expected result |
| --- | --- |
| Open the app without joining a call | Possible hint only, unless the app actually opens an audio session |
| Join a call with active audio | One prompt after a detector poll, normally about four seconds |
| Dismiss the prompt and remain in the call | No repeated prompt |
| Mute or briefly switch audio devices | No new episode unless Windows reports no active app audio for two polls |
| Leave the call, wait at least two polls, then rejoin | A fresh prompt once the old audio sessions have become inactive |
| Join while full-screen, then exit full-screen | Prompt deferred until full-screen ends |
| End one meeting while full-screen and join another | The new episode can prompt after full-screen ends |
| Choose Start recording from the prompt | Correct meeting window arranged; the meeting's audio captured |
| Run another app using WebView audio alongside idle Teams | Unrelated helper audio must not activate a Teams prompt |

Mute does not necessarily close an audio session. Windows audio-session state,
not microphone amplitude or a vendor's call status API, defines activity here.
Live Teams/Zoom calls, alternate-device switching, notification UI, and actual
recording remain required acceptance checks; synthetic tests alone cannot prove
those behaviors.
