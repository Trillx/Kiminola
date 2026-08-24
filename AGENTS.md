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
- Frontend: SvelteKit (adapter-static) + TypeScript + **Tailwind CSS v4**. UI components come from **shadcn-svelte** ("nova" style) and live in `kiminola/src/lib/components/ui/`. The Oatwave brand identity — tokens, type, motion, and hard rules (one gold element per view, no gradients, no off-palette hues) — is defined in `kiminola/branding/HANDOFF.md` (Rev A); shadcn theme variables are mapped to those tokens in `kiminola/src/app.css`. Layout and interaction patterns still follow `.scratch/kiminola/prototypes/core-ui-loop/blend.html`. Display name is "Kimi Nola"; fonts (Gentium Book Plus, Archivo, IBM Plex Mono) are self-hosted in `kiminola/static/fonts/`.
- Backend: Rust. ASR via sherpa-onnx C API (in-process); audio via windows-rs WASAPI loopback + cpal mic; rubato resampling; Silero VAD via ort; sqlx + SQLite (bundled) with migrations; LLM via a narrow ChatProvider seam (OpenAI-compatible, streaming SSE); keys via `keyring`.
- MVP scope is fixed in SPEC.md §8 — don't add features outside it without a new decision.
- No usage analytics ever; crash reporting is opt-in only.

## Agent skills

### Issue tracker

Issues live in GitHub Issues and are managed with the `gh` CLI; `SPEC.md` remains the product/spec source of truth. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the default five canonical triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repository; read `CONTEXT.md` and relevant ADRs before exploring. See `docs/agents/domain.md`.
