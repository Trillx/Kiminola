# Releasing Kimi Nola for Windows

Kimi Nola ships signed NSIS installers for Windows x64 and ARM64. The in-app
updater checks the published stable GitHub Release feed and installs only a
higher signed version. Drafts and prereleases are never offered in the app.

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

## Release flow

1. Set the same version in `kiminola/package.json`,
   `kiminola/src-tauri/Cargo.toml`, and
   `kiminola/src-tauri/tauri.conf.json`.
2. Merge the version change to `main`, then push the matching tag:

   ```powershell
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

3. The tag workflow creates one draft GitHub Release. The x64 and ARM64 jobs
   build in parallel and upload their NSIS installer plus `.sig` file.
4. A final serialized job reads both signatures and uploads one `latest.json`
   manifest. If either architecture or signature is missing, the workflow
   fails and the release remains a draft.
5. Test the draft installers on matching Windows architectures. For an
   update-enabled baseline, install the previous published version, create a
   small meeting/data marker, and accept the update from inside the app. Check
   that the app restarts once and that the SQLite database and downloaded model
   files remain intact.
6. Open the draft, confirm both architectures and `latest.json`, and select
   **Publish release**. Keep it non-prerelease; this is what makes the stable
   `releases/latest/download/latest.json` endpoint resolve to it.
7. After publication, verify the public manifest and both installer URLs. The
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

Reference documentation: [Tauri updater](https://v2.tauri.app/plugin/updater/),
[Tauri GitHub pipeline](https://v2.tauri.app/distribute/pipelines/github/), and
[GitHub releases](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository).
