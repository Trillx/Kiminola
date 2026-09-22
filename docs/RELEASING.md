# Releasing Kimi Nola for Windows

Kimi Nola ships NSIS installers and complete portable archives for Windows x64
and ARM64. Tag builds carry Tauri updater signatures and SignPath Authenticode;
those are separate identities. The in-app updater checks
the published stable GitHub Release feed and installs only a higher updater-
signed version. Drafts and prereleases are never offered in the app.

## One-time repository setup

The Tauri updater has its own signing identity. It is separate from any
SignPath Authenticode certificate used for Windows trust prompts.

Keep the private key and password outside the repository, with an offline
backup. The public key is committed in
`kiminola/src-tauri/tauri.conf.json`. Configure these GitHub Actions secrets:

Protect `main` with required pull-request checks and protect the `v*` tag namespace
with a repository ruleset restricted to release maintainers. The workflow also fetches
and peels each tag and refuses to expose signing credentials or dispatch self-hosted
hardware unless that exact commit is reachable from `origin/main`.

- `TAURI_SIGNING_PRIVATE_KEY` - the complete contents of the private key file.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` - the key password.
- `SIGNPATH_API_TOKEN` - a SignPath CI user token restricted to this project and
  release-signing policy.

Configure these GitHub repository variables from the SignPath project:

- `SIGNPATH_ORGANIZATION_ID`
- `SIGNPATH_PROJECT_SLUG`
- `SIGNPATH_SIGNING_POLICY_SLUG`
- `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG` - an artifact configuration for the
  GitHub Actions ZIP wrapper shown below.
- `SIGNPATH_SIGNER_THUMBPRINT` - the 40-character SHA-1 thumbprint of the one
  SignPath certificate approved for Kimi Nola releases. Update it only as part
  of a documented certificate rotation.

`actions/upload-artifact` submits a ZIP even when it contains one executable,
so the SignPath artifact configuration must use a `<zip-file>` root and require
exactly one PE payload:

```xml
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file path="*.exe" min-matches="1" max-matches="1">
      <authenticode-sign />
    </pe-file>
  </zip-file>
