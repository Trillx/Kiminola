Type: task
Status: resolved

## Question

How is the Tauri updater signing identity provisioned without exposing the private key?

## Answer

A Tauri updater keypair was generated outside the repository. The public key is committed in `kiminola/src-tauri/tauri.conf.json`; the private key and password remain outside the repository and are not reproduced here. The release workflow requires these GitHub Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

The operational handoff is documented in [`docs/RELEASING.md`](../../../docs/RELEASING.md). Authenticode/SignPath signing remains a separate identity and release concern. Before the first signed tag run, a maintainer must confirm the two repository secrets exist and retain the offline key backup.
