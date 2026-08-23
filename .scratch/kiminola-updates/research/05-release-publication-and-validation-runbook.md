# Research 05 - Release publication and validation runbook

Date researched: 2026-08-23

Status: resolved as a research finding; no application code changed.

## Conclusion

The repository has an installer-release path, not a working Tauri auto-update path. A stable release should be made by pushing a matching `v<version>` tag, waiting for both matrix jobs to finish, testing the generated draft assets, and only then publishing the draft. Publishing is the visibility boundary: GitHub's `latest` release excludes drafts and prereleases, and draft-release assets are not available to ordinary public readers.

The current workflow must not be advertised as updater-ready. `.github/workflows/release.yml` sets `uploadUpdaterJson: false`; `kiminola/src-tauri/tauri.conf.json` does not set `bundle.createUpdaterArtifacts`, does not define `plugins.updater`, and the updater plugin initialization is commented out in `kiminola/src-tauri/src/lib.rs`. Therefore the expected stable result for the current configuration is two public NSIS installers after publication, with no `latest.json` feed and no updater `.sig` assets. If auto-update is required, updater configuration/signing and workflow changes are prerequisites, outside this ticket.

## Exact tag -> draft -> publish sequence

Use `VERSION` for the bare SemVer (for example, `0.1.1`) and `TAG` for `vVERSION`.

