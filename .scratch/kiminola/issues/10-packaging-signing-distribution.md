# Packaging, signing, and distribution for Windows x86/x64/ARM64

Type: grilling
Status: resolved
Blocked by: 05

## Question

How does Kiminola reach users' machines? Decide with the user (one question at a time), grounded in the stack lock (ticket 05) and ticket 02's packaging findings:

- Installer formats per Windows arch (and whether MSIX is viable for OSS)
- Code signing: options and costs for an unfunded OSS project (e.g. SignPath.io for open source, self-signed with smartscreen friction, unsigned)
- Auto-update mechanism
- Store distribution (Microsoft Store? winget? GitHub Releases only?)
- CI build matrix for x86/x64/ARM64 (GitHub Actions ARM64 Windows runners availability)

Feeds the spec's distribution section.

## Resolution

Resolved on 2026-08-13 after a one-question-at-a-time grilling session. Note: ticket 05 dropped 32-bit x86, so the build matrix is x64 + ARM64 only.

### Decisions

- **Installer format — NSIS `.exe` primary, portable `.zip` secondary.** NSIS is Tauri's default Windows installer, lightweight, updater-friendly, and works for both x64 and ARM64. A portable `.zip` is also published for power users. MSI and MSIX are deferred to post-MVP (Store/enterprise scenarios).
- **Code signing — SignPath.io OSS program as the primary path.** It offers free code signing for open-source projects with public CI. If approval is delayed or denied, fall back to self-signed (SmartScreen reputation build-up) or even unsigned early releases.
- **Auto-update — Tauri built-in updater with GitHub Releases-hosted JSON manifest.** The app checks for updates, downloads the new signed installer, and installs it. Pairs naturally with the NSIS installer.
- **Distribution channels — GitHub Releases primary, winget secondary.** Microsoft Store is a post-MVP option because it requires MSIX packaging and review.
- **CI build matrix — x64 native on GitHub Actions `windows-latest`; ARM64 cross-compiled from the same x64 runner targeting `aarch64-pc-windows-msvc`.** Prebuilt ARM64 native dependencies (sherpa-onnx, ONNX Runtime) are linked at build time. If cross-compilation proves unreliable with the native ASR stack, fall back to manual or self-hosted ARM64 builds on the Snapdragon X Elite validation machine. Use GitHub-hosted Windows ARM64 runners if/when they become freely available for OSS.

### Consequences

- The first release can be unsigned/self-signed if SignPath approval is pending; the updater still works, but users will see SmartScreen friction until signing is in place.
- winget distribution points directly at the GitHub Releases NSIS installer, so it adds almost no release overhead.
- The "Update mechanism & release cadence" fog patch is now fully specified.
