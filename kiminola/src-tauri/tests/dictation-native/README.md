# Native dictation adapter fixture

Run from `kiminola/src-tauri` using a Windows Rust toolchain:

```text
cargo test --locked --manifest-path tests/dictation-native/Cargo.toml -- --nocapture --test-threads=1
cargo clippy --locked --manifest-path tests/dictation-native/Cargo.toml --all-targets -- -D warnings
cargo test --locked --lib dictation_native::tests -- --nocapture --test-threads=1
cargo clippy --locked --all-targets -- -D warnings
```

The standalone crate includes the production `dictation_native.rs`. It reexports the exact `global-hotkey` types that `tauri-plugin-global-shortcut` reexports, avoiding unrelated Tauri/ASR dependencies. The last two commands test and lint the actual application crate and real plugin import. The application's dev-dependencies already enable `tokio/test-util`.

## Test isolation

Normal tests create hidden, offscreen, nonactivating native Unicode EDIT controls containing synthetic text. The cross-process test launches its own test executable, receives that child's HWND through stdout, verifies the HWND's process ID, and only queries/changes that disposable control. It does not acquire another application's focus. Its ignored `owned_editor_subprocess` entry point is exercised by the normal parent test.

Delivery-core tests use test-only `EM_REPLACESEL` dispatch into those owned controls. Production dispatch is one `WM_PASTE`. This tests UTF-16 content/selection verification, preflight refusal, partial delivery, failure and uncertainty handling. It does not establish a successful clipboard-based paste through the public foreground-target path.

## Cross-process failure diagnostics

The child publishes its HWND only after its message loop consumes a private queued readiness marker. A leading newline keeps the stdout protocol separate from libtest's serial test label. The parent continuously drains that pipe and joins its reader after killing/waiting for the owned child, including on assertion failure. Readiness is bounded by five seconds. It does not extend the production message timeout or guarantee later scheduling.

The fixture subclasses only its own hidden EDIT and forwards messages to the original Unicode window procedure. It buffers up to 128 relevant message IDs, native-call durations and results in child memory. It writes the trace only when the parent posts a diagnostic request after delivery, outside the timed calls. The parent records readiness, architecture, exact synthetic UTF-16 state, dispatch result, immediately sampled Win32 error, duration, guard phases and child status. A successful send need not clear last error; interpret that error only when dispatch failed.

The post-outcome read is diagnostic only. The original `deliver_editor` outcome must still be `Verified`, dispatch must run exactly once, and content/caret assertions remain exact. A later correct readback cannot turn `Uncertain` into success. There is no insertion retry, timeout increase, clipboard access, foreground activation or input injection.

Focused commands:

```text
cargo test --locked --lib dictation_native::tests::cross_process_native_editor_reads_and_verifies_only_disposable_text -- --exact --nocapture --test-threads=1
cargo test --locked --manifest-path tests/dictation-native/Cargo.toml --lib dictation_native::tests::cross_process_native_editor_reads_and_verifies_only_disposable_text -- --exact --nocapture --test-threads=1
```

Interpret the trace in sequence. `0xb1` sets the initial selection, `0xc2` is the single insertion, and each complete read consists of password character `0xd2`, text length `0xe`, text `0xd`, text length `0xe`, selection `0xb0`, and limit `0xd5`. The first two reads capture the initial state and production preflight. Reads after insertion are production verification followed by the explicitly labelled diagnostic read. A dispatch error isolates the send failure. A successful dispatch with missing/slow production read messages differs from a complete read with mismatched text, selection, style or limit. No post-dispatch guard call means production verification did not reach its successful predicate.

Hosted run `35894704924` at `9c4bec7` reported `Uncertain`, but contained no dispatch error, timings or readback state. Its passing low-integrity child test does not distinguish timeout from readback mismatch. On local ARM64, the unchanged baseline passed 200 exact-test runs and 80 native-suite runs. The instrumented fixture passed 400 exact-test runs and 80 native-suite runs across both real and standalone crates, with serial and parallel execution. Both crates' focused suites and all-target clippy also passed. These results do not establish the hosted x64 cause or prove that readiness fixes it.

Scratch-only fault probes delayed dispatch, delayed the first post-dispatch read, and changed the owned caret. All three still failed the original `Verified` assertion with one insertion and distinct diagnostics; the unmodified control passed. Those deliberately induced failures validate the diagnostic oracle, not a claim about the hosted cause. The next hosted full-suite run must retain its failing test's `owned editor` lines, native trace, architecture and token observations. Compare an exact-test run on that same runner if the parallel suite alone fails. Production timeout, security rules and the empty delivery allowlist remain unchanged.

## Runner tokens and security policy

Owned-editor marshalling does not require the runner or child to be a production-eligible target. Hosted runners may inherit elevated or non-medium tokens. `process_security_matches_observed_runner_and_child_tokens` separately queries the runner's and owned child's elevation, UIAccess and integrity RID, prints those observations, and checks the production verdict against them. Eligible live processes must return their actual creation time; ineligible tokens must return the target-security error. Failed token queries fail the test, rather than counting as expected rejection.

`only_non_elevated_medium_integrity_editors_are_eligible` covers integrity boundaries and elevation/UIAccess combinations deterministically. Only non-elevated, non-UIAccess tokens in the medium integrity band pass. These are policy cases, not claims that elevated or UIAccess processes were launched locally.

`ineligible_runner_still_exercises_owned_editor_and_release_gate` starts a fresh exact-test child and lowers only that child's process integrity to low, unless it is already low or below. It verifies the resulting token and production rejection, then executes the security-verdict, cross-process readback and public delivery-refusal tests. The parent verifies its own token did not change. This test reproduces both original runner-token assumptions without elevation, UAC changes, interactive prompts, clipboard access or changes to other processes. A failure to lower or inspect the token fails the test; it does not skip coverage.