1. Prepare and merge the release commit. Before tagging, verify that all intended release changes are on the commit to be released and that the version sources agree:

   ```powershell
   $version = (Get-Content -Raw kiminola/src-tauri/tauri.conf.json | ConvertFrom-Json).version
   $packageVersion = (Get-Content -Raw kiminola/package.json | ConvertFrom-Json).version
   $lockVersion = (Get-Content -Raw kiminola/package-lock.json | ConvertFrom-Json).version
   $cargoVersion = (Select-String -Path kiminola/src-tauri/Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"$').Matches.Groups[1].Value
   $version, $packageVersion, $lockVersion, $cargoVersion
   if (@($version,$packageVersion,$lockVersion,$cargoVersion) | Where-Object { $_ -ne $version }) { throw 'Version sources do not agree.' }
   ```

   Tauri recommends `tauri.conf.json > version` as the application version source; this checkout currently carries the same `0.1.1` value in `tauri.conf.json`, `package.json`, the root package entry in `package-lock.json`, and `src-tauri/Cargo.toml`. The workflow itself enforces only the Tauri config value: it computes `v$config.version` and compares it with `github.ref_name` ([release.yml](../../../.github/workflows/release.yml#L33-L41), [Tauri versioning](https://v2.tauri.app/distribute/)). The extra checks above prevent a package/build version drift from being hidden by that narrower CI guard.

2. Run the normal pre-tag checks on the release commit, including the repository's frontend check and any release-specific native/build checks appropriate to the target machine. Do not treat a green build alone as proof of installation, updater, or public-release behavior.

3. Create the tag on the exact release commit and push the tag. The workflow is triggered only by a pushed tag matching `v*` ([release.yml](../../../.github/workflows/release.yml#L3-L6)); Tauri documents the equivalent version-tag trigger pattern ([Tauri GitHub pipeline](https://v2.tauri.app/distribute/pipelines/github/)). For an existing local tag:

   ```powershell
   git show --no-patch --decorate HEAD
   git tag -a TAG -m "Kimi Nola TAG"
   git push origin TAG
   ```

   Do not create the release manually first. The configured `tauri-action` receives the pushed tag through `github.ref_name`, searches for that tagged release, and creates it when absent. Its documented behavior is to create a release when `tagName` identifies a tag and no release exists, and `releaseDraft: true` makes that release a draft ([tauri-action inputs and caveats](https://github.com/tauri-apps/tauri-action#usage)).

4. Wait for both matrix jobs for the same tag. The workflow builds `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` in parallel, runs the tag/version guard in each job, and calls `tauri-apps/tauri-action@v1` with the same tag, `releaseDraft: true`, and `projectPath: kiminola` ([release.yml](../../../.github/workflows/release.yml#L15-L27), [release.yml](../../../.github/workflows/release.yml#L80-L95)). The first job creates or updates the draft; the other job must upload its architecture's assets to that same draft. A successful single matrix leg is not enough.

5. Inspect and test the draft while it is still private to ordinary public readers. Confirm the workflow run is successful for both architectures and that the draft release has the two expected NSIS installer assets for the version, normally:

   ```text
   Kimi.Nola_VERSION_x64-setup.exe
   Kimi.Nola_VERSION_arm64-setup.exe
   ```

   Treat the actual asset names returned by GitHub as authoritative; do not accept a generic portable executable or an asset from another tag. The current bundle target is NSIS only ([tauri.conf.json](../../../kiminola/src-tauri/tauri.conf.json#L42-L45)). Download each draft installer using an authorized maintainer account, install it on a matching Windows architecture, launch it, and record the result. GitHub documents that only users with push access receive draft-release listings, which is why this is a maintainer-side test rather than a public download test ([GitHub releases REST API](https://docs.github.com/en/rest/releases/releases#list-releases)).

6. Publish only after both installers pass the installed-update test below and all release checks pass. In GitHub Releases, open the draft and select **Publish release**; do not mark it as a prerelease. GitHub's release instructions explicitly place asset attachment/testing before publication and state that the latest-release label is assigned to a published non-prerelease release, automatically by semantic versioning unless overridden ([Managing releases](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository#creating-a-release)).

## Stable public-release checks

After publication, verify the release through the public API and unauthenticated download URLs. For `TAG`:

```powershell
gh api repos/Trillx/Kiminola/releases/tags/TAG
gh api repos/Trillx/Kiminola/releases/latest
Invoke-WebRequest -Method Head https://github.com/Trillx/Kiminola/releases/download/TAG/Kimi.Nola_VERSION_x64-setup.exe
Invoke-WebRequest -Method Head https://github.com/Trillx/Kiminola/releases/download/TAG/Kimi.Nola_VERSION_arm64-setup.exe
```

The tag-specific release object must show `tag_name` equal to `TAG`, `draft: false`, `prerelease: false`, both asset names with `state: uploaded`, nonzero sizes, and the expected architecture/version in each filename. The `/releases/latest` result must resolve to this release (or to a newer intentional stable release), and its `tag_name` must not be a draft or prerelease. GitHub defines “latest” as the most recent non-draft, non-prerelease release, and public release information is available without authentication ([Get the latest release](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)).

For a stronger artifact check, download each asset from its `browser_download_url`, verify the HTTP response is successful and the file is nonempty, and compare the downloaded byte count and SHA-256 to the GitHub release asset metadata where available. Then launch the installed result on the matching architecture. A GitHub Actions success or an asset listing does not prove that the bundled application launches or that the native runtime DLLs are usable.

## `latest.json` and signature decision

### What the current checkout can prove

It cannot prove a stable updater feed because none is produced by the current release path:

- `uploadUpdaterJson: false` explicitly disables the tauri-action JSON upload ([release.yml](../../../.github/workflows/release.yml#L80-L95)).
- Tauri requires `bundle.createUpdaterArtifacts: true` (or the legacy v1-compatible mode) to create updater bundles and signatures; the current bundle block has neither setting ([Tauri updater signing/building](https://v2.tauri.app/plugin/updater/#building), [tauri.conf.json](../../../kiminola/src-tauri/tauri.conf.json#L42-L58)).
- Tauri's updater configuration requires a public key and endpoint; the current `tauri.conf.json` has no `plugins.updater` block, and `lib.rs` leaves updater initialization commented out ([Tauri updater configuration](https://v2.tauri.app/plugin/updater/#tauri-configuration), [lib.rs](../../../kiminola/src-tauri/src/lib.rs#L21-L28)).
- Tauri's Windows updater build produces an installer plus a matching `.sig` when updater artifacts are enabled and the signing key is supplied ([Tauri updater Windows artifacts](https://v2.tauri.app/plugin/updater/#building)).

Consequently, for the present installer-only release, a missing `latest.json` or `.sig` is expected and must not be silently treated as a failed installer release. Conversely, if the product claims auto-update support, missing files are a release blocker.

### Required checks once updater support is intentionally enabled

The release workflow must be changed/configured to generate updater artifacts with the signing private key available as the documented environment variable, configure the public key and a GitHub static JSON endpoint, and set `uploadUpdaterJson: true`. Tauri Action documents that it generates the static JSON for GitHub Releases, and its `uploadUpdaterSignatures` input defaults to true; the action also warns that `uploadUpdaterJson` matters only when the updater is configured ([tauri-action inputs](https://github.com/tauri-apps/tauri-action#usage), [Tauri static JSON format](https://v2.tauri.app/plugin/updater/#static-json-file)).

Then require all of the following after publication:

1. `GET https://github.com/Trillx/Kiminola/releases/latest/download/latest.json` returns HTTP 200 to an unauthenticated request. GitHub documents the `/releases/latest/download/<asset>` form for a latest release asset ([Linking to releases](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases)).
2. The JSON parses and has `version` equal to `VERSION` (a leading `v` is accepted by Tauri), plus complete platform entries for `windows-x86_64` and `windows-aarch64`; each entry has an updater-bundle URL and the literal signature content, not a URL to the `.sig` file. Tauri lists those fields as required for a static updater JSON file ([Tauri static JSON format](https://v2.tauri.app/plugin/updater/#static-json-file)).
3. Each JSON URL resolves to an asset in the same published release/tag and each signature entry has a corresponding uploaded `.sig` asset. Download the updater bundle and verify it against the configured public key, or exercise the installed previous version's updater check so the Tauri client performs that signature verification.

Do not call `latest.json` stable while it exists only on a draft. Drafts are not public, and `/releases/latest` deliberately excludes drafts and prereleases ([GitHub release API](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)). With the current workflow, the expected result of the `latest.json` probe is a 404; that is evidence of the current updater gap, not evidence that the installer release failed.

## Installed previous-version update test

This test is required before publishing a release that claims upgrade safety. It is also the only meaningful update test available in the current checkout because the updater plugin is not initialized.

1. On a clean test machine or disposable Windows VM, choose the matching architecture and install the last published stable Kimi Nola installer. Launch it once and confirm it starts. Create or retain a small meeting/data marker, then close the app. Record the old version and confirm `%LOCALAPPDATA%\Kiminola\data\kiminola.db` exists.
2. Without uninstalling the old version and without deleting `%LOCALAPPDATA%\Kiminola\data`, run the new tag's matching NSIS installer downloaded from the draft for pre-publication testing, or from the published tag for post-publication confirmation. Use the normal per-user installation path and allow the installer to upgrade the existing installation.
3. Launch Kimi Nola after the installer completes. Confirm the new version is installed, the app reaches its normal usable state, the previous meeting/data marker is still present, and the old installation did not leave a broken shortcut or duplicate install. Capture the installer exit/result and the installed executable version as evidence.
4. Repeat on both x64 and ARM64 if both artifacts are being released. A successful x64 upgrade does not validate the ARM64 installer.
5. If updater support is later enabled, add a second path: install the already-published previous version with its updater endpoint, invoke the app's update check, confirm it discovers `VERSION` from the public `latest.json`, accept the update, relaunch, and verify the new version and retained data. A direct new-installer-over-old-install test does not substitute for that signed in-app updater test.

## Release decision

For the repository as it exists on 2026-08-23, publish `TAG` only when both draft installer assets have passed installation/launch and previous-version upgrade checks. Publish with `draft: false` and `prerelease: false`, then run the public release/API/download checks. Do not claim stable auto-update availability, `latest.json` visibility, or updater signature coverage until the updater configuration and workflow are deliberately completed and the post-publication probes pass.

## Sources

Repository sources:

- [`.github/workflows/release.yml`](../../../.github/workflows/release.yml)
- [`kiminola/src-tauri/tauri.conf.json`](../../../kiminola/src-tauri/tauri.conf.json)
- [`kiminola/src-tauri/src/lib.rs`](../../../kiminola/src-tauri/src/lib.rs)
- [`kiminola/package.json`](../../../kiminola/package.json)
- [`kiminola/package-lock.json`](../../../kiminola/package-lock.json)
- [`kiminola/src-tauri/Cargo.toml`](../../../kiminola/src-tauri/Cargo.toml)

Official external sources:

- [Tauri GitHub pipeline](https://v2.tauri.app/distribute/pipelines/github/)
- [Tauri distribution and versioning](https://v2.tauri.app/distribute/)
- [Tauri updater plugin](https://v2.tauri.app/plugin/updater/)
- [Tauri GitHub Action README](https://github.com/tauri-apps/tauri-action)
- [GitHub: Managing releases in a repository](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository)
- [GitHub REST API: Releases](https://docs.github.com/en/rest/releases/releases)
- [GitHub: Linking to releases](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases)
