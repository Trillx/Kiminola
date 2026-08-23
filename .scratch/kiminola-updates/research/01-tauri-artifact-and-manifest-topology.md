# Research 01 — Tauri artifact and manifest topology

Date researched: 2026-08-23. Sources are limited to the current Kiminola checkout and first-party Tauri / tauri-action documentation and source.

## Conclusion

Use Tauri 2's modern updater artifacts (`createUpdaterArtifacts: true`) with the existing NSIS-only target. The Windows updater payloads are the two architecture-specific NSIS setup executables and their `.sig` files. Do not let either parallel matrix job create or update `latest.json`. Create the draft release once, upload the x64 and ARM64 assets in parallel, and publish one manifest from a serialized job only after both asset pairs are present.

The current workflow already sets `uploadUpdaterJson: false`, which is the correct setting for the parallel asset jobs. It does not yet serialize draft-release creation, and the current Tauri config does not yet enable updater artifacts or configure the updater public key/endpoints. This is a research result only; no application or workflow code was changed.

## Current repository facts

- [`tauri.conf.json`](../../../kiminola/src-tauri/tauri.conf.json#L42-L51) currently enables bundling, restricts bundles to `"nsis"`, and has no `bundle.createUpdaterArtifacts` or `plugins.updater` block.
- [`package.json`](../../../kiminola/package.json#L19-L21) uses `@tauri-apps/plugin-updater` `^2.10.1`; [`Cargo.toml`](../../../kiminola/src-tauri/Cargo.toml#L21-L24) declares `tauri-plugin-updater = "2"`; and [`capabilities/default.json`](../../../kiminola/src-tauri/capabilities/default.json#L6-L10) already grants `updater:default`.
- [`src/lib.rs`](../../../kiminola/src-tauri/src/lib.rs#L21-L28) currently leaves `tauri_plugin_updater::Builder::new().build()` commented out, and the current workflow does not expose `TAURI_SIGNING_PRIVATE_KEY`; runtime plugin wiring and CI signing-secret setup therefore remain separate implementation work.
- [`release.yml`](../../../.github/workflows/release.yml#L26-L39) has a parallel `x86_64-pc-windows-msvc` / `aarch64-pc-windows-msvc` matrix. Each job currently calls `tauri-apps/tauri-action@v1` with the same tag and `releaseDraft: true` ([workflow lines 64-78](../../../.github/workflows/release.yml#L64-L78)), while explicitly setting `uploadUpdaterJson: false`.
- The repository remote is `Trillx/Kiminola`; the stable static endpoint, once configured, can therefore be `https://github.com/Trillx/Kiminola/releases/latest/download/latest.json`. Tauri's updater documentation explicitly shows the GitHub `releases/latest/download/latest.json` form for a static endpoint ([Tauri updater configuration](https://v2.tauri.app/plugin/updater/)).

## Exact Tauri 2 configuration

The required configuration shape is:

```json
{
  "bundle": {
    "targets": ["nsis"],
    "createUpdaterArtifacts": true
  },
  "plugins": {
    "updater": {
      "pubkey": "CONTENTS OF THE TAURI PUBLIC KEY",
      "endpoints": [
        "https://github.com/Trillx/Kiminola/releases/latest/download/latest.json"
      ]
    }
  }
}
```

`createUpdaterArtifacts` belongs under `bundle`, not under `plugins.updater`. For Tauri 2, `true` is the modern format; `"v1Compatible"` is only the migration format and produces zipped updater payloads. The updater `pubkey` is the public-key content, not a file path. `endpoints` is an array of URL strings; Tauri documents `{{target}}` and `{{arch}}` variables for dynamic endpoints and the static GitHub `latest.json` endpoint shown above ([Tauri updater configuration](https://v2.tauri.app/plugin/updater/)).

The private signing key is not configuration: it must be supplied to the build environment as `TAURI_SIGNING_PRIVATE_KEY` (and optionally `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Tauri says the public key is embedded in `tauri.conf.json`, the private key signs update artifacts, and the private key must not be shared ([Tauri updater signing/building](https://v2.tauri.app/plugin/updater/)).

## Windows NSIS artifact and signature names

For Kimi Nola's `productName` `"Kimi Nola"`, version `0.1.1`, and Tauri's architecture mapping (`x86_64` → `x64`; `aarch64` → `arm64`), the Tauri output names are:

| Target | Local Tauri updater artifact | Local signature | GitHub Release asset name under tauri-action@v1 |
|---|---|---|---|
| `x86_64-pc-windows-msvc` | `Kimi Nola_0.1.1_x64-setup.exe` | `Kimi Nola_0.1.1_x64-setup.exe.sig` | `Kimi.Nola_0.1.1_x64-setup.exe` and `.sig` |
| `aarch64-pc-windows-msvc` | `Kimi Nola_0.1.1_arm64-setup.exe` | `Kimi Nola_0.1.1_arm64-setup.exe.sig` | `Kimi.Nola_0.1.1_arm64-setup.exe` and `.sig` |

The Tauri bundler source maps `Arch::X86_64` to `x64` and `Arch::AArch64` to `arm64`, then constructs `<product>_<version>_<arch>-setup.exe` ([Tauri NSIS bundler source](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs#L159-L177), [same source's output naming](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs#L605-L620)). The modern Tauri 2 updater format reuses the setup executable and creates `<...>.exe.sig` ([Tauri updater artifact list](https://v2.tauri.app/plugin/updater/)). tauri-action's GitHub asset-name normalization replaces spaces and other non-URL-safe characters with dots, so `Kimi Nola` becomes `Kimi.Nola` in the uploaded asset name ([tauri-action v1 `ghAssetName`](https://raw.githubusercontent.com/tauri-apps/tauri-action/v1/src/utils.ts#L81-L139)).

Do not use the following v1-compatible names for this Tauri 2 configuration: `Kimi.Nola_0.1.1_x64-setup.nsis.zip` and `.sig`, or their ARM64 equivalents. Those are produced only by `createUpdaterArtifacts: "v1Compatible"` ([Tauri's v1-compatible Windows artifact list](https://v2.tauri.app/plugin/updater/)).

## `latest.json` shape and platform keys

The static manifest has `version`, optional `notes` and `pub_date`, and a `platforms` object. Each platform entry requires the literal signature content and a URL to the updater artifact; a signature path or URL is invalid ([Tauri static JSON schema](https://v2.tauri.app/plugin/updater/)).

For this NSIS-only release, the complete useful platform map is:

```json
{
  "version": "0.1.1",
  "notes": "...",
  "pub_date": "2026-08-23T00:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "contents of Kimi.Nola_0.1.1_x64-setup.exe.sig",
      "url": "URL for Kimi.Nola_0.1.1_x64-setup.exe"
    },
    "windows-aarch64": {
      "signature": "contents of Kimi.Nola_0.1.1_arm64-setup.exe.sig",
      "url": "URL for Kimi.Nola_0.1.1_arm64-setup.exe"
    },
    "windows-x86_64-nsis": {
      "signature": "contents of Kimi.Nola_0.1.1_x64-setup.exe.sig",
      "url": "URL for Kimi.Nola_0.1.1_x64-setup.exe"
    },
    "windows-aarch64-nsis": {
      "signature": "contents of Kimi.Nola_0.1.1_arm64-setup.exe.sig",
      "url": "URL for Kimi.Nola_0.1.1_arm64-setup.exe"
    }
  }
}
```

The first two keys are the documented `OS-ARCH` keys. Current tauri-action source also emits the installer-qualified `${os}-${arch}-${bundle}` keys; for `bundle: nsis`, those are `windows-x86_64-nsis` and `windows-aarch64-nsis`. tauri-action's release notes state that these additional keys support multiple installer formats and require `tauri-plugin-updater` 2.10.0 or newer; Kiminola's `^2.10.1` satisfies that requirement ([tauri-action v1 manifest source](https://raw.githubusercontent.com/tauri-apps/tauri-action/v1/src/upload-version-json.ts#L172-L254), [tauri-action release note](https://github.com/tauri-apps/tauri-action/releases#L476-L479)). Keep both key forms and make each architecture's two entries identical so older/default selection and installer-specific selection resolve to the same signed NSIS asset.

The current tauri-action v1 source builds the asset download URL from GitHub's release-asset API endpoint (`/repos/{owner}/{repo}/releases/assets/{asset_id}`) and stores the `.sig` file contents in the manifest ([tauri-action v1 manifest source](https://raw.githubusercontent.com/tauri-apps/tauri-action/v1/src/upload-version-json.ts#L82-L93), [signature/url mapping](https://raw.githubusercontent.com/tauri-apps/tauri-action/v1/src/upload-version-json.ts#L172-L254)). A custom serialized manifest generator may instead use the tag-pinned browser download URLs, for example `https://github.com/Trillx/Kiminola/releases/download/v0.1.1/Kimi.Nola_0.1.1_x64-setup.exe`; the important invariants are that the URL names the exact uploaded asset and the signature is the exact content of its matching `.sig` file.

## Safe upload topology for the current matrix

1. **Create the draft release once.** Add a single non-matrix job that creates or resolves the `vX.Y.Z` draft release and exposes its `releaseId`. The current matrix invokes the action with the same `tagName`, `releaseName`, and `releaseDraft: true`, so both jobs can race during release creation. The tauri-action issue tracker documents duplicate releases for this pattern ([tauri-action issue #914](https://github.com/tauri-apps/tauri-action/issues/914)).
2. **Build/upload assets in parallel.** Pass the one `releaseId` to the x64 and ARM64 jobs. Each job should build only its own NSIS bundle, upload the setup executable and its `.sig`, and keep `uploadUpdaterJson: false`. Ensure `TAURI_SIGNING_PRIVATE_KEY` is present so both signatures are generated; leave `uploadUpdaterSignatures` enabled (the action default).
3. **Generate/upload `latest.json` once, after both jobs.** A serialized `manifest` job should depend on both matrix results, list the draft release assets, require the four expected names, read both signature files, construct the four platform entries above, and replace `latest.json` exactly once. Keep the release draft while validating it; publish only after the asset/signature/manifest checks pass, matching the repository's existing release intent ([updates map](../map.md)).

Do not set `uploadUpdaterJson: true` in both matrix jobs. tauri-action's manifest implementation reads the existing `latest.json`, merges its local platform entries, deletes the existing release asset, and uploads a replacement ([tauri-action v1 manifest source](https://raw.githubusercontent.com/tauri-apps/tauri-action/v1/src/upload-version-json.ts#L54-L81), [delete/upload flow](https://raw.githubusercontent.com/tauri-apps/tauri-action/v1/src/upload-version-json.ts#L256-L281)). The official tauri-action issue for parallel builds records the exact failure: both jobs can observe the same asset ID, one deletes and recreates it, and the other then receives 404 deleting the stale ID; retries do not make this a reliable topology ([tauri-action issue #1270](https://github.com/tauri-apps/tauri-action/issues/1270)). Even when no 404 occurs, concurrent read/modify/write can leave a manifest missing one architecture. Serialization removes both races.

## Research boundary

The Research skill requested a background agent, but no background-agent/delegation tool was available in this session. The investigation was therefore performed directly using the same required primary-source method. No app code, configuration, workflow, issue, or map file was modified; only this findings file was added.

## Sources

- [Kiminola `tauri.conf.json`](../../../kiminola/src-tauri/tauri.conf.json)
- [Kiminola release workflow](../../../.github/workflows/release.yml)
- [Tauri 2 updater documentation](https://v2.tauri.app/plugin/updater/)
- [Tauri NSIS bundler source](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs)
- [tauri-action v1 README/documentation](https://github.com/tauri-apps/tauri-action/tree/v1)
- [tauri-action v1 manifest source](https://raw.githubusercontent.com/tauri-apps/tauri-action/v1/src/upload-version-json.ts)
- [tauri-action v1 asset naming source](https://raw.githubusercontent.com/tauri-apps/tauri-action/v1/src/utils.ts)
- [tauri-action issue #914: duplicate release race](https://github.com/tauri-apps/tauri-action/issues/914)
- [tauri-action issue #1270: parallel `latest.json` race](https://github.com/tauri-apps/tauri-action/issues/1270)
