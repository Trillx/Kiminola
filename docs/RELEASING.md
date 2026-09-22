# Releasing Kimi Nola for Windows

Kimi Nola ships NSIS installers and complete portable archives for Windows x64
and ARM64. Tag builds carry Tauri updater signatures; that identity is separate
from Authenticode/SignPath trust for Windows prompts. The in-app updater checks
the published stable GitHub Release feed and installs only a higher updater-
signed version. Drafts and prereleases are never offered in the app.

## One-time repository setup

The Tauri updater has its own signing identity. It is separate from any
SignPath Authenticode certificate used for Windows trust prompts.

Keep the private key and password outside the repository, with an offline
backup. The public key is committed in
`kiminola/src-tauri/tauri.conf.json`. Configure these GitHub Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY` - the complete contents of the private key file.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` - the key password.

For a local `gh` installation, these commands send the files directly to
GitHub without printing their contents:

```powershell
Get-Content -Raw "$env:USERPROFILE\.tauri\kiminola-updater.key" |
  gh secret set TAURI_SIGNING_PRIVATE_KEY --repo Trillx/Kiminola
Get-Content -Raw "$env:USERPROFILE\.tauri\kiminola-updater.key.password" |
  gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo Trillx/Kiminola
```

Do not rotate this key casually. Existing installed versions trust the public
key embedded in their binary; a key rotation requires a manually installed
bridge release before those versions can accept updates signed by the new
identity.

### Hardware-validation runners

Register two dedicated Windows runners and label them `kiminola-hardware` in
addition to their default `self-hosted`, `Windows`, and `X64` or `ARM64` labels.
Provision Visual Studio Build Tools, the Windows SDK, LLVM under
`C:\Program Files\LLVM`, and the pinned Nemotron model pack on each runner.
Also provision a deterministic 16 kHz mono PCM speech fixture at
`%LOCALAPPDATA%\Kiminola\hardware\speech-test.wav` and its expected transcript
at `%LOCALAPPDATA%\Kiminola\hardware\speech-test.txt`, then configure this
repository variable:

- `KIMINOLA_HARDWARE_CI=true`

`KIMINOLA_PREVIOUS_RELEASE_TAG=vX.Y.Z` is optional. When omitted, the workflow
uses the latest published stable release; a manual dispatch can override either
source. A missing hardware configuration fails visibly and blocks tag releases
instead of reporting a green no-op.

The runners must use dedicated test accounts. The workflow backs up any existing
Kimi Nola application and data directories before replacing them and restores
those directories in a `finally` block. It never receives updater signing
secrets. The scheduled run verifies native runner and LLVM architecture,
non-silent physical-microphone capture under a known acoustic stimulus, classic
and process-tree loopback, deterministic speech transcription against the
expected text, native packaging, installation, startup, and preservation across
a previous-to-current installer upgrade. Before executing the previous installer,
the workflow verifies its updater signature against the public key embedded in
the app. The update test inserts a unique meeting-and-note row into the real
SQLite database and checks that exact row after installation; it also compares
every provisioned model-file hash.

Ordinary pull requests do not use physical devices. They build and start native
unsigned x64 and ARM64 packages on GitHub-hosted runners and retain the installer
and complete portable archives for seven days. Each archive contains
`kiminola.exe` and all required ONNX Runtime/sherpa DLLs.

## Release flow

1. Set the same version in `kiminola/package.json`,
   `kiminola/src-tauri/Cargo.toml`, and
   `kiminola/src-tauri/tauri.conf.json`.
