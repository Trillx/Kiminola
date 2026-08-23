Type: research
Status: resolved

## Question

Which Tauri 2 artifact and manifest topology gives the parallel x64/ARM64 release one valid signed stable updater feed?

## Answer

Implemented in `kiminola/src-tauri/tauri.conf.json` and `.github/workflows/release.yml`:

- `bundle.createUpdaterArtifacts` is `true`, with the modern Tauri 2 NSIS setup executable plus `.sig` artifact for each architecture.
- The draft release is created once, then x64 and ARM64 assets upload in parallel through one `releaseId`.
- `kiminola/scripts/generate-updater-manifest.ps1` runs only after both matrix jobs, reads the literal `.sig` contents, and uploads one manifest with both documented and NSIS-qualified Windows platform keys.
- The app endpoint is the published stable `releases/latest/download/latest.json` URL. Drafts and prereleases remain excluded from the client.
