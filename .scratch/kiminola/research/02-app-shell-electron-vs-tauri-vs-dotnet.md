# Research 02 — App shell: Electron vs Tauri vs .NET for a native-ASR desktop app

- **Ticket**: `issues/02-app-shell-electron-vs-tauri-vs-dotnet.md`
- **Date**: 2026-08-12
- **Question**: Which shell (Electron / Tauri / .NET Avalonia-or-WinUI) best fits Kiminola: Windows-first (x86/x64/ARM64), macOS/Linux later, heavy lifting (WASAPI loopback + mic capture, streaming ONNX-Runtime-class ASR) in native code, cloud LLM for note enhancement?

## TL;DR

**Recommendation: Tauri 2 (Rust core + TypeScript frontend).** The deciding insight is that Kiminola's hard problems — WASAPI loopback capture, ONNX Runtime / sherpa-onnx / whisper.cpp inference, streaming token push — all live in native code no matter which shell is picked. Tauri puts that native code in-process in Rust (the strongest crate ecosystem for exactly this: `windows-rs`/WASAPI, `ort`, `whisper-rs`, sherpa-onnx bindings) and adds the smallest installer/RAM footprint, first-class Windows ARM64 NSIS/MSI packaging, a free updater story, and free OSS code signing via SignPath. Electron is the safe runner-up (biggest contributor pool, most mature updater) but pays ~10x installer size and 3-5x RAM for zero gain on the parts that are actually hard. .NET (Avalonia) is technically excellent on Windows ARM64 and P/Invoke, but its desktop-OSS contributor pool in the AI-audio niche is the smallest and its macOS/Linux story is the least proven for this app class. Every comparable OSS project in this exact niche (Handy, Whispering, Meetily, Vibe) chose Tauri.

## Scored comparison (1 = poor, 5 = excellent)

| Criterion | Electron | Tauri 2 | .NET (Avalonia / WinUI 3) |
|---|---|---|---|
| Windows ARM64 build & packaging maturity | 5 | 4 | 5 (Avalonia) / 4 (WinUI unpackaged) |
| Installer + auto-update + code signing (unfunded OSS) | 5 | 4 | 3 |
| winget / Microsoft Store publishability | 5 | 5 | 5 |
| Native integration (in-process vs sidecar) | 3 | 5 | 4 |
| WASAPI loopback capture access | 2 | 5 | 5 |
| Footprint (installer size, RAM) | 2 | 5 | 3 |
| Contributor pool & maintenance (MIT OSS) | 5 | 3 | 3 |
| Streaming transcript → UI latency | 4 | 5 | 4 |
| **Total (unweighted)** | **31** | **36** | **32** |

Weights matter: for this project the differentiating criteria are native integration, WASAPI access, and ARM64-on-Snapdragon footprint — Tauri wins all three decisively. Electron's wins (contributor pool, update tooling maturity) are real but not decisive.

---

## 1. Windows ARM64 build/packaging maturity