2. Merge the version change to `main`, then push the matching tag:

   ```powershell
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

3. The tag workflow checks all three application versions, validates the tag,
   and tests the release support scripts. It then runs the full physical x64 and
   ARM64 hardware workflow. Both hardware jobs must pass before a draft exists.
4. Native x64 and ARM64 release jobs build in parallel, verify the executable
   and four bundled DLL PE architectures, run startup smoke tests, and bundle
   that same verified executable. Each job uploads its NSIS installer, updater
   `.sig`, complete portable archive, and a SHA-256 verification report.
5. A final serialized job downloads the exact release assets, checks every
   report hash and portable-archive member, and verifies both updater signatures
   with pinned Minisign and the public key embedded in the app. It then uploads
   one `latest.json` whose URLs and literal signatures must match the verified
   installers, downloads that uploaded manifest, and requires its SHA-256 to
   match the validated local file. Any mismatch leaves the release as a draft.
6. Test the draft installers on matching Windows architectures. For an
   update-enabled baseline, install the previous published version, create a
   small meeting/data marker, and accept the update from inside the app. Check
   that the app restarts once and that the SQLite database and downloaded model
   files remain intact.
7. Open the draft, confirm both architectures, portable archives, and
   `latest.json`, and select
   **Publish release**. Keep it non-prerelease; this is what makes the stable
   `releases/latest/download/latest.json` endpoint resolve to it.
8. After publication, verify the public manifest and both installer URLs. The
   manifest must contain complete entries for `windows-x86_64` and
   `windows-aarch64`, with literal signature contents rather than `.sig` URLs.
   Installer URLs must be the permanent tag-versioned
   `releases/download/<tag>/` URLs; draft-only `untagged-*` URLs stop
   resolving at publication.

The existing `v0.1.1` installation predates the updater configuration. Users
on that version need one manual install of the first updater-enabled release;
after that bridge release, later stable releases can arrive through the app.

## Safety rules

- Never publish a draft or prerelease as the stable update feed.
- Never install or restart while the recording route is active. Finish and
  save the meeting first.
- Do not use an update to downgrade. If a release is bad, withdraw it from the
  stable path and publish a higher patch version.
- Treat a successful GitHub build as necessary but insufficient: installation,
  launch, restart, database preservation, model preservation, and live updater
  discovery still require a Windows test on each architecture.

## Database and shutdown validation

CI runs locked Rust all-target and documentation tests on Windows x64 and again
on native x64 and ARM64 release jobs. Native package jobs also run an executable
startup smoke test. Physical x64 and ARM64 validation is a required tag gate,
and signed in-app updater acceptance remains a manual publication gate. Run
`npm test`, `cargo test --locked --all-targets`, and `cargo test --locked --doc`
with the matching native DLL directory on PATH.

The app saves pending editor changes and waits for outstanding app commands
before installation. Its native update barrier blocks new recordings and
closes the database pool. If saving fails, installation is cancelled and the
app stays open. Exercise a downloaded update immediately after typing, while
a save is still running, after a save failure, and during a recording.

Existing databases receive a verified `VACUUM INTO` snapshot before pending
migrations run. Snapshots live in `%LOCALAPPDATA%\Kiminola\data\backups` and
are retained across updates. Backups are local, unencrypted like the source
database, and contain notes and transcripts. They do not include API keys or
downloaded models. Published migration files must remain byte-for-byte stable;
add a new migration instead of editing an applied one.

Database startup errors show a recovery screen. Retry restarts the app after
successful initialization. Restore requires an explicit user choice and
confirmation, validates and upgrades a temporary copy, then archives the
original database and sidecars under `data\before-restore-*`. Changes after the
backup will not appear in the restored library; the archived originals remain
available. An interrupted restore leaves `kiminola.restore-pending`, which
blocks normal startup until recovery succeeds. Do not delete that marker or
replace the database with an empty file to bypass an error.

For an installed release test, compare populated fixtures and migration history
before and after updating. Include notes, enhanced notes, transcript timestamps,
custom templates, settings, draft recovery text and library destinations.
Confirm model hashes and local ASR still work. Exercise backup creation failure,
an invalid migration, a corrupted backup, and interrupted restore using isolated
test data. Neither a build nor these unit tests prove the signed installer path.

Reference documentation: [Tauri updater](https://v2.tauri.app/plugin/updater/),
[Tauri GitHub pipeline](https://v2.tauri.app/distribute/pipelines/github/), and
[GitHub releases](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository).