Focused token regression command:

```text
cargo test --manifest-path tests/dictation-native/Cargo.toml -- --exact dictation_native::tests::ineligible_runner_still_exercises_owned_editor_and_release_gate --nocapture --test-threads=1
```

The normal suite also runs this regression. Readback success under a policy-rejected token proves fixture isolation, not production target eligibility or release qualification.

## Input and interruption isolation

The interruption test creates the real native monitor thread, hidden top-level window, WTS registration and activity hooks. It sends synthetic session and power messages only to that owned monitor. It never locks or suspends Windows. Public snapshot and copy functions are tested for refusal during that synthetic locked state. Unlock/resume produce no capture callback.

The chord test supplies virtual-key states and releases each primary/modifier component. It proves the decision rule, not physical keyboard event delivery.

The activation-key regression runs in a fresh exact-test child process, separate from the suite's real monitor and its global activity counters. It calls the production keyboard callback directly with synthetic `KBDLLHOOKSTRUCT` events, without installing hooks or injecting input. The test covers normal and system key-up, repeat events during the original hold, later same-key presses, independent remaining chord keys, duplicate releases, ignored hook codes/messages and out-of-range key codes. It does not change the user's keyboard state or clipboard, activate an application, or establish physical keyboard delivery.

The source finding that activation-key exemptions persisted after release is fixed: `WM_KEYUP` and `WM_SYSKEYUP` consume that key's exemption, so later key-down and repeat events count as activity. Original-held keys stay exempt only until their own release. The regression failed on a later Space press before the fix and passes with it. This closes that counter defect, not the remaining timing and qualification limits below. Automatic-delivery certification remains empty.

## Explicit clipboard qualification

```text
cargo test --manifest-path tests/dictation-native/Cargo.toml -- --ignored --exact dictation_native::tests::private_clipboard_copy_is_unicode_and_excludes_history_cloud --nocapture
```

This test attempts to create a private window station and desktop in a separate process before opening any clipboard. A separate desktop alone would not isolate clipboard data. If station creation is denied, the test fails before touching any clipboard. It never falls back to WinSta0's clipboard and never reads or saves the user's clipboard.

The implementation host denied `CreateWindowStationW` with `HRESULT(0x80070005)`. Actual Unicode clipboard publication, exclusion-format readback and production `WM_PASTE` remain unqualified here. The ignored test is a release-qualification task, not a passing test.

## Release qualification gate

No production application/version is certified in this build. `CERTIFIED_APPLICATION_VERSIONS` is deliberately empty. Public `snapshot_target` refuses before inspecting any foreground editor or surrounding text. Public `paste_to_target` independently requires certification before consuming an attempt, validating the editor, publishing to the clipboard or dispatching. Clipboard consent does not bypass qualification. The backend keeps final text in review and offers explicit Copy, whose desktop checks and error handling still apply. This is not a claim that successful clipboard publication has been qualified.

Regression tests call both public functions. The delivery test uses a privately constructed, deliberately ineligible snapshot of a hidden, test-owned EDIT with a sentinel creation time. It checks the exact qualification error and that both initial attempt states and editor content stay unchanged. Constructing this snapshot does not call `process_security`: early refusal must not require an eligible runner. These tests do not require foreground activation, clipboard access or a real user's editor. They do not replace the existing private native delivery tests.

A future allowlist entry must verify the retained live process's exact application/version and tested platform, with recorded disposable-editor clipboard and `WM_PASTE` evidence. A Unicode EDIT class, user consent, compilation or the synthetic dispatch below cannot certify an application. Unknown identities must fail closed.

## Native checks behind the gate

The retained native implementation only considers a focused, visible, enabled Unicode EDIT. These checks do not establish application compatibility. Password characters/styles, read-only, numeric/case-converting/OEM styles, non-medium integrity, elevation/UIAccess, own-process targets, known terminal executables, oversized text, changed focus/content/selection/activity and unavailable input desktops cause review/copy fallback. RichEdit, browser, Electron and custom accessibility editors are not added by name or assumed compatible.

The snapshot retains a process handle, process creation time, UTF-16 content and selection privately in memory. No surrounding content is logged, serialized, persisted or sent to the provider. Content is capped at 32,000 UTF-16 units. A snapshot is single-attempt. Clipboard use requires backend consent. A held input key causes refusal; no keys are synthesized or released and Enter is never emitted.

The owned monitor lives for the process lifetime. Its callback must request cancellation and return promptly. If activity hooks cannot be installed, automatic-target snapshots fail closed while lock/suspend notification remains available. A dead monitor cannot report a successful restart.

## Remaining safety limits

- Windows eligibility checks, input hooks, clipboard access and dispatch are not atomic. Target/clipboard changes after the last check cannot be ruled out. Post-dispatch doubt returns `Uncertain`, never an automatic retry.
- Asynchronous WinEvent delivery and low-level hook loss cannot provide a proof that no transient change occurred. Exact content, selection, focus, process and desktop are rechecked.
- Clipboard exclusion formats request Windows history/cloud suppression. Third-party clipboard readers can ignore them. Prior formats are not restored.
- Backend session invalidation and clipboard consent remain preconditions. A synchronous native call cannot infer a concurrently cancelled backend generation.
- Actual lock/suspend, secure-desktop transitions, physical chord release, user-clipboard behavior, named application compatibility and x64 execution were not exercised by these fixtures. ARM64 native EDIT readback and synthetic interruption delivery were exercised.
