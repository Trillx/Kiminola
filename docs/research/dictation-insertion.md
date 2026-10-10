# Safe Windows dictation delivery across apps

- Researched on 2026-09-22, Central time.
- Scope is documentary research for [Investigate safe Windows dictation delivery across apps](https://github.com/Trillx/Kiminola/issues/46), under [Plan system-wide dictation for Kimi Nola](https://github.com/Trillx/Kiminola/issues/43). Both issues and their comments were read through `gh`, with paginated comment reads to check completeness.
- Evidence consists of Microsoft documentation, upstream Tauri source, first-party application documentation, a W3C specification, and repository source at `e4a448d99da8d372088e220e9c7465c5341902b5`.
- No application compatibility measurements were made. No live window text, clipboard content, microphone, credentials, or user documents were read. No input was injected, applications installed, security settings changed, or issues modified.
- Every proposed policy below is a recommendation for the delivery decision, not adopted product behavior. This note does not change `SPEC.md` or certify any application.

## Findings that affect the decision

1. **There is no documented universal, atomic "insert these words into this unchanged selection" operation among the generic mechanisms reviewed.** `SendInput` queues input for the foreground application, not an identified editor transaction. Clipboard publication and paste are separate operations. UI Automation can address a control, but `ValuePattern.SetValue` sets its value rather than inserting at an arbitrary selection. TSF offers insertion within a text context and edit session, but requires a text-service integration, not a generic external HWND call. [M1][M2][M3][M13][M19]
2. Recommend prototyping **UI Automation plus Win32 metadata for eligibility and target revalidation**, with **Unicode `SendInput` as a clipboard-free candidate** for tested ordinary text controls. Consider clipboard/paste only as an explicitly accepted compatibility alternative. Do not silently chain delivery methods after an ambiguous result. These are candidates for experiments, not a chosen production mechanism.
3. **Input accepted by Windows is not proof that the intended text reached the intended editor.** Report an attempted, uncertain result honestly. A generic automatic retry can duplicate text, and a generic rollback can delete the user's newer work. Strict exactly-once delivery needs a cooperating target with an acknowledgment and operation identity, beyond the contracts of these generic APIs. [M1][M2][M3][M13]
4. **No synthesized Enter key is necessary for terminal execution.** Microsoft explicitly warns that multiline paste can execute commands automatically. A terminal, coding CLI, chat box, and source editor in the same VS Code window are distinct delivery targets. Unknown terminal-like targets should retain the result for review rather than receive automatic delivery until a separate policy and tests exist. [A1][A2][A4]
5. **The installed shortcut dependency's release event is a polling heuristic.** `global-hotkey` 0.8.0 polls the main key with `GetAsyncKeyState`, sleeping 50 ms between polls. It does not observe release of every modifier or attach a per-hold generation to the event. The API exposes `Pressed` and `Released`, but that does not establish reliable hold-to-talk lifecycle behavior. [T1][T2][M22]
6. Clipboard history and sync suppression have documented Windows controls, but **restoring the previous clipboard is neither guaranteed nor a privacy eraser**. New clipboard writes, delayed paste, non-text formats, owner-managed data, and process crashes need an explicit contract. [M13][M14][M15][M16]

## Scope and repository baseline

The new planning scope is English-first system-wide dictation on native Windows x64 and ARM64, with both hold-to-talk and start/stop toggle. Audio stays local. Plain local transcription remains usable without cleanup. Optional cleanup may use a local model or a configured text-only cloud provider. Voice-command automation and automatic submission are out of scope. The existing meeting-oriented `SPEC.md` and glossary predate this scope. In particular, the Background companion currently never captures audio, and managed fully offline local-LLM enhancement is outside the meeting MVP. This research does not resolve those product boundaries. [P1]

Source inspection found:

- [`shortcuts.rs`](../../kiminola/src-tauri/src/shortcuts.rs) owns shortcut persistence and registration. The actual handler in [`lib.rs`](../../kiminola/src-tauri/src/lib.rs), around lines 119-135, rejects everything except `ShortcutState::Pressed` and emits `shortcut:triggered`.
- [`+layout.svelte`](../../kiminola/src/routes/+layout.svelte), around lines 72-85, routes that event to the meeting recording view when not already recording. This is not a cross-app dictation lifecycle.
- [`meeting-export.ts`](../../kiminola/src/lib/meeting-export.ts) calls `navigator.clipboard.writeText` for user-invoked export. It does not implement target capture, paste, history exclusion, clipboard restoration, or insertion acknowledgment.
- [`Cargo.lock`](../../kiminola/src-tauri/Cargo.lock) pins `tauri` 2.11.5, `tauri-plugin-global-shortcut` 2.3.2, `global-hotkey` 0.8.0, and `tao` 0.35.3. The upstream source citations below match those versions.

No cross-app insertion implementation was identified in the reviewed shortcut/export paths or the repository `SendInput` search. Existing meeting-window ownership logic is not proof of editor or browser-tab identity.

## Mechanism comparison

| Mechanism | Documented capability | Limit relevant to dictation | Recommended role to investigate |
| --- | --- | --- | --- |
| UI Automation focus/properties and Text patterns | `GetFocusedElement` retrieves the focused accessible element. Providers expose supported control patterns, including ranges and selection. Pattern availability can vary dynamically. [M4][M5][M6] | A provider may omit information or events. An accessible text region need not be writable. A focused element can disappear before the query returns. Ranges are not durable immutable document revisions. [M4][M6][M7] | Inspect minimal metadata and reject unsafe or uncertain targets. Do not scrape document text by default. |
| UI Automation `ValuePattern.SetValue` | Sets the element's value. It requires enabled and not-read-only state; success returns `S_OK`. [M3] | This is value replacement, not an insert-at-caret operation. Reading the whole value, splicing text, and setting it back risks overwriting concurrent edits and losing formatting. It has no expected-revision or compare-and-swap parameter. | Avoid as the generic fallback. A separately tested adapter could use it for an intentionally replaced simple field, under an explicit replacement contract. |
| Unicode `SendInput` | `KEYEVENTF_UNICODE`, with `wVk = 0`, carries a Unicode character through `VK_PACKET` to the foreground thread and normally through `TranslateMessage` to `WM_CHAR`. Microsoft explicitly includes voice recognition as a use case. [M1][M2] | No target HWND parameter and no editor acknowledgment. UIPI, held modifiers, custom input processing, line breaks, and focus changes remain concerns. The returned count is input events, not characters inserted. | Clipboard-free candidate for eligible, tested text controls. Restrict content and target category according to experiments. |
| Clipboard plus paste shortcut | Publishes Unicode text in `CF_UNICODETEXT`; the receiving application performs paste. Windows supplies history/sync exclusion formats. [M13][M14][M16] | Shared mutable global resource. Publishing does not paste, and sending Ctrl+V does not prove consumption. The shortcut still has input/focus/UIPI risks. Apps can interpret paste differently. [M1][A5] | Compatibility candidate only after accepting clipboard privacy and restoration limits. Prefer a visible manual copy action for uncertain targets. |
| Targeted `WM_PASTE` | Microsoft documents a message to an edit control or combo box, inserting clipboard content at the caret. Its documented return has no value. [M17] | This is not a universal message for browser DOM editors, Teams, or VS Code. The basic message page describes `CF_TEXT`; it is not evidence of rich-editor coverage or transaction acknowledgment. | At most an adapter for a specifically tested native control, not an elevation or compatibility escape hatch. |
| TSF text service | `ITfInsertAtSelection::InsertTextAtSelection` inserts at the selection/insertion point, with a read/write edit cookie and context lock. It can return an inserted range. [M19] | Requires participation in TSF context/session/registration lifecycle. Locks can fail, contexts can disconnect, and edit-session requests can be asynchronous. `S_OK` from `RequestEditSession` alone is not proof the later edit succeeded. [M19][M20] | Longer-term spike if transactional context-aware input justifies a registered text service and packaging work. Not the first generic fallback. |

### UI Automation details

`TextPattern` exposes read access and selection/range operations, not a general text insertion method. `Select` can move the caret, which is itself a mutation of user state. It is not safe to reselect an old range merely to make late dictation fit. Microsoft's current Win32 Value-pattern guidelines discuss writable multiline controls; older .NET examples make narrower assumptions. Do not infer that Text and Value support are mutually exclusive or that all multiline controls lack Value support. Probe actual capabilities. Neither guideline turns `SetValue` into selection-preserving insertion. [M3][M5][M6]

Microsoft warns that providers need not raise every possible event. A text range can become invalid after replacement, and provider-managed anchors can move with edits. Comparing a retained range to current selection can detect some changes, but cannot establish that no edits occurred or that the same document revision remains. No-event and same-endpoints are not proofs of an unchanged target. [M6][M7]

Use a separate COM MTA worker for future UIA work, rather than Tauri's UI thread. Microsoft documents UI-thread hangs and apartment-lifetime issues in cross-bitness UIA use. Bound waiting and treat timeouts as uncertainty. A local timeout does not cancel a remote setter or prove it had no effect. Do not start a second insertion because the first UIA call was slow. [M8]

### Unicode input details

- With `KEYEVENTF_UNICODE`, `wVk` must be zero. `wScan` is a `WORD`; construct input from UTF-16 code units, with corresponding key-up events, rather than truncating Rust Unicode scalar values. Supplementary characters need surrogate-pair tests. Test combining marks, punctuation, and emoji even with English-first ASR. [M2]
- Windows serializes the events submitted in one `SendInput` call without interspersing other keyboard/mouse input events. That guarantee does not bind the batch to a particular control or unchanged selection, and does not make several calls one atomic operation. It also does not prevent programmatic activation or application-side transformations. [M1][M2]
- Existing key state is not reset. A released hotkey main key does not prove Ctrl, Alt, Shift, or Win are up. Prefer deferral until modifiers are released and revalidate again, rather than forcibly releasing keys the user is holding. The modifier check is another snapshot, not a lock. [M1][M22][T2]
- Do not synthesize navigation, select-all, deletion, Enter, Tab, submit shortcuts, or clicks as a generic repair. Treat CR, LF, tabs, NUL, ESC, and other control characters as a separate content policy. Sending CR/LF as Unicode is not evidence that the receiver will treat it as inert prose. Clipboard `CF_UNICODETEXT` specifies CR-LF line endings and NUL termination, so reject or explicitly handle embedded NUL rather than silently truncating. [M2][M16][A1]
- A full returned event count means queued input. Zero or a short count does not give a safe universal retry rule. Record the attempt and result category; do not try another transport after uncertain or partial delivery. [M1]

## Target identity and revalidation

### What Windows establishes

| Observation | What it establishes | What it does not establish |
| --- | --- | --- |
| `GetForegroundWindow` | The current foreground HWND, possibly null during activation changes. [M9] | Which editor, tab, document, or selection is intended. |
| `GetWindowThreadProcessId` and process creation time | HWND-owning thread/PID and, where query access permits, a process-instance discriminator. [M10] | Semantic ownership of a browser frame or editor buffer. HWND and PID alone are not lasting identities. |
| `GetGUIThreadInfo` | Cross-thread active, focus, and caret window information. Handles may be invalid while activation changes; caret coordinates have control/DPI caveats. [M11] | An immutable text position. A coordinate is not a document revision or a reliable substitute for text selection. |
| UIA focused element, runtime ID, patterns and properties | The currently exposed accessible control and its reported capabilities. Runtime IDs are opaque comparison values, unique on the desktop at the time, and reusable. [M4][M12] | A durable token across app restarts, UI reconstruction, browser navigation, or provider failures. |
| UIA focus/text/selection events | Notifications that can invalidate an operation. [M7] | Exhaustive observation of every change. Missing events cannot authorize insertion. |

`IsWindow` is not a safe identity check for another application's window. Microsoft explicitly warns that the handle can be destroyed after the call or recycled for another window. `SetForegroundWindow` can be denied even when its documented conditions hold. Bringing a window back would not restore its original caret or user intent. Do not force activation, attach input queues, or emulate mouse clicks to rescue stale delivery. [M9][M12]

### Proposed conservative transaction model

This is a design recommendation to test, not an API guarantee or an adopted state machine.

1. On the initiating gesture, create a new dictation generation and capture minimal target metadata before showing any UI. Include foreground HWND, thread/PID, process creation time when available, focused control identity, relevant UIA state, and selection evidence when supported. Keep identities in memory. Do not persist titles, URLs, control names/values, or selected text.
2. Keep capture, ASR finalization, optional cleanup, and delivery as distinct states. Attach every ASR/provider result to the generation and chosen text revision. Do not stream provisional corrections into the other application.
3. Invalidate automatic delivery on observed focus, selection, document, or user-edit changes, target closure, lock/desktop transition, cancellation, or a newer conflicting generation. An original editor becoming focused again must not silently rearm an invalidated operation. The user may explicitly select a new destination later.
4. Immediately before an attempt, re-read current metadata and capabilities. Reject password, disabled, read-only, inaccessible, terminal-like, own-app, and otherwise disallowed targets. Check modifier state. Treat unavailable information as unknown, not as permission.
5. Spend a one-use delivery authorization before invoking the mutating API. Serialize delivery attempts. If clipboard work or another wait occurs after validation, revalidate again. There is still a check-to-use race; disclose it rather than calling this atomic target locking.
6. Classify the result as `not attempted`, `attempted, unconfirmed`, `confirmed by a specific adapter/test oracle`, or `partial/unknown`. Never infer `not inserted` from a timeout or an absent accessibility event. A retry after an attempt needs user review of possible duplicates, not merely another click labeled "Retry".
7. Retain a recoverable local result according to a separately chosen retention policy. Offer review and an explicit copy/manual-delivery action. Do not overwrite the clipboard merely because automatic insertion was skipped. Do not automatically undo or backspace external text.

There is a real product trade-off here. Generic HWND/UIA checks cannot perfectly detect user typing followed by deletion, selection movement away and back, or an unreported document change. More inspection also increases privacy cost. If the product requires zero wrong-target risk rather than a tested best-effort bound, generic injection does not satisfy that requirement. Require a fresh user-mediated transfer or a cooperating editor integration with a revision check and acknowledgment.

### Late cleanup and user edits

Recommend choosing one final text variant per generation. If plain transcription is delivered after cleanup times out, a later provider response must not replace, append to, or redeliver it. A cancel, new recording, closed target, changed provider configuration, or explicit fallback must invalidate older delivery authority. Cancellation of a request is not proof that its response cannot arrive. Recheck generation and revision at delivery, not only when starting the provider request.

Cloud cleanup authorization should cover transcript text only. Target names, window titles, document contents, selected text, clipboard snapshots, and focus metadata should remain outside the provider request. Local cleanup needs the same stale-result guards because its latency can cause the same race. Preserve plain transcription independently so provider failure never requires an empty or fabricated result. These are recommendations consistent with the new planning scope, not existing implementation guarantees. [P1]

## Security, architecture, and supported boundaries

- **UIPI and elevation.** `SendInput` may inject only into equal- or lower-integrity applications. Neither its return value nor `GetLastError` identifies UIPI as the cause of failure. UIA access to higher-integrity UI also has security constraints. Clipboard publication does not grant permission to drive an elevated editor. Recommend a normal non-elevated process and a visible unsupported/uncertain outcome, not automatic elevation. [M1][M18]
- **UIAccess is not a flag to enable casually.** Microsoft's assistive-technology guidance requires signing, a protected installation location, a manifest, and an appropriate accessibility scenario, with further privilege constraints. It does not provide blanket access to system-integrity UI. Supporting elevated applications would need a separate security and packaging decision. Do not disable UIPI or change UAC policy to increase coverage. [M18]
- **Passwords and secure UI.** UIA `IsPassword` reports whether an element contains a disguised password. It is useful to deny delivery, but a false or unavailable value is not proof that a field is nonsensitive. Custom credential, payment, recovery, and one-time-code fields may need additional restrictions or explicit exclusion. Treat lock/logon screens, UAC secure desktop, and inaccessible UI as unsupported. Never probe contents or try an alternative injection method to bypass the boundary. [M18][M21]
- **Native x64 and ARM64.** These are ordinary Win32/COM API candidates, not an x64-only injection design. That is architectural applicability, not measured portability. Build native `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` clients with generated ABI-correct bindings and `size_of::<INPUT>()`; do not hardcode structure sizes or truncate handles. Windows 11 on Arm can run native ARM64 and emulated x64 apps, so test both target types from the native ARM64 client. [M1][M23]
- **Cross-process versus in-process integration.** UIA documents cross-bitness helper/apartment behavior. Do not extrapolate that into a certification of every ARM64/native/emulated provider. TSF services use COM in-process registration. Microsoft documents different DLL-loading compatibility for ARM64 and x64/ARM64EC processes, so a TSF design would need a deliberate architecture-specific service/package plan. One native ARM64 executable does not imply that its DLL can load into an x64 target. [M8][M20][M23]
- Remote desktop, Citrix, virtual machines, inaccessible enterprise-managed targets, and other sessions are not covered by this note. Do not interpret system-wide as all desktops, sessions, privilege levels, or application versions.

## Clipboard privacy and restoration

### Documented facts

Windows can retain clipboard history and synchronize clipboard items to the user's devices. Microsoft documents these registered formats. [M14]

- `ExcludeClipboardContentFromMonitorProcessing`, with any data, excludes all formats of the item from Windows clipboard history and device synchronization.
- `CanIncludeInClipboardHistory`, with a serialized `DWORD` zero, disables Windows history inclusion for the item, independently of synchronization.
- `CanUploadToCloudClipboard`, with a serialized `DWORD` zero, disables synchronization, independently of history.

The WinRT alternative is `Clipboard.SetContentWithOptions`, explicitly setting `IsAllowedInHistory = false` and `IsRoamable = false`. It can return false when the clipboard is in use. These options were introduced in Windows 10 version 1809. Do not rely on default options or assume `navigator.clipboard.writeText` sets them. [M15]

These documented Windows mechanisms are not access control against other clipboard readers and do not promise that third-party clipboard managers, security software, or a recipient application will not retain data. A target email or Teams application may store or sync its own draft independently. Audio staying local does not make transient transcript text on the clipboard private. [M13][M14]

### Why a save, paste, restore sequence is not a transaction

`OpenClipboard` prevents other applications from modifying the clipboard while it is open and fails if another window already has it open. A writer should use a valid owner HWND. The clipboard must be available to the recipient for paste. `SetClipboardData` transfers ownership of supplied memory to Windows and supports delayed rendering. Clipboard contents may include multiple formats, private data, graphics, files, and owner-dependent representations. Saving only plain text cannot restore that state faithfully. [M13][M14]

`GetClipboardSequenceNumber` changes when contents change or are emptied, with special timing for delayed rendering; it may return zero when access is unavailable. It reports content changes, not reads, paste completion, or destination identity. [M16]

A recommendation for any future clipboard experiment is:

1. Establish a restoration capability policy before replacing anything. If existing formats cannot be faithfully preserved under that policy, skip automatic clipboard delivery. Do not silently downgrade rich text, images, file copies, or owner-managed data to plain text.
2. Serialize Kimi Nola's clipboard deliveries. Take any permitted snapshot only in memory and only for this transaction. A snapshot itself reads potentially sensitive user data, so its production authorization is a separate decision. The experiments below use dummy clipboard fixtures only.
3. Acquire the clipboard, publish the text and Windows exclusion metadata before closing it, and record the resulting sequence and an application transaction marker. Treat failures to apply privacy metadata as a failed publication, not permission to paste unprotected text.
4. Release the clipboard and revalidate the target before issuing any paste request. A process can still change the clipboard after release but before the target reads it. No clipboard lock spans that exchange safely for a generic target.
5. Restore only after reacquiring the clipboard and comparing the current sequence/marker inside that protected interval. Never overwrite a newer clipboard item. Checking outside the lock and restoring later creates another race. A sequence/marker match still does not prove the target consumed the text.
6. Do not promise that a fixed sleep is enough. Restore too early and a slow recipient may paste old contents; restore too late and the transcript remains exposed or a user paste may use it elsewhere. If consumption is unconfirmed, restoration policy must admit that trade-off. A crash can leave the transcript on the clipboard.

Even successful restoration does not remove copies already read or retained elsewhere. Clearing the user's clipboard history to hide that risk would be a destructive unrelated action, not a restoration mechanism. Recommend keeping clipboard delivery optional unless these limitations are acceptable.

## Hold, toggle, and a nonactivating indicator

### Shortcut evidence

Tauri's plugin documents both `ShortcutState::Pressed` and `ShortcutState::Released`. Version 2.3.2 forwards events from `global-hotkey`. In the pinned Windows backend, registration uses `MOD_NOREPEAT`; `WM_HOTKEY` emits `Pressed`, then a spawned thread polls `GetAsyncKeyState` for the main virtual key until the returned value is exactly zero, with a 50 ms sleep between polls. `Released` carries shortcut ID and state, not the recording generation. [T1][T2]

Microsoft documents the high bit of `GetAsyncKeyState` as current down state, the low bit as unreliable recent-press state, and zero for several failures, including an inactive desktop or access constraints. Consequently, the upstream `state == 0` test is not a guaranteed physical-release detector. It also ignores modifier release. The 50 ms interval is source code, not a measured or guaranteed maximum latency. [M22][T2]

Recommendations for a hold/toggle prototype:

- Hold mode should have explicit pressed, capturing, stopping, and canceled generations. Test releasing modifiers before the main key and vice versa, rapid taps shorter than a polling interval, two fast holds, repeated presses, and delayed release events. A release from an older polling thread must not stop a new capture.
- Toggle mode should act once on an accepted press and ignore release for toggling. `MOD_NOREPEAT` limits OS repeat notifications but is not the entire duplicate-event defense.
- On suspend, lock, shortcut replacement, app shutdown, registration failure, or uncertain release, stop/cancel safely without delivering stale text. Supply an explicit stop/cancel path and decide a bounded maximum hold duration. Do not hide a stuck capture behind presumed release reliability.
- Keep lifecycle authority in the backend, independent of a particular webview route being mounted. Do not reuse the meeting route event unchanged.
- Registration conflicts and OS-reserved combinations need visible failure. Microsoft's `RegisterHotKey` documents these restrictions. The chosen default shortcut and whether releasing any key or only the main key ends hold remain human decisions. [M24]

### Indicator evidence

`WebviewWindowBuilder` in Tauri 2.11.5 distinguishes `.focused(false)`, which affects initial focus, from `.focusable(false)`. Tao 0.35.3 maps nonfocusable windows to `WS_EX_NOACTIVATE`; its code also distinguishes nonactivating initial show from other show paths. Microsoft's style documentation says a top-level `WS_EX_NOACTIVATE` window does not become foreground when clicked or when the current foreground window closes/minimizes. It can still be activated explicitly by APIs. [T3][M25]

Recommend a separate passive indicator with both initial-focus and focusability settings explicit. Never call `set_focus` to show it. Always-on-top and click-through are separate concerns; neither means nonactivation. Validate create, show, hide, reshow, resize, monitor/DPI changes, and mouse interaction on both architectures. Source mapping does not prove that every Tauri/WebView2 child-window or click path preserves focus.

A nonfocusable indicator cannot also be the only keyboard-accessible place to review/correct/cancel. Provide shortcut-based stop/cancel and an explicitly opened review window. Opening review deliberately changes focus and should invalidate any pending automatic delivery rather than then force focus back. The accessibility trade-off needs an interaction decision.

## App-category compatibility hypotheses

No row below is a pass. Each is a distinct future test target with version-specific results.

| Target category | What the sources support | Questions a disposable fixture must answer |
| --- | --- | --- |
| Native single-line and multiline edit controls | UIA and Win32 input have documented control-level mechanisms. [M1][M3][M5][M17] | Append versus selection replacement; Unicode; read-only/password rejection; undo grouping; max length; multiline handling; focus loss during finalization. |
| Outlook/new Outlook/classic Outlook and browser email | Outlook documents Ctrl+V at the current location and Ctrl+Enter to send; classic/new variants can differ. [A3] | Subject versus body versus recipient field; rich-text boundaries and signature preservation; pop-out compose; nonempty selection; no send action. Start with local draft fixtures, no recipients or real accounts. |
| Teams-style desktop and web editors | Teams documents Shift+Enter for a new line and Ctrl+Enter to send in expanded compose. [A6] | Compact versus expanded composition; line breaks; lists and pasted markdown; mentions and code blocks; reply-thread changes; no message transmission. Do not assume the same control or semantics in every compose mode. |
| Browser `input`, `textarea`, and `contenteditable` | The W3C clipboard specification makes paste cancelable; a synthetic DOM paste event does not itself change the document. [A5] | Plain versus rich editable hosts; canceled/transformed paste; rerendered nodes; frames/shadow DOM; tab switch/navigation with unchanged HWND; JS-controlled fields; max length and form-submit handlers. Test Edge/Chrome and any other promised browser separately. |
| VS Code source editor | Being inside the VS Code process does not identify the focused editor. This is an inference from window/control identity limits. [M9][M11][M12] | Caret versus selection, multiple cursors, read-only files, snippets, format-on-paste, autoindent, IME, undo grouping, and accidental insertion into search/command palette. Use an untitled disposable file. |
| VS Code chat and inline chat | VS Code documents Enter to submit a chat request and that the agent may change code. [A4] | Editor-to-chat focus change inside one window; multiline prompt insertion; no submit or model request; context chips and slash-command menus; separately test extensions rather than inheriting certification. |
| VS Code integrated terminal, Windows Terminal, coding CLI/TUI prompts | Terminal paste can execute newline-delimited commands. VS Code's warning depends on settings and bracketed-paste mode. [A1][A2] | Shell versus coding prompt versus raw TUI; nested SSH/WSL; multiline/trailing newline; bracketed paste active/inactive; warnings disabled; control characters. Do not allow a shell or agent to execute the payload during the initial probe. |

For browser-backed apps, a top-level executable name or HWND allowlist is insufficient. A tab can navigate, a compose box can be replaced, and a terminal can be selected while the top-level window remains unchanged. This is a target-identity limitation, not a claim that any named app necessarily exposes inadequate UIA data.

### Text insertion is not submission, but text can still execute

No voice-command or auto-submit intent means delivery must not invoke buttons, press Enter, or synthesize send shortcuts. It does not make arbitrary text harmless. Microsoft's Windows Terminal documentation explicitly says a newline can execute one or more commands upon paste, before the user validates them. VS Code only shows its default multiline warning when the shell does not advertise bracketed paste, and the warning can be disabled. Bracketed paste is a cooperation protocol between terminal and running program, not a universal safety guarantee for every shell, TUI, nested session, or injected Unicode stream. [A1][A2]

Recommend treating known and suspected terminal targets as manual-review-only initially. Even a single line can become dangerous if focus switches to a program interpreting each key. If terminal dictation is a launch requirement, require a separate tested terminal/prompt adapter and an explicit newline/control-character policy. Replacing newlines with spaces changes meaning and is not an invisible safe fallback. Sending bracketed-paste escape sequences blindly is not a substitute for determining receiver state.

## Proposed safe compatibility experiments

These experiments are not authorized or run by this research. They require a separate task and an isolated user-approved environment. No microphone, ASR model, or paid provider is needed to test delivery.

### Smallest useful probe

1. Use a throwaway Windows account or VM on each required architecture. Build a native fixture with two disposable editor windows, enabled/read-only/password controls, and a test-owned event/result recorder. Only those fixture HWNDs and processes may receive input. Start with no real app accounts or clipboard data.
2. Use fixed synthetic transcripts and an in-process fake provider that can delay, reorder, duplicate, fail, or return after cancellation. The fixture can record its own selection and exact text before/after. This establishes an oracle without reading unrelated windows.
3. Compare Unicode input, clipboard/paste, and UIA capabilities separately. Do not run an automatic fallback chain. Exercise `SetValue` only to demonstrate replacement semantics in a dummy field, not as an assumed insertion adapter.
4. Add a local HTML fixture for `input`, `textarea`, and `contenteditable`, including handlers that cancel/transform paste and rerender the focused node. Instrument only that page. Browser fixtures test mechanism behavior, not Teams or Outlook compatibility.
5. Add a passive Tauri indicator and hold/toggle event recorder with no microphone capture. Confirm foreground/focus/selection preservation from fixture metadata throughout indicator lifecycle. Record shortcut event timing, not only screenshots.
6. Test terminal bytes first with a test-owned terminal/PTY input recorder that never evaluates commands and has no credentials or agent tools. Explicitly stop before using a real interpreter. If later approved, use a disposable restricted shell and harmless sentinel output to measure execution semantics; never use a destructive payload or a real coding agent.

### Experiment matrix and expected safety observations

| Experiment | Evidence to record and candidate pass condition |
| --- | --- |
| Ordinary insertion | Exact expected fixture text, preserved prefix/suffix, intended selection replacement, no duplicate, no unexpected input event or submit. Separate mechanism acceptance from rendered final text. |
| Unicode and size | ASCII, curly punctuation, accented names, combining marks, supplementary characters, short/long text, empty text, and explicit LF/CRLF cases. No split surrogate, NUL truncation, unexpected normalization, or silent dropped suffix. |
| Selection and user typing | Move caret, replace selection, type/delete, select all, and move away/back while the fake provider waits. Conservative candidate refuses stale automatic delivery; document undetectable same-state cases instead of declaring them safe. |
| Focus and identity | Alt-tab to the other fixture, change fields inside one HWND, switch tabs, rebuild a control, close/reopen target, and simulate reused identifiers in unit tests. No delivery to the new target; no forced reactivation. |
| Late and competing results | Cancel, start a newer generation, timeout then choose plain text, return cleanup afterward, deliver duplicate callbacks, and delay events until after stop. At most one authorized attempt per generation; no late replacement. |
| Unconfirmed delivery | Fixture ignores `VK_PACKET`, cancels paste, delays consumption, disconnects UIA, or times out. State remains unconfirmed/unknown where appropriate; no blind retry or automatic undo. |
| Clipboard races | Dummy plain/rich/image/file/custom/delayed-render formats; fixture process holds clipboard; third fixture writes during paste/restoration; crash after publication. Preserve newer clipboard content; report unsupported restoration; distinguish text restored from all formats restored. |
| History/privacy | Use only synthetic text in a preconfigured test environment. Verify Windows history exclusion and inspect a test clipboard monitor's observations. Do not connect a personal sync account or change the user's history/security settings. Cloud-sync behavior remains unverified without a separately approved isolated sync test. |
| Privilege/secure boundaries | Preprovision normal and elevated dummy editors plus a dummy password field. Expect normal client denial or no attempt where required; no bypass fallback. Use mocked secure-desktop transitions first. Any later real lock/UAC test is manual and must preserve existing security policy. |
| Shortcut lifecycle | Main-key-first/modifier-first release, fast taps, rapid repeated holds, long hold, held modifiers at delivery, lock/suspend, unregister/rebind, main webview hidden/closed. No stuck recording state, late release stopping a newer generation, or shortcut repeat causing duplicate attempts. |
| Indicator lifecycle | Create/show/hide/reshow, click, move, DPI/monitor transition, foreground target closes. Passive indicator does not activate or consume editor selection. Review action deliberately changes focus and invalidates automatic delivery. |
| App-category probes | Only after fixture gates, use throwaway untitled files/local pages and separately approved test-only accounts if a real Teams/email composer cannot exist offline. Never send a message, submit a prompt, save real files, or certify an untested version. |

Run native x64 client to x64 fixtures, native ARM64 client to ARM64 fixtures, and native ARM64 client to emulated x64 fixtures. If x86 application compatibility is promised, add x86 targets separately; it is not established by x64/ARM64 client support. Record Windows build, hardware/process architecture, app/browser/WebView2 version, mechanism, relevant editor mode, fixture ID, synthetic payload ID, event counts, API result, independent final-state result, and uncertainty. Do not retain real document text or clipboard snapshots.

Do not certify an app based only on API return success, a green native build, or one plain-text test. Coverage should be a versioned matrix of target control/mode, mechanism, architecture, and the negative tests above. The allowed residual risk, latency bounds, and fallback wording remain product acceptance decisions.

## Follow-up questions for the delivery decision

1. Is "never insert into the wrong place" a strict requirement? If yes, is manual transfer or a cooperating-editor integration acceptable where generic APIs cannot bind to a document revision?
2. Is the intended destination the selection at recording start, at stop, or at final delivery? Should any intervening edit/focus change permanently revoke automatic delivery until the user explicitly rearms it?
3. Is replacing selected text intended? What should happen when selection cannot be observed, there are multiple cursors, or the field is not empty? Whole-field replacement should not be inferred from dictation intent.
4. Which exact launch app versions and modes are required? In particular, are coding terminal/TUI prompts required alongside VS Code editor/chat, or can terminal delivery remain manual initially?
5. Are terminal newlines, tabs, and other control characters blocked, previewed, or supported through a tested adapter? Is changed whitespace an acceptable explicit transformation, or must original text remain intact?
6. Is clipboard fallback opt-in? May Kimi Nola read a prior clipboard snapshot for restoration, and which formats must it preserve? What should it show when another app copies during delivery or paste completion is unknown?
7. After an unconfirmed attempt, may the UI offer a fresh insertion only with an explicit duplicate warning? Is retaining raw and cleaned results locally acceptable, for how long, and with what user deletion controls?
8. If cleanup is slow, does the user choose plain text, or does it become the single committed variant automatically? Either choice must prohibit late replacement and must not send target context or clipboard data to the provider.
9. Does hold end on main-key release or release of any part of the chord? What cancellation/max-duration rule applies when upstream polling misses or misclassifies release? Is a different Windows key-event implementation permitted if the probe fails?
10. Will elevated apps, password/credential/payment fields, secure desktop, remote sessions, and unknown editors be explicitly unsupported? Supporting UIAccess or TSF would be separate security/packaging work, not a small fallback switch.
11. What accessible review/cancel UI complements a nonactivating indicator without automatically restoring focus? How does dictation coexist with a Meeting capture and the existing Background companion boundary?
12. Is UIA permitted to read any surrounding/selected target text to improve validation, or metadata only? If metadata only, which undetectable document-change risks require manual delivery rather than a weaker automatic claim?

## Primary sources

Sources were retrieved for this note. Version-tagged upstream links describe the pinned implementation, not a promise about future releases. Microsoft, application, and W3C pages are moving documentation. The W3C document describes web behavior, not proof that a particular browser version conforms.

- [P1] [Plan system-wide dictation for Kimi Nola](https://github.com/Trillx/Kiminola/issues/43), [Investigate safe Windows dictation delivery across apps, research context](https://github.com/Trillx/Kiminola/issues/46#issuecomment-5788107918). Repository baseline files are linked above.
- [M1] Microsoft, [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput). Event-count return, UIPI, input serialization, held-key state, structure size.
- [M2] Microsoft, [KEYBDINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput). Unicode, foreground queue, `VK_PACKET`, `WM_CHAR`, and key-up flags.
- [M3] Microsoft, [IUIAutomationValuePattern::SetValue](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationvaluepattern-setvalue), [Value control pattern](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-implementingvalue).
- [M4] Microsoft, [IUIAutomation::GetFocusedElement](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation-getfocusedelement). Focused element can already be removed when the call returns.
- [M5] Microsoft, [UI Automation control patterns overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controlpatternsoverview).
- [M6] Microsoft, [TextPattern overview](https://learn.microsoft.com/en-us/dotnet/framework/ui-automation/ui-automation-textpattern-overview), [using IUIAutomationTextRange](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-usingtextrangeobjects). Read-only text model, range validity, selection, comparisons.
- [M7] Microsoft, [subscribing to UI Automation events](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients). Not all providers raise all events; late event delivery after unsubscribe is possible.
- [M8] Microsoft, [UI Automation threading issues](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading). Separate MTA thread and cross-bitness apartment lifetime.
- [M9] Microsoft, [GetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getforegroundwindow), [SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow).
- [M10] Microsoft, [GetWindowThreadProcessId](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid), [GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes).
- [M11] Microsoft, [GetGUIThreadInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getguithreadinfo), [GUITHREADINFO](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-guithreadinfo).
- [M12] Microsoft, [GetRuntimeId](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationelement-getruntimeid), [IsWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindow). Reusable identities and lifetime races.
- [M13] Microsoft, [OpenClipboard](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard), [SetClipboardData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setclipboarddata), [using the clipboard](https://learn.microsoft.com/en-us/windows/win32/dataxchg/using-the-clipboard).
- [M14] Microsoft, [clipboard formats, including cloud clipboard and history exclusions](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats).
- [M15] Microsoft, [ClipboardContentOptions](https://learn.microsoft.com/en-us/uwp/api/windows.applicationmodel.datatransfer.clipboardcontentoptions), [SetContentWithOptions](https://learn.microsoft.com/en-us/uwp/api/windows.applicationmodel.datatransfer.clipboard.setcontentwithoptions).
- [M16] Microsoft, [GetClipboardSequenceNumber](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber), [standard clipboard formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/standard-clipboard-formats).
- [M17] Microsoft, [WM_PASTE](https://learn.microsoft.com/en-us/windows/win32/dataxchg/wm-paste).
- [M18] Microsoft, [security considerations for assistive technologies](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview). UIAccess requirements and protected-UI limitations.
- [M19] Microsoft, [Text Services Framework](https://learn.microsoft.com/en-us/windows/win32/tsf/text-services-framework), [ITfInsertAtSelection::InsertTextAtSelection](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinsertatselection-inserttextatselection).
- [M20] Microsoft, [ITfContext::RequestEditSession](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontext-requesteditsession), [text service registration](https://learn.microsoft.com/en-us/windows/win32/tsf/text-service-registration).
- [M21] Microsoft, [CurrentIsPassword](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationelement-get_currentispassword).
- [M22] Microsoft, [GetAsyncKeyState](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getasynckeystate).
- [M23] Microsoft, [Windows on Arm overview](https://learn.microsoft.com/en-us/windows/arm/overview), [ARM64EC interoperability and DLL compatibility](https://learn.microsoft.com/en-us/windows/arm/arm64ec).
- [M24] Microsoft, [RegisterHotKey](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey).
- [M25] Microsoft, [extended window styles, WS_EX_NOACTIVATE](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles).
- [T1] Tauri, [global shortcut plugin guide](https://v2.tauri.app/plugin/global-shortcut/), [plugin 2.3.2 source](https://github.com/tauri-apps/plugins-workspace/blob/global-shortcut-v2.3.2/plugins/global-shortcut/src/lib.rs).
- [T2] Tauri, [global-hotkey 0.8.0 Windows implementation](https://github.com/tauri-apps/global-hotkey/blob/global-hotkey-v0.8.0/src/platform_impl/windows/mod.rs#L146-L174). `WM_HOTKEY`, polling release, and the 50 ms sleep; registration uses `MOD_NOREPEAT` earlier in the file.
- [T3] Tauri, [WebviewWindowBuilder 2.11.5 source](https://github.com/tauri-apps/tauri/blob/tauri-v2.11.5/crates/tauri/src/webview/webview_window.rs), [Tao 0.35.3 Windows window state](https://github.com/tauri-apps/tao/blob/tao-v0.35.3/src/platform_impl/windows/window_state.rs), [Windows window implementation](https://github.com/tauri-apps/tao/blob/tao-v0.35.3/src/platform_impl/windows/window.rs).
- [A1] Microsoft, [Windows Terminal interaction settings, multiline paste warning](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/interaction#warn-when-the-text-to-paste-contains-multiple-lines).
- [A2] Microsoft, [VS Code terminal basics](https://code.visualstudio.com/docs/terminal/basics#_copy-paste). Warning, bracketed-paste conditions, and paste troubleshooting.
- [A3] Microsoft, [Outlook keyboard shortcuts](https://support.microsoft.com/en-US/accessibility/outlook/keyboard-shortcuts-for-outlook).
- [A4] Microsoft, [use chat in VS Code](https://code.visualstudio.com/docs/chat/chat-overview).
- [A5] W3C, [Clipboard API and events](https://www.w3.org/TR/clipboard-apis/#clipboard-event-paste). Cancelable paste and synthetic-event limitations.
- [A6] Microsoft, [Teams keyboard shortcuts](https://support.microsoft.com/en-us/accessibility/teams/keyboard-shortcuts-for-microsoft-teams).
