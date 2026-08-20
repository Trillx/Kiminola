# Contributing to Kimi Nola

Thanks for helping build a private, local-first meeting-notes tool. Kimi Nola is Windows-first and currently targets Windows x64 and ARM64.

## Before you start

- Read [`SPEC.md`](./SPEC.md), the source of truth for product behavior and MVP scope.
- Read [`CONTEXT.md`](./CONTEXT.md) for domain terminology.
- For UI work, follow [`kiminola/branding/HANDOFF.md`](./kiminola/branding/HANDOFF.md) and the interaction patterns in `.scratch/kiminola/prototypes/core-ui-loop/blend.html`.
- Keep the privacy boundary intact: audio stays on the machine, transcript text leaves only when the user explicitly requests cloud enhancement, and usage analytics are never added.

## Development setup

The complete prerequisites and native sherpa-onnx setup are in the [root README](./README.md#build-from-source). The short version is:

```powershell
git clone https://github.com/Trillx/Kiminola.git
cd Kiminola\kiminola
npm install
```

For frontend-only work:

```powershell
npm run dev
```

For the full desktop app, set `SHERPA_ONNX_LIB_DIR` to the extracted sherpa-onnx package's `lib` directory first, then run:

```powershell
npm run tauri dev
```

## Useful commands

Run frontend commands from `kiminola/`:

```powershell
npm run check
npm run build
```

Run Rust commands from `kiminola/src-tauri/`:

```powershell
cargo check
cargo test
```

Build the Windows package from `kiminola/` with:

```powershell
npm run tauri build
```

## Before opening a pull request

- Run `npm run check`, `npm run build`, `cargo check`, and `cargo test`.
- For audio, ASR, model-management, or Tauri lifecycle changes, launch the app and perform a short end-to-end smoke test as well. Build success alone does not prove live capture or transcription.
- For UI changes, include a screenshot or short description of the changed interaction.
- Update `SPEC.md` when a product decision or MVP boundary changes.
- Keep pull requests focused and explain what was tested, including the Windows architecture used.

## Reporting bugs

Include the Windows version, x64 or ARM64 architecture, commit or release version, whether the issue occurs in frontend-only or desktop mode, reproduction steps, and relevant logs. For capture or transcription issues, say whether microphone capture, system-audio capture, model loading, and transcript updates each succeeded.

## Scope and design constraints

Do not add features outside the MVP without a decision recorded in `SPEC.md`. Do not add telemetry, audio retention, or plaintext credential storage. New visual surfaces should follow the Oatwave brand handoff: no gradients, no off-palette hues, and one gold emphasis per view.
