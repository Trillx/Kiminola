# Model manifest and downloader backend architecture

Type: research
Status: resolved
Blocked by:

## Question

Decide the backend architecture for the Model pack downloader: the manifest format, the download transport, resume/verification implementation, and how download progress is streamed to the Svelte frontend.

Specifically:
- What does the **Model manifest** look like? (Rust struct, embedded JSON, or a config file; fields beyond repo/revision/files/sizes/SHA-256.)
- Which HTTP client do we use? (`reqwest` in Rust, Tauri's native `http` API, or another option.)
- How are partial files resumed? (HTTP Range requests, `.part` files, an index/map of completed ranges.)
- Where does verification happen? (Per-file SHA-256 after each file completes, or once for the whole pack.)
- How is progress streamed to the frontend? (Tauri event channel, polling an invoke command, or another mechanism.)

Research Tauri 2's recommended patterns for long-running downloads and event streaming, then lock the decision. The output should be a concrete backend design the wizard UI can be built against.

## Resolution

Resolved by research subagent. Full findings in `.scratch/kiminola/research/06-model-manifest-and-downloader-backend.md`.

### Decisions

- **Manifest format & location**: embed `src-tauri/models/manifest.json` and load it via `include_str!`. Define typed `ModelManifest` / `ModelFile` Rust structs. The manifest pins `csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25` @ `main`.
- **HTTP client**: keep the existing `reqwest` dependency (`native-tls` + `stream` features). Do **not** use `tauri-plugin-http` or frontend fetch — pushing ~650 MB through IPC is wasteful.
- **Resume**: one `.part` file per model file. Request `Range: bytes={existing_len}-` from `https://huggingface.co/{repo}/resolve/{revision}/{file}`. If the server returns `200` instead of `206`, fall back to a full re-download.
- **Progress streaming**: use Tauri 2 `tauri::ipc::Channel` — the documented ordered streaming mechanism from Rust to frontend. Throttle updates to ~10 Hz.
- **SHA-256 verification**: per-file, immediately after download completes and before atomic rename `.part` → final. Stream from disk using the existing `sha2` crate.
- **Model size correction**: the 160 ms encoder is ~653 MB; total pack ~663 MB. Record exact byte sizes in the manifest (from HF API/HEAD) rather than relying on earlier ~631 MB estimates.
