# Cross-app meeting-presence hints without recording audio

- **Researched:** 2026-09-21.
- **Scope:** Microsoft API documentation, Chromium documentation/source, and local source inspection. This research did not run live meeting or hardware tests. Subsequent implementation checks are recorded in [meeting detection validation](../meeting-detection-validation.md).
- **Constraints:** [CONTEXT.md](../../CONTEXT.md) and [SPEC.md §8.1](../../SPEC.md#81-post-mvp-meeting-presence-prompts): opt-in, local, advisory; the detector never captures audio or starts recording. No raw executable names, window titles, URLs, or detector history persisted/shown.

## Recommendation

**Yes: detect meeting-like activity across app names, not meetings with certainty.** A useful generic fallback is a visible application plus sustained active input- and output-endpoint sessions attributable to that same application/process family. Ask the existing uncertain question; never auto-record. Keep one-direction or ambiguous activity quiet. This is a **proposed heuristic**, not a Windows “in meeting” flag: recorders, audio workstations, games, microphone tests, and unrelated browser tabs can resemble calls; muted/listen-only calls can lack the required signals.

Before this change, [`meeting_presence.rs`](../../kiminola/src-tauri/src/meeting_presence.rs) enumerated all active endpoints with `eAll`, but collapsed direction into one PID set. It associated known apps/browser families while requiring an unknown audio PID's own visible window. Ordinary playback could qualify, while an unknown application's windowless audio helper could be missed. The implementation now retains direction and adds bounded generic family resolution. These are source-inspection findings, not live-call measurements.

## Documented facts: practical Windows surface

| Question | Supported API / documented behavior | Consequence for this detector |
| --- | --- | --- |
| Which devices? | `IMMDeviceEnumerator::EnumAudioEndpoints(eAll, DEVICE_STATE_ACTIVE)` includes render and capture endpoints. Device “active” means present/enabled, not actively streaming. `IMMEndpoint::GetDataFlow` returns `eRender` or `eCapture`. [M1][M2] | Inspect every available endpoint, including non-default headsets; retain its direction. Alternatively enumerate each direction separately. |
| Which sessions are running? | Activate `IAudioSessionManager2` on each endpoint; use `GetSessionEnumerator` → `GetCount` → `GetSession`, then `IAudioSessionControl::GetState`. `AudioSessionStateActive` means at least one stream is running, transitioned by `IAudioClient::Start`/`Stop`; it is not a loudness test. [M3][M4] | Read metadata only. No `IAudioClient::Initialize`, `Start`, `IAudioCaptureClient`, loopback stream, PCM buffer, VAD, or ASR is needed in the detector. |
| Whose session? | `IAudioSessionControl2::GetProcessId` returns a PID. `AUDCLNT_S_NO_SINGLE_PROCESS` is a **success status** for a cross-process session and returns its initial creator PID, not every contributor. `GetSessionInstanceIdentifier` identifies a particular session instance; `IsSystemSoundsSession` identifies system sounds. [M5][M6] | Preserve attribution uncertainty, not just a successful PID value. Check whether the Rust binding preserves that success status. Ignore system sounds/unusable PIDs; do not interpret a creator PID as exclusive ownership. |
| How to follow changes? | Microsoft warns enumeration alone can miss newly notified sessions. It prescribes retaining session controls and combining initial enumeration with `RegisterSessionNotification`; a non-UI COM MTA and an initial `GetCount` are required. Session callbacks report state changes. `IMMNotificationClient` reports device/default-role changes. [M3][M7][M8] | Polling is a practical first step, not a completeness guarantee. A durable collector should reconcile snapshots with creation/state/device notifications and release controls appropriately. |
| What is the coverage boundary? | `IAudioSessionControl` explicitly supports shared-mode streams, **not exclusive-mode streams**. Sessions belong to one endpoint; apps may have multiple sessions or cross-process sessions. [M9][M10] | “All enumerated active endpoints” does not mean every app, stream, or call. Missing/failed evidence is unknown, not proof there is no meeting. |

**Direction caveat:** endpoint direction is not guaranteed stream intent. WASAPI loopback captures from a **render** endpoint, and hardware loopback/“Stereo Mix” can appear as a **capture** device. Do not describe `eCapture` as proven physical-microphone use, or every render-endpoint session as proven remote speech/playback. The generic pair remains an input/output-*endpoint* activity heuristic. [M10][M11]

### Silence and mute are not session inactivity

- **Documented:** active state tracks running streams, not nonzero samples. Session mute is separately exposed by `ISimpleAudioVolume::GetMute`. [M4][M12]
- **Inference:** a running silent or muted stream can still qualify. A meeting app's own mute button is not necessarily the Windows session mute switch; whether it keeps, stops, or recreates the capture stream requires app-specific testing. Do not label either API result “user is speaking” or “user has left.”
- **Documented:** `IAudioMeterInformation` exposes scalar peak levels without delivering PCM; endpoint meters are endpoint-wide, measure before endpoint attenuation, and software meters report zero in exclusive mode. [M13] **Recommendation:** omit amplitude from eligibility. Nonzero amplitude neither establishes a conversation nor reliably survives silence/mute, and an endpoint peak cannot attribute activity to a particular app.

### Communications metadata: what really is externally exposed?

1. **Category exists, but the setter is not a query.** An app sets its own stream's `AudioClientProperties.eCategory` through `IAudioClient2::SetClientProperties`, including `AudioCategory_Communications`. The documented `IAudioSessionControl`/`IAudioSessionControl2` enumeration surface has **no category getter**. No supported arbitrary-other-app category query was established by this research. Do not invent one or treat the app's chosen category as meeting truth. [M6][M9][M14]
2. **Device role is externally queryable, not a per-session category.** `GetDefaultAudioEndpoint(direction, eCommunications)` identifies the user's default communications device. The same speakers/mic may serve every role. Using that endpoint does not establish a meeting, and a call can select another device. [M15]
3. **Ducking notifications are a genuine optional external hint.** `IAudioSessionManager2::RegisterDuckNotification(NULL, callback)` explicitly permits observers that do not alter their own streams to receive all ducking notifications. `OnVolumeDuckNotification` supplies the triggering communications session's instance ID and active-communications-session count. The contract describes communications-stream events on the default communications device. [M16][M17] This is not a universal category snapshot or meeting API. **Proposal:** optionally match the event's instance ID to an enumerated session; never attach an unmatched global count to an arbitrary foreground app. Startup during an existing call, non-default devices, device switches, and Windows “Do nothing” ducking policy need live validation. Do not open a dummy audio stream or change ducking preferences to manufacture a signal; absence must not veto the generic fallback.

## Documented facts and inference: ownership and browsers

- **Documented:** Toolhelp `PROCESSENTRY32W` provides PID and creator/parent PID; `GetWindowThreadProcessId` identifies a window's owning process. Process identifiers are valid for the process lifetime, and `GetProcessTimes` exposes creation time with query access. These are process relationships, not semantic app ownership. [M18]
- **Documented:** Chromium's Audio Service runs in a separate process on Windows. Chromium can share renderer processes between tabs; a document/frame needs more than a process ID for identity. Its internal `MediaStreamCaptureIndicator` tracks capture by `WebContents`, including `IsCapturingAudio`. These are browser-internal objects, not Core Audio fields. [C1][C2][C3]
- **Inference:** the OS audio PID may be a utility/helper, not a window or tab. Even correct browser-family attribution cannot prove which tab supplies the mic and which supplies output. A dictation tab plus a music tab can produce the same family-level pair as a call. A visible “Google Meet” title does not prove the observed audio belongs to it. Report only the coarse generic hint when attribution is uncertain; do not claim tab-level recording isolation from this detector.
- **Coverage limit:** parent traversal cannot reliably identify every brokered/service-hosted app. Remote, virtual-device, elevated/protected-process, exclusive-mode, and non-Core-Audio paths are not certified by these APIs or this research. Do not advertise “all apps detected.” No universally reliable “in a meeting” flag was found in the reviewed Microsoft/Chromium interfaces.

## Proposed balanced fallback — not implemented or validated

1. **Retain evidence:** collect endpoint ID/direction, session-instance ID, state, PID, and attribution quality in memory. Aggregate `active_capture_endpoint` and `active_render_endpoint` independently per confidently resolved family; input/output can use different devices and different helper PIDs. Never combine mic activity from app A with output from app B.
2. **Bound family resolution:** allow a direct visible-window PID or a validated helper→application relationship. Use live ancestry plus corroborating app identity (e.g. package/install identity); shared executable name, shared signer, or a common shell ancestor alone is insufficient. Stop at application/browser boundaries; never sweep Explorer's descendants into one app. Exclude Kimi Nola and its helpers. Key long-lived ownership by PID plus creation time where available; ambiguity/access failures remain quiet rather than guessed. Known-app rules can remain separately tested refinements, not the only route to eligibility.
3. **Qualify conservatively:** visible app + active input-endpoint session + active output-endpoint session within that family, sustained across two consecutive successful polls, is a candidate for the existing “You may be in a meeting” prompt. The two-poll onset debounce is a proposed tuning value, not an API fact. Capture-only, render-only, app-only, and uncertain ownership stay quiet. Do not require nonzero amplitude or communications metadata.
4. **Preserve consent and suppression:** opt-in/pause controls, existing toast/floating-popup delivery, presentation/full-screen deferral, explicit Start recording, and at most one prompt per episode. Maintain the existing two-confirmed-inactive-poll reset; failed/partial scans must not masquerade as confirmed inactivity. A running muted session remains active. Separately validate how loss of just one direction affects episode retention, so mute/unmute does not repeatedly re-prompt. Jot notes/Not now never record; all detection prompts are suppressed during recording startup/capture/finalization.
5. **Accept the trade-off:** listen-only or mute-closes-input calls may be missed. DAWs, mic-monitoring recorders, voice-chat games, and mixed browser tabs can still qualify. Keep uncertain wording and episode suppression; measure these cases before broadening criteria. A future user-managed per-app exclusion could reduce nuisance prompts, but is outside this note's implementation scope.

The balanced fallback has been adopted in SPEC.md. This research note distinguishes API contracts from the heuristic and does not establish live app coverage.

## Minimal live acceptance matrix (pending)

Run classifier fixtures plus real Windows x64 and ARM64 checks. Record OS/app versions and coarse pass/fail results without persisting raw titles, URLs, executable names, or detector history.

| Case | Required result / accepted limitation |
| --- | --- |
| Unknown visible desktop app, active input + output; start detector before and during activity | One generic prompt after debounce, without adding the app to a name list; startup must not depend on receiving a new duck event. |
| Unknown app's input/output in separate validated helpers; unrelated helper under Explorer; unrelated same-name process | Correct family can qualify; unrelated/ambiguous processes never get merged or borrow another window. |
| Non-default USB/Bluetooth device; input/output on different endpoints; unplug/replug/default switch | Both directions retained, no default-device-only blind spot; errors remain unknown, no duplicate episode caused solely by a partial scan. |
| Silence, Windows session mute, app mute/unmute | Running sessions remain eligible regardless of amplitude; document whether each tested app stops capture on mute. No mute/unmute prompt storm. |
| Music/video only; mic recorder/dictation only; mic in app A plus playback in app B | No generic visible prompt. |
| Recorder with monitoring/loopback, DAW, voice-chat game; browser mic tab + unrelated media tab | Exercise and document residual false positives; uncertain prompt at most once, never claim confirmed meeting/tab identity. |
| Browser meeting in foreground/background tab; alternate visible tab; minimized app | Family detection does not require meeting-title matching or foreground focus. No claim that audio belongs to the visible tab. Validate which minimized windows remain eligible. |
| Listen-only call, denied mic permission, mute that closes capture, camera-only call | Quiet/missed detection is an explicit generic-fallback limitation; no fabricated second signal. |
| Cross-process session, PID exit/reuse, inaccessible process, exclusive-mode stream, device/session API failure | No confident wrong-owner attribution, crash, or false “meeting ended”; exercise cleanup/recovery and document unsupported coverage. |
| Dismiss/rejoin with app still running; one versus two inactive polls; off/pause; presentation mode; active recording | Existing suppression/rearm/deferral rules hold. Stale actions cannot record. Detector makes no audio-stream/capture/ASR calls, no outbound requests, and persists no raw evidence. |

**Open validation questions:** real capture-session visibility and helper ownership per app/version; loopback-session representation; mute behavior; reliable generic family boundaries; debounce/episode tuning; duck callback delivery and identity correlation. These are not answered by successful documentation retrieval or source inspection.

## Primary sources

- [M1] Microsoft: [EnumAudioEndpoints](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-immdeviceenumerator-enumaudioendpoints).
- [M2] Microsoft: [IMMEndpoint::GetDataFlow](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-immendpoint-getdataflow).
- [M3] Microsoft: [GetSessionEnumerator](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessionmanager2-getsessionenumerator), [IAudioSessionEnumerator lifecycle guidance](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiosessionenumerator).
- [M4] Microsoft: [AudioSessionState](https://learn.microsoft.com/en-us/windows/win32/api/audiosessiontypes/ne-audiosessiontypes-audiosessionstate).
- [M5] Microsoft: [GetProcessId](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessioncontrol2-getprocessid).
- [M6] Microsoft: [IAudioSessionControl2](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiosessioncontrol2), [GetSessionInstanceIdentifier](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessioncontrol2-getsessioninstanceidentifier).
- [M7] Microsoft: [RegisterSessionNotification and required GetCount/MTA initialization](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessionmanager2-registersessionnotification).
- [M8] Microsoft: [IMMNotificationClient](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nn-mmdeviceapi-immnotificationclient).
- [M9] Microsoft: [IAudioSessionControl and exclusive-mode limitation](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiosessioncontrol).
- [M10] Microsoft: [Audio Sessions](https://learn.microsoft.com/en-us/windows/win32/coreaudio/audio-sessions).
- [M11] Microsoft: [Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording).
- [M12] Microsoft: [ISimpleAudioVolume::GetMute](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-isimpleaudiovolume-getmute).
- [M13] Microsoft: [IAudioMeterInformation](https://learn.microsoft.com/en-us/windows/win32/api/endpointvolume/nn-endpointvolume-iaudiometerinformation).
- [M14] Microsoft: [SetClientProperties](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient2-setclientproperties), [AudioClientProperties](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/ns-audioclient-audioclientproperties-r1), [communications category guidance](https://learn.microsoft.com/en-us/windows/win32/coreaudio/communications-audio-format-capabilities).
- [M15] Microsoft: [Using a Communication Device](https://learn.microsoft.com/en-us/windows/win32/coreaudio/using-the-communication-device).
- [M16] Microsoft: [RegisterDuckNotification](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessionmanager2-registerducknotification).
- [M17] Microsoft: [OnVolumeDuckNotification](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiovolumeducknotification-onvolumeducknotification).
- [M18] Microsoft: [PROCESSENTRY32W](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/ns-tlhelp32-processentry32w), [GetWindowThreadProcessId](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid), [process lifetime/identifiers](https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers), [GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes).
- [C1] Chromium: [Audio Service README](https://chromium.googlesource.com/chromium/src/+/main/services/audio/README.md).
- [C2] Chromium: [Multi-process Architecture](https://www.chromium.org/developers/design-documents/multi-process-architecture/).
- [C3] Chromium source: [MediaStreamCaptureIndicator](https://chromium.googlesource.com/chromium/src/+/main/chrome/browser/media/webrtc/media_stream_capture_indicator.h). Chromium `main` links are moving references; findings describe the source retrieved for this note, not every Chromium-derived browser release.
