# Kimi Nola updates from GitHub Releases — Wayfinder Map

## Destination

A build-ready route for complete, signed in-app updates for installed Kimi Nola Windows x64 and ARM64 builds, using published stable GitHub Releases. Done when updater configuration, app UX, release publication, signing, update safety, and validation have no unresolved decisions before implementation begins.

## Notes

- **Domain:** desktop release distribution and in-app updates. Keep “release” (the published GitHub artifact set) distinct from “update” (the app’s check/download/install lifecycle).
- **Skills:** `/grilling`, `/domain-modeling`, and `/research` for research tickets.
- **Implementation anchors:** `.github/workflows/release.yml` creates one draft release, uploads x64/ARM64 assets in parallel, and generates `latest.json` after both jobs; the Tauri updater plugin and signed-artifact configuration live in `kiminola/src-tauri/`; the frontend updater dependency and `updater:default` capability are active.
- **Settled during charting:**
  - The destination is the complete updater path, not release publishing alone.
  - The app checks automatically after launch in a non-blocking way, but the user explicitly approves download, installation, and restart; an active recording must not be interrupted.
  - Only published stable releases feed production updates. Drafts and prereleases remain manual-install-only.
  - Tauri updater signing is separate from Windows Authenticode/SignPath signing. One Tauri updater keypair will be used; only its public key is tracked, the private key is stored as a GitHub Actions secret, and an offline backup is retained.
  - Pushing `vX.Y.Z` creates a draft release with x64 and ARM64 artifacts. The release is manually published after installer and updater validation.
  - Updates are forward-only. A bad release is withdrawn and replaced by a higher patch version; in-app downgrades are out of scope.

## Decisions so far

- Modern Tauri NSIS updater artifacts use one serialized `latest.json` job after the parallel architecture uploads; see [ticket 01](issues/01-tauri-artifact-and-manifest-topology.md).
- Normal current-user NSIS updates replace the installed app while meeting data and model files remain under `%LOCALAPPDATA%\Kiminola`; runtime preservation still requires the two-architecture installed test; see [ticket 02](issues/02-windows-update-safety-and-data-preservation.md).
- The app checks once, non-blocking, after launch; it shows a global banner and Settings state, defers safely, and blocks installation on the active recording route; see [ticket 03](issues/03-update-check-ux-and-cadence.md).
- One Tauri updater keypair is used; only the public key is tracked, while the private key/password belong in GitHub Actions secrets plus an offline backup; see [ticket 04](issues/04-provision-updater-signing-identity.md).
- Releases follow tag -> draft -> parallel assets -> serialized manifest -> human validation -> manual publication; see [ticket 05](issues/05-release-publication-and-validation-runbook.md).

## Not yet specified

<!-- The remaining gates are operational: add GitHub secrets and run the first signed release on both Windows architectures. -->

## Out of scope

- Beta, prerelease, or alternate update channels for this effort.
- In-app downgrade support or automatic rollback to an older version.
- Microsoft Store, winget update integration, or a hosted dynamic update service.
