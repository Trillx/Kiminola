# Kiminola

Open-source, Windows-first (x64 + ARM64) Granola alternative: local meeting transcription (on-device ASR) + optional AI note enhancement (BYOK cloud LLM). Privacy line: audio never leaves the machine.

## Layout

- `SPEC.md` — the build-ready spec; the single source of truth for product decisions. Update it when decisions change.
- `CONTEXT.md` — domain glossary.
- `.scratch/kiminola/` — wayfinder planning map (`map.md`) and resolved decision tickets (`issues/`). Read-only history; all tickets are closed.
- `.scratch/kiminola/prototypes/core-ui-loop/blend.html` — the locked UI direction prototype; match its look and interaction patterns.
- `kiminola/` — the Tauri 2 app (SvelteKit + TypeScript frontend in `src/`, Rust backend in `src-tauri/`).

## Commands

All run from `kiminola/`:

- `npm install` — install frontend deps
- `npm run dev` — Vite dev server only (frontend)
- `npm run tauri dev` — run the desktop app with hot reload
- `npm run build` — production frontend build (outputs to `kiminola/build/`)
- `cd src-tauri && cargo check` — type-check the Rust backend
- `npm run tauri build` — produce the NSIS installer + portable binary

## Conventions

- Windows x64 + ARM64 only (32-bit x86 dropped). ARM64 target: `aarch64-pc-windows-msvc`; validated on a Snapdragon X Elite machine.
- **Toolchain note (ARM64 Windows)**: `ring` (via tauri-plugin-updater) hardcodes `clang` for compilation, and bindgen will need libclang for sherpa-onnx FFI. LLVM is installed at `C:\Program Files\LLVM` but NOT on PATH. Prefix cargo commands with `export PATH="/c/Program Files/LLVM/bin:$PATH" &&` (Git Bash) or add LLVM to the user PATH. `kiminola/src-tauri/.cargo/config.toml` sets CC/CXX/LIBCLANG_PATH for crates that respect them.
- Frontend: SvelteKit (adapter-static) + TypeScript. UI follows the Granola-calm × Wispr-Flow-fluid blend in the prototype: pill buttons, warm amber accent on deep charcoal in dark mode, light stationery feel in light mode.
- Backend: Rust. ASR via sherpa-onnx C API (in-process); audio via windows-rs WASAPI loopback + cpal mic; rubato resampling; Silero VAD via ort; sqlx + SQLite (bundled) with migrations; LLM via a narrow ChatProvider seam (OpenAI-compatible, streaming SSE); keys via `keyring`.
- MVP scope is fixed in SPEC.md §8 — don't add features outside it without a new decision.
- No usage analytics ever; crash reporting is opt-in only.
