# Kimi Nola application

This directory contains the Tauri 2 desktop application: a SvelteKit and TypeScript frontend in `src/` with the Rust backend in `src-tauri/`.

See the [repository README](../README.md) for the product overview, privacy boundary, architecture, prerequisites, and complete build instructions.

## Common commands

Run these commands from this directory:

```powershell
npm install
npm run dev
npm run check
npm run build
npm run tauri dev
```

Rust validation runs from `src-tauri/`:

```powershell
cargo check
cargo test
```

On Windows ARM64, add `C:\Program Files\LLVM\bin` to `PATH` before running Cargo. The local sherpa-onnx shared-library path is configured in `src-tauri/.cargo/config.toml`.
