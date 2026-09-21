# Kimi Nola application

This directory contains the Tauri 2 desktop application: a SvelteKit and TypeScript frontend in `src/` with the Rust backend in `src-tauri/`.

See the [repository README](../README.md) for the product overview, privacy boundary, architecture, prerequisites, and complete build instructions.

## Common commands

Run these commands from this directory:

```powershell
npm install
npm run dev
npm run check
npm test
npm run test:ui
npm run build
npm run tauri dev
```

`npm test` runs the Node regression suite (Node 24 recommended). `npm run test:ui`
starts and stops its own local Vite server and runs isolated Playwright/axe checks
against synthetic Tauri IPC. It does not access native audio, Credential Manager,
or the user database. Chrome must be installed; set `PLAYWRIGHT_CHANNEL=msedge`
to use Edge instead. These checks complement, rather than replace, native tests.

Rust validation runs from `src-tauri/`:

```powershell
cargo check
cargo test
```

On Windows ARM64, add `C:\Program Files\LLVM\bin` to `PATH` before running Cargo. Set `SHERPA_ONNX_LIB_DIR` to the extracted package's `src-tauri\...\lib` directory before running Cargo or Tauri; the [repository README](../README.md#build-from-source) has the full setup.
