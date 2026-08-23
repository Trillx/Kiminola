Type: research
Status: resolved

## Question

What tag-to-draft-to-publish sequence and validation prove a stable Kimi Nola update release?

## Answer

The release flow is now tag -> one draft release -> parallel x64/ARM64 installer and signature uploads -> serialized `latest.json` generation -> human validation -> manual publication. The workflow verifies that `package.json`, `Cargo.toml`, and `tauri.conf.json` agree with `vX.Y.Z`.

The maintainer runbook is [`docs/RELEASING.md`](../../../docs/RELEASING.md). It requires installer launch and installed-update tests on both architectures, including preservation of a meeting/database marker and downloaded model files. Only a published non-prerelease release is allowed to feed the stable endpoint; a bad release is corrected with a higher patch version rather than an in-app downgrade.