**Electron — 5/5.** Official win/arm64 binaries since Electron 6.0.8 (2019); `electron-builder` produces NSIS (default), MSI, MSIX/AppX, portable and zip for `x64, ia32, arm64` targets ([electron-builder NSIS docs](https://www.electron.build/docs/nsis/), [electron-builder CLI](https://www.electron.build/docs/cli)). Arm publishes an official learning path for Electron on Windows on Arm ([Arm Learning Paths](https://learn.arm.com/learning-paths/laptops-and-desktops/electron/how-to-2/)). Historical rough edges (universal x64+arm64 NSIS broken, electron-userland/electron-builder#5461; arm64 MSI via WiX needed electron-wix-msi 5.1.3, issue #150) are resolved in current versions. Most mature of the three.

**Tauri 2 — 4/5.** Rust's `aarch64-pc-windows-msvc` is a Tier-2 target **with host tools** ([rustc target spec](https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_target/spec/targets/aarch64_pc_windows_msvc.rs.html)) — you can develop and build natively on the Snapdragon X Elite, and cross-compile x64 from it (`rustup target add x86_64-pc-windows-msvc`). tauri-cli ships prebuilt Windows ARM64 binaries since 1.4.0 ([tauri-cli 1.4.0 release notes](https://tauri.app/release/tauri-cli/all-versions/)). The bundler produces NSIS `.exe` and WiX MSI per architecture; real projects ship `_arm64-setup.exe` NSIS installers today (e.g. [pitchprompter-ai RELEASE.md, 2026-06](https://github.com/amit1858/pitchprompter-ai/blob/main/RELEASE.md), [PmuSim releases](https://github.com/Karl-Dai/PmuSim)). WebView2 Evergreen — Tauri's renderer — has a native ARM64 build and is preinstalled on Windows 11 (and serviced by Microsoft on Win10). Deduction: no MSIX bundler (Store submission requires wrapping or a winget-only strategy), and the ecosystem is younger — expect occasional per-arch bundler papercuts (e.g. [Handy's Windows ARM64 CI fixes, 2026-03](https://aidirtylist.info/repositories/cjpais/Handy/1/)).

**.NET — 5/5 (Avalonia) / 4/5 (WinUI 3).** `win-arm64` is a first-class RID; `dotnet publish -r win-arm64 --self-contained` cross-compiles from any host, and NativeAOT + single-file is officially documented for Avalonia ([Avalonia Native AOT docs](https://docs.avaloniaui.net/docs/deployment/native-aot), [supported platforms](https://docs.avaloniaui.net/docs/supported-platforms)). Windows App SDK / WinUI 3 supports x86/x64/ARM64 (arch-specific builds required, no AnyCPU — [WinAppSDK 0.8 notes](https://learn.microsoft.com/ja-jp/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-0.8)) and ships unpackaged (non-MSIX) on ARM64 ([Distribute an unpackaged WinUI 3 app](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/unpackage-winui-app)). WinUI 3 is Windows-only, though — "cross-platform .NET" means Avalonia, so WinUI's strengths only count if we abandon macOS/Linux.

## 2. Installer + auto-update + code signing for unfunded OSS

**Code signing (shell-independent, so it mostly cancels out):** [SignPath Foundation](https://signpath.org/) provides **free OV code signing for qualifying OSS projects** (MIT license qualifies; approval ~1-4 weeks, GitHub-Actions-integrated, HSM-held keys) — Microsoft's own docs list it as the OSS option ([Microsoft Learn: code signing options, 2026-04](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)). Paid fallback: Azure Artifact Signing at $9.99/mo/5,000 signatures ([Azure pricing](https://azure.microsoft.com/en-us/products/artifact-signing)), but note Public Trust is limited to US/Canada/EU/UK orgs and US/Canada individuals ([Zenn writeup, 2026-02](https://zenn.dev/shm_7ec/articles/signpath-oss-code-signing?locale=en)). EV certs no longer bypass SmartScreen (removed 2024), so OV + reputation-building is the path regardless. Unsigned interim releases are normal in this niche ("installers are not signed at the moment… security warning" — [Skiff README](https://github.com/DrizzleTime/Skiff)).

**Electron — 5/5.** `electron-updater` + electron-builder NSIS gives differential-ish auto-update from plain GitHub Releases with two lines of code ([electron-updater](https://www.npmjs.com/package/electron-updater)); signed-update verification on Windows requires the code-signing cert above. The most battle-tested OSS update pipeline of the three.

**Tauri — 4/5.** `tauri-plugin-updater` is official: static JSON manifest (e.g. on GitHub Releases) + minisign signature verification built in, no code-signing cert required *for update integrity* ([Tauri updater plugin](https://v2.tauri.app/plugin/updater/)). NSIS installer handles per-user/per-machine install modes. Deduction: updater key management mistakes have bitten real projects ([TimesFlow 0.1.72: regenerated updater key broke auto-update for four versions](https://www.timesflow.app/download)), and it's younger than electron-updater.

**.NET — 3/5.** No single dominant answer: Velopack (Squirrel successor), NetSparkle, or MSIX + Store/App Installer. All workable, none as turnkey-from-GitHub-Releases as the other two, and MSIX *requires* a signing cert to install at all — a hard gate for an unfunded project's day-one releases.

## 3. winget / Microsoft Store publishability

All three are publishable; differences are minor.

- **winget**: the community repo ([microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs)) accepts MSIX, MSI, APPX, or **exe** installers (NSIS included) that run unattended, per-architecture entries in the manifest ([installer schema 1.12](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md), [submission docs](https://learn.microsoft.com/en-us/windows/package-manager/package/repository)). Code signing is **not** a hard requirement (SignatureSha256 is optional), though SmartScreen reputation still applies at install time. Electron NSIS, Tauri NSIS/MSI, and .NET MSIX/MSI/exe all qualify → **5/5 each**.
- **Microsoft Store**: individual developer registration is now **free** ($19 fee waived, [Windows Blog, 2025-09-10](https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/), [Microsoft Learn](https://learn.microsoft.com/en-us/windows/apps/publish/whats-new-individual-developer)). Store submission also signs your MSIX with Microsoft's cert — i.e. the Store path sidesteps the code-signing cost entirely. Store accepts unpackaged Win32 apps too, but MSIX is smoothest; .NET produces MSIX natively, Electron via electron-builder's appx target, Tauri needs extra packaging work (no official MSIX bundler). Store edge to .NET/Electron; winget parity for all.

## 4. Native integration: hosting the ASR runtime

**Tauri — 5/5.** Two clean patterns, both first-class:
1. **In-process Rust crates** — this is the sweet spot. `ort` (ONNX Runtime bindings, [crates.io/crates/ort](https://crates.io/crates/ort)), `whisper-rs` (whisper.cpp bindings — used by [tauri-plugin-stt](https://crates.io/crates/tauri-plugin-stt)), sherpa-onnx via its C API / community `sherpa-rs`, and `wasapi`/`cpal`/`windows-rs` for capture. The ASR engine compiles *into* the app binary; no IPC, no ABI pinning, per-arch builds handled by cargo.
2. **Sidecar** — official `externalBin` pattern bundles any prebuilt exe (sherpa-onnx binary, whisper.cpp CLI) with target-triple naming ([Tauri sidecar docs](https://v2.tauri.app/develop/sidecar/)); communicate over stdin/stdout or localhost TCP ([Evil Martians sidecar guide](https://evilmartians.com/chronicles/making-desktop-apps-with-revved-up-potential-rust-tauri-sidecar)). Good fallback if a runtime lacks Rust bindings.

**Electron — 3/5.** N-API addons work and onnxruntime-node ships **official prebuilt binaries including Windows arm64** ([ONNX Runtime Node.js binding docs](https://onnxruntime.ai/docs/get-started/with-javascript/node.html), [npm onnxruntime-node](https://www.npmjs.com/package/onnxruntime-node)); sherpa-onnx also publishes Node bindings. But N-API means maintaining a native-addon build matrix across Electron ABI versions × 3 architectures, `@electron/rebuild` in CI, and packaged-app `asar` unpacking quirks ([SO: onnxruntime-node in packaged Electron](https://stackoverflow.com/questions/76256928/onnxruntime-node-in-packaged-electron-app)). Many projects end up with a **sidecar process** anyway — at which point Electron is just an expensive webview around the same architecture Tauri gives you natively.

**.NET — 4/5.** P/Invoke against sherpa-onnx's C API or whisper.cpp DLLs is trivial, and sherpa-onnx ships **official C# bindings with a `win-arm64` runtime NuGet** ([org.k2fsa.sherpa.onnx.runtime.win-arm64](https://www.nuget.org/packages/org.k2fsa.sherpa.onnx.runtime.win-arm64/1.12.4)) plus NAudio for WASAPI. Deduction: NativeAOT complicates some interop/reflection scenarios, and you give up the Rust audio-ML crate ecosystem momentum.

## 5. WASAPI loopback (system audio) capture

This is where the shells genuinely differ, because Kiminola needs **raw, unprocessed loopback audio as a separate channel** from the mic.

- **Electron — 2/5.** Chromium's only built-in route is `getDisplayMedia`/`desktopCapturer` with audio — on Windows it can capture system audio, but (a) it's bound to the screen-share picker UX and a video track you must open and discard, (b) the audio passes through Chromium's processing pipeline (echo cancellation/NS defaults aimed at conferencing), and (c) system-audio-alone (`video:false, audio:true`) is still an open W3C request ([w3c/mediacapture-screen-share#331](https://github.com/w3c/mediacapture-screen-share/issues/331)). Community shims exist ([electron-audio-loopback](https://npmjs.com/package/electron-audio-loopback)) but are exactly the "native addon" complexity Electron was supposed to avoid. Clean WASAPI loopback ([Microsoft: Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)) means writing native code regardless.
- **Tauri — 5/5.** Direct in-process access via `windows-rs` WASAPI bindings or the safe `wasapi` crate ([wasapi crate docs](http://www.camilladsp.com/wasapi-rs/docs/0.4.0/wasapi/index.html)); `cpal` covers mic capture cross-platform (loopback needs the wasapi path — [cpal#251](https://github.com/tomaka/cpal/issues/251)). No permission prompt needed for loopback on Windows. Later ports map cleanly: CoreAudio taps (macOS 14.4+) / PipeWire monitor sources (Linux) — same Rust code structure, `cfg`-gated backends. This is exactly the architecture [Hush](https://github.com/khawkins98/Hush/issues/107) and Handy use.
- **.NET — 5/5.** NAudio's `WasapiLoopbackCapture` is the canonical .NET API and works on ARM64; CSCore alternative. Equally good as Rust here.

## 6. Footprint (installer size / RAM)

| | Electron | Tauri | .NET (Avalonia, self-contained) |
|---|---|---|---|
| Installer (hello-world class) | ~60–90 MB (NSIS, bundles Chromium+Node) | ~3–10 MB NSIS (measured 2.26 MB: [pitchprompter-ai](https://github.com/amit1858/pitchprompter-ai/blob/main/RELEASE.md)); WebView2 already on Win11 | ~30–90 MB single-file (trimmed/AOT at the low end); framework-dependent ~few MB but requires .NET runtime install |
| Idle RAM | ~300–600 MB (Chromium + Node main) | ~60–150 MB (shared WebView2 + small Rust core) | ~100–250 MB (managed runtime; NativeAOT lower) |

Sources: Hopp's engineering benchmark writeup ([Tauri vs Electron trade-offs](https://tool.lu/index.php/en_US/article/78K/preview)), [Rustify comparison 2026-04](https://rustify.rs/articles/rust-tauri-vs-electron-2026) ("20-50x smaller, ~5x less RAM"). On the owner's 32 GB Snapdragon X Elite the ASR model + runtime will eat several GB; the shell should not compete with it, and ARM64 Chromium under any memory pressure is the worst citizen of the three. Scores: Electron 2, Tauri 5, .NET 3.

Note: the ASR engine itself (ONNX Runtime DLLs + model files, hundreds of MB downloaded at runtime) dominates total footprint and is identical across shells — this criterion is only about the shell's own overhead.

## 7. Contributor pool & maintenance for MIT OSS

Usage share (Stack Overflow 2025 survey, via [aggregated stats](https://zenn.dev/inuinu/articles/where-is-csharp-used-2026?locale=en)): JavaScript ~66%, TypeScript ~44%, C# ~28%, Rust ~15% (but most-admired ~9 years running, fastest-growing in systems/audio/AI tooling).

- **Electron — 5/5.** Pure TS end-to-end; the largest possible contributor funnel; trivial `npm install && npm run dev`. But: the ASR engine work is native C++/C anyway, so "TS-only" is an illusion for the parts that differentiate Kiminola — N-API addon maintainers are scarcer than Rust crate maintainers in this niche.
- **Tauri — 3/5.** Two languages (TS frontend + Rust backend). In practice the split is clean: UI contributors touch only TS, engine contributors touch only Rust — and the Rust audio/ML community (cpal, whisper-rs, ort, sherpa) is precisely the talent pool this project needs. Empirical evidence: the thriving OSS projects in this exact space — Handy, Whispering, Meetily, Vibe — are all Tauri and attract contributors. Still, the median drive-by contributor knows TS, not Rust.
- **.NET — 3/5.** C# has a huge pool globally, but it's concentrated in enterprise/LOB; the desktop-OSS + AI-audio corner is thin, and Avalonia knowledge is rarer still. WinUI 3 would narrow it further.

## 8. Streaming transcript tokens native → UI at low latency

Transcript deltas are small (tens of bytes, a few per second) — no shell is truly bottlenecked here, but the plumbing differs:

- **Tauri — 5/5.** Two official paths ([Tauri: Calling the Frontend from Rust](https://v2.tauri.app/develop/calling-frontend/)): the **event system** (fire-and-forget JSON broadcasts; docs note it is *not* designed for low latency/high throughput) and **Channels** (added in Tauri 2, "the implementation optimized for streaming data" — ordered, fast, single-consumer). Partial-result tokens → Channel; segment-finalized + speaker-label updates → events. No process boundary at all when the engine is an in-process crate: capture thread → inference thread → `channel.send()` → Svelte/React store.
- **Electron — 4/5.** N-API addon → main process → `webContents.send` IPC → renderer. Structured-clone serialization per message; fine for text tokens, measurably worse if you later stream waveforms/spectrograms for UI (needs transferable buffers / shared memory workarounds). Sidecar adds stdout pipe parsing jitter.
- **.NET — 4/5.** In-process callbacks → MVVM binding; no serialization at all. Fastest in theory; deducted only because high-frequency UI updates on Avalonia's dispatcher need the usual batching discipline.

---

## Recommended architecture (Tauri)

```
┌─ Tauri app (single binary, per-arch: x64 / x86? / ARM64) ─────────┐
│  Rust core (in-process):                                          │
│    windows-rs WASAPI loopback  ──┐                                │
│    cpal mic capture            ──┼→ ring buffers → streaming ASR  │
│                                  │   (sherpa-onnx C API / ort /   │
│                                  │    whisper-rs, per-arch DLLs)  │
│  Tauri Channels → transcript partials ─┐                          │
│  Tauri events   → segment updates   ───┤                          │
│  WebView2 frontend (TS/Svelte or React): live transcript, notes,  │
│  BYOK cloud-LLM calls from frontend or Rust (provider-pluggable)  │
└───────────────────────────────────────────────────────────────────┘
Packaging: NSIS (per-user default) + MSI; updater: tauri-plugin-updater
with static JSON on GitHub Releases (minisign); signing: SignPath
Foundation (free, MIT-eligible); distribution: GitHub Releases + winget
manifest PR (+ optional free Store listing via MSIX wrap later).
```

x86 (32-bit) note: Windows ARM64 machines emulate x86, but there is no reason to ship a 32-bit build of a model-inference app; ship x64 + ARM64 and treat x86 as out of scope unless demand appears (all three shells *can* build ia32 — Electron most easily).

## Risks & open questions

- **Tauri ecosystem youth**: no official MSIX bundler (Store needs wrapping or winget-only); occasional per-arch bundler/CI papercuts (observed in Handy, pitchprompter). Mitigation: build matrix in GitHub Actions early; owner has native ARM64 hardware to test on — a real advantage.
- **Two-language contributor tax** is the main strategic bet: if the project later wants maximum drive-by contributions, Electron remains the fallback; the native engine (Rust crates + FFI) ports to a sidecar unchanged, so the shell decision is partially reversible.
- **WebView2 dependency**: preinstalled on Win11/ARM64 and auto-serviced; evergreen bootstrapper covers the long tail. Low risk.
- **Updater key management** (minisign keys in CI secrets) has burned real projects — document key backup in the release runbook.
- **SmartScreen reputation** is unavoidable for any new unsigned/OV-signed app; plan Store listing (free, Microsoft-signed MSIX) as the reputation shortcut.
- Deferred to other tickets: macOS CoreAudio tap path (14.4+), Linux PipeWire monitor sources, telemetry stance, and update cadence all hang off this shell decision but don't change it.

## Sources

Primary/official:
- [Tauri 2 sidecar (externalBin) docs](https://v2.tauri.app/develop/sidecar/) · [Tauri updater plugin](https://v2.tauri.app/plugin/updater/) (2025-11) · [Tauri: Calling the Frontend from Rust](https://v2.tauri.app/develop/calling-frontend/) (events vs Channels)
- [tauri-cli 1.4.0 release notes — prebuilt Windows ARM64 CLI](https://tauri.app/release/tauri-cli/all-versions/)
- [Rust `aarch64-pc-windows-msvc` target spec (Tier 2 + host tools)](https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_target/spec/targets/aarch64_pc_windows_msvc.rs.html)
- [electron-builder NSIS target docs](https://www.electron.build/docs/nsis/) · [electron-builder CLI (win arm64)](https://www.electron.build/docs/cli) · [electron-updater](https://www.npmjs.com/package/electron-updater) (2026-06)
- [Arm learning path: Electron on Windows on Arm](https://learn.arm.com/learning-paths/laptops-and-desktops/electron/how-to-2/)
- [ONNX Runtime Node.js binding — supported platforms incl. Windows arm64](https://onnxruntime.ai/docs/get-started/with-javascript/node.html) · [npm onnxruntime-node](https://www.npmjs.com/package/onnxruntime-node)
- [sherpa-onnx `win-arm64` runtime NuGet](https://www.nuget.org/packages/org.k2fsa.sherpa.onnx.runtime.win-arm64/1.12.4) · [ort crate](https://crates.io/crates/ort) · [tauri-plugin-stt (whisper-rs)](https://crates.io/crates/tauri-plugin-stt) (2026-07)
- [Microsoft Learn: WASAPI Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording) · [wasapi crate](http://www.camilladsp.com/wasapi-rs/docs/0.4.0/wasapi/index.html) · [cpal WASAPI loopback issue #251](https://github.com/tomaka/cpal/issues/251)
- [W3C: system-audio-only getDisplayMedia request #331](https://github.com/w3c/mediacapture-screen-share/issues/331) · [electron-audio-loopback shim](https://npmjs.com/package/electron-audio-loopback)
- [Avalonia supported platforms](https://docs.avaloniaui.net/docs/supported-platforms) · [Avalonia Native AOT](https://docs.avaloniaui.net/docs/deployment/native-aot)
- [Microsoft Learn: distribute unpackaged WinUI 3 app](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/unpackage-winui-app) (2026-05)
- [Microsoft Learn: code signing options (SignPath for OSS)](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) (2026-04) · [SignPath Foundation](https://signpath.org/) · [Azure Artifact Signing pricing](https://azure.microsoft.com/en-us/products/artifact-signing)
- [Windows Blog: free Store registration for individuals](https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/) (2025-09)
- [winget-pkgs submission docs](https://learn.microsoft.com/en-us/windows/package-manager/package/repository) · [winget installer manifest schema](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md)

Field evidence / comparables:
- [pitchprompter-ai RELEASE.md — Tauri ARM64 NSIS, 2.26 MB installer](https://github.com/amit1858/pitchprompter-ai/blob/main/RELEASE.md) (2026-06)
- [PmuSim — Tauri x64+ARM64 NSIS/MSI with minisign updater](https://github.com/Karl-Dai/PmuSim)
- [Handy (Tauri speech-to-text) — Windows ARM64 CI history](https://aidirtylist.info/repositories/cjpais/Handy/1/) (2026-03)
- [TimesFlow — Tauri updater key incident](https://www.timesflow.app/download) (2026-04)
- [Hopp: Tauri vs Electron benchmarks & trade-offs](https://tool.lu/index.php/en_US/article/78K/preview) · [Rustify: Tauri vs Electron 2026](https://rustify.rs/articles/rust-tauri-vs-electron-2026)
- [Skiff — unsigned-release precedent](https://github.com/DrizzleTime/Skiff) · [Zenn: SignPath application walkthrough](https://zenn.dev/shm_7ec/articles/signpath-oss-code-signing?locale=en) (2026-02)
- [SO 2025 language usage aggregates](https://zenn.dev/inuinu/articles/where-is-csharp-used-2026?locale=en)
