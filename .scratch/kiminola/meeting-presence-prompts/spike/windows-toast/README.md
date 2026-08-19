# Windows toast spike

This is a disposable, production-isolated probe for the Meeting prompt notification seam. It does not import Kimi Nola, open a microphone, capture audio, write SQLite, or start recording.

Run from this directory with the LLVM bin directory available on `PATH`:

```powershell
$env:Path = 'C:\Program Files\LLVM\bin;' + $env:Path
$env:KIMINOLA_TOAST_TARGET = Join-Path $env:TEMP 'kiminola-windows-toast-target'
cargo test
cargo run -- test-actions
cargo run -- probe
cargo run -- show --wait
```

The `show` command first probes the Kimi Nola AUMID (`com.kiminola.app`), then uses the default Windows toast notifier only as a diagnostic fallback. A visible toast is evidence that the XML/API path works; it is not evidence that an installed NSIS artifact can receive actions after termination.

The output deliberately reports these separately:

- `registration=accepted` — Windows accepted the supplied AUMID for this process.
- `notification=shown` — the notification service accepted the toast.
- `activation=accepted` — the current process received an action and the prompt gate accepted its ID/action.
- `recording=false` or `recording=NOT_STARTED_BY_SPIKE` — this probe never starts capture.

The terminated-app activation path remains a separate packaging/COM-activation check. Do not promote this crate into `src-tauri` until the installed x64 and ARM64 artifacts have a verified identity, activation registration, stale-action behavior, and fallback to the in-app pending prompt.
