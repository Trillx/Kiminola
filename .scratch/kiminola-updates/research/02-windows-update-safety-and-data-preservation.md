# Windows update safety and data preservation

**Ticket:** `.scratch/kiminola-updates/issues/02-windows-update-safety-and-data-preservation.md`
**Research date:** 2026-08-23
**Scope:** Tauri 2 Windows NSIS updater behavior, the current Kimi Nola packaging/data layout, and the runtime evidence required before calling an x64 or ARM64 update safe.

## Conclusion

The Kimi Nola data layout is safe from a normal NSIS update by construction: the SQLite database and downloaded models live below `%LOCALAPPDATA%\Kiminola`, while the current-user NSIS application directory is `%LOCALAPPDATA%\Kimi Nola`. Tauri's update-mode NSIS path replaces the installed application files and does not recursively delete arbitrary `%LOCALAPPDATA%` content. The built-in update cleanup also skips its app-data deletion branch when `/UPDATE` is present.

However, this is not yet runtime-proven, and the repository does not currently have a live updater path. The updater crate is declared, but its plugin registration is commented out; `tauri.conf.json` has no updater public key/endpoints, no `createUpdaterArtifacts`, and no updater install-mode configuration. The current bundle is NSIS-only, and the release workflow explicitly disables updater JSON upload. Therefore the ticket should be treated as **layout-safe, updater-unconfigured, runtime validation pending**.

## Current repository facts