</artifact-configuration>
```

Both signing requests leave `skip-decompress` disabled and verify the returned
certificate against `SIGNPATH_SIGNER_THUMBPRINT`; a valid signature from any
other certificate fails the release.

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
repository variables:

- `KIMINOLA_HARDWARE_CI=true`
- `KIMINOLA_X64_HARDWARE_MICROPHONE_NAME=<exact default input-device name>`
- `KIMINOLA_ARM64_HARDWARE_MICROPHONE_NAME=<exact default input-device name>`

The configured input names must identify the physical microphones attached to
the respective runners. Virtual, loopback, Stereo Mix, Voicemeeter, and cable
devices are rejected. The physical-microphone test requires the configured
device to capture the 440 Hz acoustic stimulus, rather than accepting arbitrary
non-silent ambient audio.

`KIMINOLA_PREVIOUS_RELEASE_TAG=vX.Y.Z` is optional. When omitted, the workflow
selects the highest updater-signed stable release strictly older than the
candidate; a manual dispatch can request a specific baseline only when it equals
that computed highest eligible release. Eligibility first requires the canonical
installer and signature assets; the selected signature is then verified
cryptographically before execution. An invalid signature blocks the gate rather than
silently falling back to an older release. A stale override is rejected. A missing
baseline or hardware configuration fails visibly and blocks tag releases instead
of reporting a green no-op.

The runners must use dedicated test accounts. The workflow snapshots the existing
Kimi Nola application directory, complete persistent user root, installer-managed
registry keys, and Start Menu/Desktop shortcuts into the stable, same-volume
`%LOCALAPPDATA%\KiminolaHardwareRecovery` root before replacing them. An atomic
write-through transaction journal is written before the first mutation, after
each filesystem snapshot, immediately after each durably flushed registry export,
before each live registry deletion starts, and again after each key is removed.
Immediately after checkout, every new run restores
or fails closed on an incomplete transaction before tool setup or hardware fixtures
are read, so cancellation, timeout, service termination, or restart cannot strand
recoverable state under runner temporary storage. The
journal and backups are removed only after every restoration check succeeds.
Incomplete snapshots are never deleted or nested during recovery; failures preserve
the recovery root for manual intervention. The workflow never receives updater signing
secrets. The scheduled run verifies native runner and LLVM architecture,
physical-microphone capture of the known 440 Hz acoustic stimulus against a
matched stimulus-off baseline while the source process remains live, classic
and process-tree loopback, deterministic speech transcription against the
expected text, native packaging, installation, startup, and preservation across
a previous-to-current installer upgrade. Before executing the previous installer,
the workflow decodes its Tauri Base64 signature and verifies the resulting
Minisign data against the public key embedded in the app. The update test inserts a
complete semantic fixture directly into the untouched previous-version SQLite
database, launches the candidate to apply any pending migrations, and checks the
meeting, raw and enhanced notes, timestamped transcript, custom template, setting,
recovery draft, and library destination through exact queries and application
deserialization afterward. A release with no new migration still verifies the complete
unchanged history and all preserved data. It also compares every provisioned model-file
hash. The legacy startup probe keeps the previous process alive until the migration
count from that exact release tag, compatible checksums, integrity check, required
tables, and default Personal space are readable.
Both installed versions must expose native executable and runtime-DLL PE machine
types matching the physical runner.

Ordinary pull requests do not use physical devices. Pull requests, `main` pushes,
and version tags build and start native unsigned x64 and ARM64 packages on
GitHub-hosted runners and retain the installer and complete portable archives for
seven days. Each archive contains
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
   requires the updater and SignPath credentials/configuration, tests the release
   support scripts, and reruns locked npm and Rust dependency audits. A
   GitHub-hosted gate then dispatches the physical x64
   and ARM64 checks as a separate workflow and verifies that run succeeded for the
   exact tag commit. Keeping self-hosted jobs outside the signing dependency graph
   preserves SignPath OSS trusted-build provenance. Both hardware jobs must pass
   before a draft exists.
4. Native x64 and ARM64 release jobs build in parallel, verify the executable
   and four bundled DLL PE architectures, submit the executable and NSIS installer
   to SignPath, and require `Get-AuthenticodeSignature` to report `Valid`. The
   installer is renamed to the canonical
   `Kimi.Nola_<version>_<arch>-setup.exe` form, and the updater signature is
   regenerated only after the installer receives its final Authenticode bytes.
   Read-only build jobs pass the installer, updater `.sig`,
   complete portable archive, and SHA-256/Authenticode verification report to a
   minimal publishing job; repository-write access is not exposed to build hooks
   or signing actions.
5. A read-only verification job downloads the exact release assets, checks every
   report hash, installer and extracted portable-executable Authenticode signer
   thumbprints, portable-archive member, and the PE architecture of every extracted
   executable and native DLL, then
   verifies both updater signatures with pinned Minisign and the public key
   embedded in the app. It generates one unsigned `latest.json` whose URLs and
   literal verified installer signatures must match the verified installers. A
   minimal publishing job uploads that file, downloads it again, requires its
   SHA-256 to match, and checks the exact final draft asset set. Any mismatch
   leaves the release as a draft.
6. Download the exact final signed draft installers and test them on matching Windows
   architectures. Confirm their SHA-256, Authenticode signer/timestamp, and updater
   signatures against the workflow reports before execution. Install the
   previous published version, create a small meeting/data marker, run the draft
   installer as an upgrade, and check that the new app starts with the SQLite
   database and downloaded model files intact. Transcribe the deterministic speech
   fixture through the installed application so inference exercises the DLLs beside
   the installed executable. Drafts are not exposed by the
   app's stable `releases/latest` feed, so this pre-publication gate deliberately
   validates the same installer directly rather than claiming an in-app update.
7. Open the draft, confirm both architectures, portable archives, and
   `latest.json`, and select
   **Publish release**. Keep it non-prerelease; this is what makes the stable
   `releases/latest/download/latest.json` endpoint resolve to it.
8. After publication, verify the public manifest and both installer URLs, then
   run an in-app updater smoke test from the previous stable release on x64 and
   ARM64. The
   manifest must contain complete entries for `windows-x86_64` and
   `windows-aarch64`, with literal signature contents rather than `.sig` URLs.
   Installer URLs must be the permanent tag-versioned
   `releases/download/<tag>/` URLs; draft-only `untagged-*` URLs stop
   resolving at publication.

`v0.1.2` is the first updater-enabled release. Users on `v0.1.1` or any older
installation need one manual upgrade to `v0.1.2` or later; after that bridge,
later stable releases can arrive through the app.

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
on native x64 and ARM64 release jobs. Native package jobs also run executable
startup behavior checks before their artifacts are accepted.
Physical x64 and ARM64 validation is a required tag gate. Because draft releases
are absent from the stable feed, in-app updater acceptance is a post-publication
smoke test rather than a pre-publication gate. Run
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
