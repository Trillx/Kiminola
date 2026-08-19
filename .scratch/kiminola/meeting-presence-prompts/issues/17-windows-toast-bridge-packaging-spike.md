# Windows toast bridge and packaging spike

Type: task
Status: resolved
Blocked by: Native toast accessibility and lifecycle

## Question

Build a disposable Windows x64/ARM64 spike that proves the selected native notification path can register the installed Kimi Nola artifact, show the Meeting prompt, receive actions while the app is running/minimized/terminated, and reject stale actions without starting capture. Record the required app identity, registration, dependency, elevation, and NSIS packaging facts before implementation planning.

## Resolution

Built the disposable probe at `spike/windows-toast/` and recorded the full result in `spike/windows-toast/RESULTS.md`.

- ARM64 and x64 release builds pass.
- Both binaries execute the current/stale/unknown action gate tests.
- `CreateToastNotifierWithId("com.kiminola.app")` returns, but notification settings return `0x80070490 (Element not found)` and showing returns `0x80070005 (Access is denied)` in the current unpackaged NSIS-based environment.
- No Kimi Nola AppX/MSIX package is installed on the test host, so installed-artifact identity and terminated-process activation are not proven.
- The ARM64 NSIS installer was then run with `/S` (exit `0`); `%LOCALAPPDATA%\Kimi Nola\kiminola.exe` launched successfully, but the post-install notification probe produced the same `0x80070490` / `0x80070005` result.

Keep the in-app pending Meeting prompt canonical and native toast best-effort. A separate installed-NSIS identity/COM activation task is required before native toast can be wired into production.