| Area | Finding | Evidence |
| --- | --- | --- |
| Bundle | Only `nsis` is enabled. The config is version `0.1.1`; it has no `bundle.createUpdaterArtifacts` and no `bundle.windows.nsis` block. | [`tauri.conf.json`](../../../kiminola/src-tauri/tauri.conf.json#L1-L48) |
| Updater dependency | Rust `tauri-plugin-updater` and JavaScript `@tauri-apps/plugin-updater` are locked at `2.10.1`. | [`Cargo.lock`](../../../kiminola/src-tauri/Cargo.lock#L5014-L5028), [`package-lock.json`](../../../kiminola/package-lock.json#L1873-L1880) |
| Plugin registration | The updater builder call is present only as a commented line, with a comment saying the required config block is not present. | [`lib.rs`](../../../kiminola/src-tauri/src/lib.rs#L21-L29) |
| Permissions | `updater:default` is already granted, but permission alone does not initialize the Rust plugin or provide endpoint/key configuration. | [`default.json`](../../../kiminola/src-tauri/capabilities/default.json#L1-L10); [Tauri updater permissions](https://v2.tauri.app/plugin/updater/#permissions) |
| Release artifacts | The tag workflow builds x64 and ARM64 NSIS installers, but `uploadUpdaterJson: false` means the workflow does not publish the generated updater manifest. | [`release.yml`](../../../.github/workflows/release.yml#L47-L95) |
| Installer hooks | No `installerHooks` setting or hook file exists in the current repository search. Tauri supports hooks only when a hook file is configured. | [`tauri.conf.json`](../../../kiminola/src-tauri/tauri.conf.json#L35-L48); [Tauri NSIS hooks](https://v2.tauri.app/distribute/windows-installer/#extending-the-installer) |
| Meeting data | The database path is `%LOCALAPPDATA%\Kiminola\data\kiminola.db`; if `LOCALAPPDATA` is unavailable, code falls back to a `data` directory beside the executable. | [`db.rs`](../../../kiminola/src-tauri/src/db.rs#L1-L33) |
| Models | The normal model path is `%LOCALAPPDATA%\Kiminola\models\nemotron`; the same executable-relative fallback exists for portable operation. | [`models.rs`](../../../kiminola/src-tauri/src/models.rs#L38-L68) |

The fallback matters: a normal installed Windows run uses `%LOCALAPPDATA%`, but an executable-relative/portable run would put data inside the directory being replaced. The current release config does not enable a portable bundle, so the preservation conclusion below applies to the configured NSIS install path, not an ad-hoc portable deployment.

## Official Tauri behavior

### Two different install-mode settings

Tauri has two separate Windows settings that must not be conflated:

1. `bundle.windows.nsis.installMode` controls where the application is installed. The official Windows installer documentation says the default is current-user installation, under `%LOCALAPPDATA%`, with `perMachine` and `both` as alternatives. The current Kimi Nola config omits this setting, so it uses the current-user default. See [Tauri Windows install modes](https://v2.tauri.app/distribute/windows-installer/#install-modes).
2. `plugins.updater.windows.installMode` controls how the updater launches the downloaded NSIS installer. The official updater default is `passive`: a small progress window, no user interaction required. `basicUi` shows a basic interactive UI, and `quiet` is silent but cannot request administrator privileges by itself. See [Tauri updater Windows `installMode`](https://v2.tauri.app/plugin/updater/#installmode-on-windows) and the official [`WindowsUpdateInstallMode` source](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/config.rs#L8-L85).

The updater source maps these modes to NSIS arguments as follows:

| Updater mode | NSIS arguments supplied by Tauri | Expected user experience |
| --- | --- | --- |
| `passive` (default) | `/P /UPDATE /R /ARGS ...` | Progress window, automatic install, automatic restart with the prior app arguments. |
| `quiet` | `/S /UPDATE /R /ARGS ...` | Silent install, automatic restart; only appropriate when elevation is already available or the install is user-wide. |
| `basicUi` | `/UPDATE` | Interactive installer; no automatic `/R` restart argument is added. |

The `/P`, `/S`, and `/R` mapping is defined in the official updater source, not inferred from the documentation: [`config.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/config.rs#L36-L53). The updater constructs `/UPDATE`, conditionally adds restart arguments and `/ARGS`, then launches the NSIS executable: [`updater.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs#L3475-L3517) and [`updater.rs` argument construction](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs#L3699-L3749).

### Shutdown and restart

On Windows, Tauri's updater runs the configured `on_before_exit` callback, launches the extracted NSIS installer with `ShellExecuteW`, and then exits the current application process with `std::process::exit(0)`. The plugin's builder defaults `restart_after_install` to `true`. The official API documents both facts: [`Update::install`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs#L3475-L3517), [`restart_after_install`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs#L3527-L3543), and [Tauri's Windows before-exit guidance](https://v2.tauri.app/plugin/updater/#windows-before-exit-hook).

The NSIS template consumes `/R` only for passive/silent update execution. After a successful install, it reads `/ARGS` and calls Tauri's `RunAsUser` helper with the installed executable and those arguments. The relevant template behavior is [`installer.nsi`](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi#L693-L704). This means the normal recommended path is:

```text
app -> on_before_exit -> app exits -> NSIS setup /P /UPDATE /R /ARGS ...
    -> files replaced -> installed app restarted with preserved arguments
```

The updater's process exit is not a durability guarantee for an in-memory meeting. Kimi Nola's durable meeting-save path is an explicit database transaction, and the recording command exposes `stop_recording` separately from `save_meeting`; the update test must therefore seed a meeting that has completed its save before starting the update. See [`recording.rs`](../../../kiminola/src-tauri/src/recording.rs#L236-L252) and [`db.rs` save transaction](../../../kiminola/src-tauri/src/db.rs#L353-L478).

### What `/UPDATE` does in the NSIS template

The official Tauri NSIS template parses `/UPDATE` into `UpdateMode` in `.onInit`. In update mode:

- the existing-install page proceeds without invoking the old uninstaller;
- the WebView2 installation section is skipped;
- the install section sets `$INSTDIR`, runs the normal running-process check, copies the new main executable/resources/binaries, rewrites installer metadata, and runs any configured post-install hook;
- the uninstall section's shortcut, autostart, and app-data cleanup branches are guarded by `UpdateMode <> 1`;
- the installer auto-closes when update mode is active.

These are direct properties of the official template: argument parsing and update-mode routing are in [`installer.nsi`](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi#L446-L490), the update-mode reinstall bypass and copied-file section are in [`installer.nsi`](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi#L290-L359), and cleanup guards are in [`installer.nsi`](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi#L725-L834).

Before copying, the template calls `CheckIfAppIsRunning`. For a current-user install it uses the current-user process finder/killer; otherwise it uses the broader process finder/killer. This is the reason the updater's application shutdown must be tested with the real installed executable and any auxiliary processes, not merely with a build exit code. See the official [`utils.nsh`](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh#L19-L67) macro and the template's call site at [`installer.nsi`](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi#L597-L607).

## Preservation assessment for Kimi Nola

### Meeting database

For the configured current-user NSIS install, `$INSTDIR` is the application directory (`%LOCALAPPDATA%\Kimi Nola` by default). Kimi Nola opens `%LOCALAPPDATA%\Kiminola\data\kiminola.db`, a sibling application-data tree rather than a child of `$INSTDIR`. The normal `/UPDATE` copy operation therefore has no path that targets the database, its directory, or its SQLite sidecars.

**Assessment:** a normal NSIS update should preserve the database. This is a source-backed layout conclusion, not a completed runtime proof. A future custom hook could invalidate it if it recursively deletes `%LOCALAPPDATA%\Kiminola`, so the generated installer script must be inspected whenever hooks are added or changed.

### Downloaded model pack

The normal model path is `%LOCALAPPDATA%\Kiminola\models\nemotron`, also outside `$INSTDIR`. The model downloader writes `.part` files and atomically renames a verified part to its final path; the NSIS update does not target that tree. See [`models.rs`](../../../kiminola/src-tauri/src/models.rs#L48-L68) and the downloader's final rename/verification path [`models.rs`](../../../kiminola/src-tauri/src/models.rs#L170-L280).

**Assessment:** a normal NSIS update should preserve a valid downloaded model pack, and the post-update application should reuse it rather than redownload it. This requires a same-manifest update test; if a future app version changes the embedded model manifest, a download may be intentional and is a separate model-migration case.

### Uninstall distinction

Preservation during update must not be confused with deletion during uninstall. The official NSIS template's optional `Delete app data` checkbox removes `$APPDATA\${BUNDLEID}` and `$LOCALAPPDATA\${BUNDLEID}` only. Kimi Nola's current data/model roots use the literal `Kiminola` directory, while the bundle identifier is `com.kiminola.app`; the default template therefore does not target the current database/model paths even during that explicit cleanup branch. That is a separate uninstall-policy finding, not a reason to add update-time cleanup.

## Runtime validation required before closing the ticket

The following must be run with real signed release-like artifacts, not only `cargo check`, `npm run build`, or a successful CI bundle. Run the full matrix on a native Windows x64 machine and on native Windows ARM64 hardware. Tauri's documentation notes that the NSIS installer executable itself is x86 on ARM machines and runs under emulation, while the installed application can be native ARM64; validate both the installer path and the installed app architecture. See [Tauri ARM build notes](https://v2.tauri.app/distribute/windows-installer/#building-for-32-bit-or-arm).

For each architecture:

1. **Install baseline.** Install the version N NSIS artifact in the configured current-user mode. Record the installed executable path, architecture, file version, uninstall registry `InstallLocation`, and `DisplayVersion`. Confirm that the app launches from the installed path and that all bundled native DLLs load.
2. **Create durable fixtures.** Through the installed app, create at least two meetings with distinctive titles, transcript text, edited segment text, notes, a template, and a note draft. Stop/save the meeting before updating. Record a semantic snapshot of IDs, titles, timestamps, transcript text, notes, drafts, and template rows. Record the database path and SQLite `integrity_check`; copy the DB only after the app is closed.
3. **Download and verify a model.** Complete the real model download. Record every expected file under `%LOCALAPPDATA%\Kiminola\models\nemotron`, byte length, and SHA-256. Confirm the app's model-health command returns true and run a short local ASR smoke test. Keep the embedded model manifest unchanged between N and N+1 for this preservation test.
4. **Exercise the actual updater.** From the installed N app, check/download/install the signed N+1 NSIS artifact. Capture updater logs, the `on_before_exit` event, the installer command line if available, process exit status, and whether the app restarts automatically. For the recommended passive mode, prove the observable sequence: old app exits, NSIS shows progress, the installer returns successfully, and the N+1 app relaunches with the expected arguments. Also run one controlled `basicUi` case to document that it requires user completion and does not receive Tauri's automatic `/R` path.
5. **Verify application replacement.** Confirm the installed executable and each bundled native DLL now has N+1 version/build identity and the correct PE machine type (`x64` for `x86_64-pc-windows-msvc`; `ARM64` for `aarch64-pc-windows-msvc`). Confirm the uninstall registry still points to the same install directory and reports N+1.
6. **Verify database preservation.** Confirm `%LOCALAPPDATA%\Kiminola\data\kiminola.db` still exists, run SQLite `PRAGMA integrity_check`, and compare the post-update semantic snapshot with the baseline. Do not require the DB file hash to remain identical because migrations or SQLite header metadata can legitimately change; require all fixture rows/content and the expected migration result instead. Confirm no data directory or SQLite sidecar was removed.
7. **Verify model preservation without redownload.** Confirm every pre-update model file still exists with the same byte length and SHA-256, the model-health command still returns true, and a local ASR smoke test succeeds. Repeat the launch with access to the model download host blocked while allowing the app to start; success proves the update reused the existing model instead of silently redownloading it.
8. **Verify restart and close behavior.** After the automatic restart, confirm exactly one Kimi Nola app instance is running, no stale old-version process remains, and the app can open the library and load the model. Repeat while the app is idle with the DB open, and once with the meeting UI open but after the meeting has been saved. Do not claim an active unsaved recording survives: the updater's required shutdown is not a recording-session checkpoint.
9. **Verify update-mode filesystem scope.** Before and after the update, inventory the application directory and the two `%LOCALAPPDATA%\Kiminola` trees. The application directory may change; the database and model trees must not be deleted, renamed, or recreated empty. If any `installerHooks` file is introduced later, repeat the test with the generated NSIS script and test both update and explicit uninstall paths.

The pass criterion is two independent successful runs per architecture, including one clean baseline install and one update over a populated database/model tree. A successful installer exit or a new version shown in Add/Remove Programs alone is insufficient evidence.

## Sources

- [Tauri Updater documentation](https://v2.tauri.app/plugin/updater/) — updater registration, signatures, artifacts, Windows modes, static manifest shape, Windows shutdown, and permissions.
- [Tauri Windows Installer documentation](https://v2.tauri.app/distribute/windows-installer/) — NSIS packaging, hooks, install modes, and ARM build behavior.
- [Official updater configuration source](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/config.rs) — exact Windows updater mode defaults and NSIS argument mapping.
- [Official updater implementation](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs) — NSIS launch, `/UPDATE`, restart arguments, `/ARGS`, process exit, and default restart state.
- [Official Tauri NSIS template](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi) — update-mode routing, file-copy scope, restart, and cleanup guards.
- [Official Tauri NSIS utilities](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh) — running-process detection and termination.

## Research limitation

The repository currently has no configured updater endpoint/key or runnable updater registration, and this session exposed no background-agent tool even though the Research skill requests one. The behavior above is therefore source-verified and repository-verified research; the x64/ARM64 installed-update run itself remains a required follow-up validation, not an assertion that it has passed.
