# Kimi Nola review rules

Treat `SPEC.md` as the source of truth for product behavior and MVP scope, `AGENTS.md` as the engineering guide, and `kiminola/branding/HANDOFF.md` as the visual-system contract.

## Blocking correctness and privacy rules

- Audio must remain on the local machine. Flag any path that uploads, persists, logs, or otherwise exports microphone or system audio.
- Transcript text and notes may leave the machine only after an explicit user request for AI enhancement through the configured provider.
- Never add usage analytics. Crash reporting must remain opt-in.
- Credentials must use Windows Credential Manager through the existing `keyring` integration; flag plaintext storage, logs, or committed secrets.
- Preserve Windows x64 and ARM64 support. Flag architecture assumptions, incompatible native artifacts, or packaging changes that support only one target.
- Check the complete Rust/Tauri-to-Svelte boundary when commands, events, payloads, or persisted models change. Call out mismatched names, types, nullability, serialization, or lifecycle assumptions.
- For SQLite changes, check migration safety, compatibility with existing user data, and behavior after closing and reopening the application.
- For audio, ASR, concurrency, and window-lifecycle code, prioritize resource cleanup, cancellation, deadlocks, race conditions, and behavior across pause, resume, stop, close, and relaunch.

## Scope and maintainability

- Do not recommend features outside the MVP boundary in `SPEC.md` as required fixes.
- Prefer existing modules and abstractions over parallel implementations. Flag duplicated business logic or native lifecycle behavior.
- Require regression tests for bug fixes when the affected behavior can be exercised deterministically.
- Distinguish blocking defects from optional cleanup or subjective style preferences. Avoid comments that merely restate the diff.
- For frontend changes, enforce the Oatwave constraints: no gradients, no off-palette colors, and one gold emphasis per view.

## Expected validation

Use the affected subset of these checks when evaluating a change:

- `npm run test`
- `npm run check`
- `npm run build`
- `cargo check` from `kiminola/src-tauri` with the documented LLVM and `SHERPA_ONNX_LIB_DIR` setup on Windows ARM64
