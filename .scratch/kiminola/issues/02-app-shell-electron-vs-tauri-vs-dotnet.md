# App shell: Electron vs Tauri vs .NET for native-ASR desktop app

Type: research
Status: resolved

## Question

Which desktop app shell best fits Kiminola — a cross-platform (Windows x86/x64/ARM64 first, macOS/Linux later) meeting-notes app whose heavy lifting (audio capture + streaming ASR inference) runs in native code?

Compare **Electron**, **Tauri**, and **.NET (Avalonia/WinUI)** on:

- **Windows ARM64 maturity**: official builds, packaging (NSIS/MSIX/MSI), code-signing story for OSS projects, auto-update solutions, winget publishability.
- **Native integration**: how each hosts/embeds a native ASR runtime (ONNX Runtime, sherpa-onnx, whisper.cpp) — in-process (N-API, Rust crate, P/Invoke) vs sidecar process; system audio loopback capture access from each on Windows (WASAPI), and the path to CoreAudio/PipeWire later.
- **Footprint**: installer size, RAM overhead — relevant on a 32 GB unified-memory ARM64 machine also running ASR.
- **Contributor pool & maintenance** for an MIT-licensed OSS project: TS-only (Electron) vs Rust+TS (Tauri) vs C# (.NET).
- **Streaming UI**: any constraints pushing live transcript tokens from native code to the UI at low latency.

Output: a recommendation with scores per criterion and links to evidence.

## Answer

**Recommendation: Tauri 2 (Rust core + TypeScript frontend).** Full findings, per-criterion scores (Tauri 36 / .NET 32 / Electron 31 unweighted), and dated sources: [research/02-app-shell-electron-vs-tauri-vs-dotnet.md](../research/02-app-shell-electron-vs-tauri-vs-dotnet.md).

- Kiminola's hard parts (WASAPI loopback, ONNX Runtime / sherpa-onnx / whisper.cpp inference, token streaming) live in native code under any shell; Tauri hosts them in-process via the strongest crate ecosystem (`windows-rs`/WASAPI, `ort`, `whisper-rs`, sherpa-onnx C API), with an official sidecar pattern as fallback.
- Windows ARM64 is solid: Rust `aarch64-pc-windows-msvc` has host tools (native dev on the X Elite), tauri-cli ships arm64 binaries, NSIS/MSI bundlers emit arm64 installers (~3-10 MB vs Electron's ~60-90 MB; ~5x less RAM), WebView2 ARM64 is preinstalled on Win11.
- Unfunded-OSS story is free end to end: SignPath Foundation code signing (MIT-eligible), tauri-plugin-updater with static JSON on GitHub Releases (minisign), winget accepts NSIS/MSI per-arch, Store registration is now free for individuals.
- Streaming: Tauri 2 Channels are the docs-blessed low-latency streaming path from Rust to the UI; events for lower-rate updates.
- Main costs accepted: two-language contributor pool (TS + Rust) vs Electron's TS-only, and a younger packaging ecosystem (no official MSIX bundler). Electron stays the documented fallback — the native engine ports to a sidecar unchanged, so the decision is partially reversible.
- Ship x64 + ARM64 only; x86 (32-bit) deferred unless demand appears.
