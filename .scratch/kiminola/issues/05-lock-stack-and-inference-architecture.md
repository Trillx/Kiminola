# Lock the stack and inference architecture

Type: grilling
Status: resolved
Blocked by: 01, 02, 04, 11

## Question

Decide Kiminola's technical spine: app shell (Electron / Tauri / .NET), where ASR inference lives (in-process native vs sidecar), the audio pipeline libraries, and the language(s) the repo will be written in. Resolve with the user in a /grilling session (one question at a time), grounded in the findings of tickets 01, 02, and 04 — and in ticket 11's hardware spike, which validates the two riskiest ARM64 assumptions first.

Also settle the Windows architecture matrix here: the charting requirement was x86/x64/ARM64, but ticket 02's research recommends shipping x64 + ARM64 only and dropping 32-bit x86 (extinct for new apps; unsupported by the chosen ASR runtimes). The user must confirm or override.

The output feeds directly into the build-ready spec — include: process architecture diagram (described), module boundaries, and the reasoning a future contributor would need.

## Resolution

Locked after a /grilling session on 2026-08-13.

### Decisions

- **App shell**: Tauri 2.
- **ASR inference**: In-process Rust, calling sherpa-onnx through its C API / Rust bindings. Chosen for lowest streaming latency and a single installable binary.
- **Windows audio pipeline**:
  - System loopback: custom `windows-rs` WASAPI module using Process Loopback Capture (Windows 10 build 20348+) with classic render-endpoint loopback fallback.
  - Mic: `cpal` (keeps the mic path cross-platform).
  - Resampling/alignment: `rubato` → 16 kHz mono; dual ring buffers; QPC-based timestamps; synthesize silence when loopback idles.
  - VAD: Silero VAD via `ort`, with an energy-based fallback.
- **Languages**: Rust for the native/audio/ASR/LLM backend; TypeScript + Svelte for the Tauri frontend.
- **Windows architecture matrix**: Ship x64 + ARM64 only; 32-bit x86 is out of scope.

### Architecture sketch

```
┌─ Tauri 2 app (x64 / ARM64) ──────────────────────────────────────┐
│  Frontend (TypeScript + Svelte)                                   │
│    ├── Idle / meeting-list view                                   │
│    ├── Recording view: live transcript ("You" / "Others")         │
│    │   └── optional notepad                                       │
│    └── Post-meeting view: enhanced notes + raw transcript         │
│                                                                   │
│  Rust core (in-process)                                           │
│    ├── windows-rs WASAPI process loopback ──┐                     │
│    ├── cpal mic capture                  ───┼→ ring buffers       │
│    │                                        │   QPC-aligned       │
│    ├── rubato → 16 kHz mono                 │                     │
│    ├── Silero VAD (ort)                     │                     │
│    └── sherpa-onnx streaming ASR per lane   │                     │
│         → "You" / "Others" segments → Tauri Channels → frontend   │
│                                                                   │
│  BYOK LLM enhancement (OpenAI-compatible provider seam)           │
└───────────────────────────────────────────────────────────────────┘
```

### Reasoning for future contributors

Tauri 2 was chosen because Kiminola's hardest problems — WASAPI loopback capture, ONNX-class ASR inference, and streaming token push — all live in native code. Tauri hosts that native code in-process in Rust, which has the strongest crate ecosystem for this exact work (`windows-rs`, `cpal`, `rubato`, `ort`, sherpa-onnx FFI) while keeping installer size and RAM use minimal on ARM64. Electron would add ~10x installer size and 3–5x RAM for no gain on the difficult parts; .NET Avalonia is technically solid but has a smaller niche community for desktop OSS audio/ML.

ASR runs in-process rather than as a sidecar because streaming latency is critical to the user experience: capture thread → inference thread → Tauri Channel → UI with no IPC boundary.

x86 (32-bit) was dropped because it is effectively extinct for new desktop apps, the chosen ASR runtimes do not prioritize it, and limiting the matrix to x64 + ARM64 simplifies CI and packaging. ARM64 is a first-class target because the primary dev/test machine is a Snapdragon X Elite Copilot+ PC.
