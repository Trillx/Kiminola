# Windows toast bridge and packaging spike results

Date: 2026-08-18  
Host: Windows ARM64 (`aarch64-pc-windows-msvc`)  
Targets: `aarch64-pc-windows-msvc`, `x86_64-pc-windows-msvc`  
Windows binding: `windows 0.58.0`

## What was built

The sibling `Cargo.toml` and `src/main.rs` are a disposable Rust probe. It is not part of `src-tauri` and has no path to Kimi Nola recording, audio, ASR, SQLite, or production state.

It exercises:

- WinRT apartment initialization.
- `ToastNotificationManager::CreateToastNotifierWithId` using Kimi Nola's `com.kiminola.app` identifier.
- Toast XML construction with the minimum Meeting prompt wording and three explicit actions.
- In-process activation callback plumbing.
- A prompt-ID/action gate that accepts one current action and rejects stale, unknown, or already-resolved actions.

## Build evidence

```text
cargo test --offline
3 passed; 0 failed

cargo build --offline --release
Finished `release` profile [optimized]

cargo check --target x86_64-pc-windows-msvc
Finished `dev` profile

cargo build --offline --release --target x86_64-pc-windows-msvc
Finished `release` profile [optimized]
```

The ARM64 and x64 release binaries were both executed on this ARM64 host. Both printed the action-gate passes:

```text
PASS: current actions are accepted once
PASS: stale, unknown, and already-resolved actions are rejected
PASS: this gate contains no recording/audio side effect
```

## Runtime result

On this machine there is no installed/package identity for Kimi Nola (`Get-AppxPackage -Name '*Kiminola*'` returned no package). The existing Kimi Nola artifact is an NSIS installer, not a Windows AppX/MSIX package.

The WinRT calls returned:

```text
CreateToastNotifierWithId(com.kiminola.app) succeeded
notification_setting=unavailable HRESULT=0x80070490 message=Element not found.
CreateToastNotifier() HRESULT=0x80070490 message=Element not found.
Show HRESULT=0x80070005 message=Access is denied.
```

The same result occurred for the ARM64 and x64 probe binaries. Therefore:

- API binding and architecture compilation are proven.
- The current unpackaged process does not prove a usable toast registration.
- Toast display is not proven.
- In-process activation is not proven because no toast was shown.
- Terminated-process activation, COM activator registration, NSIS identity registration, and installed-artifact behavior remain unproven.

## Installed NSIS sanity check

The ARM64 installer was run with `/S` and exited `0`. It installed the executable at `%LOCALAPPDATA%\Kimi Nola\kiminola.exe`, created per-user Start Menu shortcuts, and the installed executable launched successfully as process `kiminola`.

After installation, the ARM64 probe was run again. Its results were unchanged: the AUMID notifier call returned, settings were `0x80070490`, and showing the toast was `0x80070005`. The installer created an unpackaged desktop install; `Get-AppxPackage -Name '*Kiminola*'` still returned no package identity.

This verifies that the NSIS artifact installs and starts, but it does not verify notification activation because the current installed production executable does not contain the toast bridge yet. Terminated-process action delivery remains a follow-up implementation/packaging test.

## Production boundary

Do not make native toast the only Meeting prompt surface. The in-app pending prompt remains canonical. A future native bridge may be best-effort and must route every action through the same current-prompt ID gate; unavailable settings, denied display, Notification Center/DND suppression, stale actions, and missing packaging registration must fall back to the in-app prompt. No toast action may start capture without that validation.

The next separate packaging task is to test a real installed x64 and ARM64 NSIS artifact with a registered desktop identity and terminated-process activation. This scratch result is not evidence that that task has passed.
