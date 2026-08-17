# Research 06 — Model manifest and downloader backend architecture

Date researched: 2026-08-16. Sources are primary (Tauri 2 docs, reqwest docs, Hugging Face, sherpa-onnx docs) unless noted.

## TL;DR — concrete recommendation

1. **Manifest**: a typed Rust struct generated from an embedded JSON file (`models/manifest.json` via `include_str!`). Pin one model pack: `csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25`, `main` branch. Record repo, revision, target directory, and a per-file entry with `path`, `size`, and `sha256`.
2. **HTTP client**: `reqwest` (already in `Cargo.toml`) with the existing `native-tls` feature. No extra Tauri plugin needed; Rust commands have full network access and the binary size stays the same.
3. **Resume**: HTTP `Range: bytes={existing_len}-` requests against `<repo>/resolve/main/<file>`. Write to `<file>.part`; on restart, resume from the `.part` length. Servers that do not honor ranges fall back to a full re-download.
4. **Progress streaming**: Tauri 2 `tauri::ipc::Channel` passed into the download command. It is ordered, typed, and the documented pattern for long-running streaming operations such as downloads ([Tauri docs](https://v2.tauri.app/develop/calling-frontend/)). Emit a small enum: `Started`, `Progress { file, bytes, total }`, `FileDone`, `Verifying`, `Verified`, `Error`, `Finished`.
5. **SHA-256 verification**: per-file, immediately after a file finishes downloading, before renaming `.part` to the final name. Use the already-included `sha2` crate with `tokio::io::AsyncReadExt` to stream the file through `Sha256` without loading it into memory. Failing files are deleted and retried from scratch.

---

## 1. Model manifest format and where it lives

### Current constraints from the codebase

- `src-tauri/src/asr.rs` expects either:
  - `%LOCALAPPDATA%\Kiminola\models\nemotron\{encoder.int8.onnx, decoder.int8.onnx, joiner.int8.onnx, tokens.txt}`, or
  - an executable-relative `models\nemotron\...` fallback.
- The pinned Nemotron streaming 0.6B EN INT8 160 ms pack is at `csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25` on Hugging Face. Its files are ([HF repo tree](https://huggingface.co/csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25/tree/main)):

| File | Size (web UI) | Storage |
|---|---|---|
| `encoder.int8.onnx` | ~653 MB | Xet |
| `decoder.int8.onnx` | ~7.26 MB | Xet |
| `joiner.int8.onnx` | ~1.74 MB | Xet |
| `tokens.txt` | ~8.95 kB | git |

> Note: earlier notes quoted ~631 MB; the HF UI lists the 160 ms encoder at ~653 MB and the repo total at ~663 MB. The manifest should record exact byte sizes from the HF API/HEAD request, not rounded UI values.

### Manifest schema

Keep the manifest in the binary as an embedded JSON file so it is version-locked to the release and cannot drift. A new `models/manifest.json` under `src-tauri/` plus `include_str!` is the simplest approach and matches how other Tauri apps ship pinned model metadata.

```json
{
  "schema_version": 1,
  "pack_id": "nemotron-160ms-int8",
  "target_dir": "nemotron",
  "repo": "csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25",
  "revision": "main",
  "files": [
    {
      "path": "encoder.int8.onnx",
      "size": 653XXX XXX,
      "sha256": "..."
    },
    {
      "path": "decoder.int8.onnx",
      "size": 762XXXX,
      "sha256": "..."
    },
    {
      "path": "joiner.int8.onnx",
      "size": 182XXXX,
      "sha256": "..."
    },
    {
      "path": "tokens.txt",
      "size": 8950,
      "sha256": "..."
    }
  ]
}
```

### Rust representation

```rust
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ModelManifest {
    pub schema_version: u32,
    pub pack_id: String,
    pub target_dir: String,
    pub repo: String,
    pub revision: String,
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ModelFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
```

Load it once at startup:

```rust
const MANIFEST_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/models/manifest.json"));

impl ModelManifest {
    pub fn embedded() -> anyhow::Result<Self> {
        serde_json::from_str(MANIFEST_JSON).map_err(Into::into)
    }
}
```

### Why not a runtime config / remote manifest?

- The MVP ships exactly one model pack; there is no model manager UI and no need to update manifests without shipping a new binary.
- Embedding prevents a broken or tampered remote manifest from causing the app to download the wrong weights.
- A remote manifest would add a network dependency before first-run and complicate reproducible builds. Revisit only if the product adds multiple model packs or over-the-air model updates.

---

## 2. HTTP client choice

### Existing dependency

`Cargo.toml` already depends on:

```toml
reqwest = { version = "0.12", default-features = false, features = ["native-tls", "stream", "json"] }
```

That is exactly what we need. `stream` gives `Response::bytes_stream()`/`chunk()` for progressive file writes, and `native-tls` uses the Windows certificate store, which is appropriate for a Windows-first app that may run behind corporate proxies.

### Tauri plugin-http vs. plain reqwest

Tauri 2 offers `tauri-plugin-http`, which re-exports reqwest on the Rust side and exposes a fetch-like API on the frontend ([Tauri HTTP client docs](https://v2.tauri.app/plugin/http-client/)). For this use case it adds no value:

- The download must run in Rust anyway (progress streaming, resume, SHA-256, atomic writes, disk layout).
- `reqwest` is already a direct dependency and is the underlying client Tauri uses.
- Using the frontend `fetch` would require piping ~650 MB through the Tauri IPC layer, which is slower and unnecessary.
- Avoiding `tauri-plugin-http` keeps the capability file smaller and the binary unchanged.

### TLS backend

The current `native-tls` feature is the right default for Windows because it trusts the OS certificate store. `rustls` is a fine alternative ([reqwest 0.13 will default to rustls](https://seanmonstar.com/blog/reqwest-v013-rustls-default/)), but switching is not required for the downloader. If corporate proxy issues appear later, `native-tls` is the more compatible choice on Windows.

### Recommended client setup

```rust
use reqwest::Client;
use std::time::Duration;

fn http_client() -> reqwest::Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(300))   // per-operation ceiling
        .connect_timeout(Duration::from_secs(30))
        .user_agent("Kiminola/0.1.0")
        .build()
}
```

Create one `Client` and reuse it; reqwest internally pools connections ([reqwest docs](https://docs.rs/reqwest/latest/reqwest/struct.Client.html)).

---

## 3. Resume via HTTP Range requests

### Approach

1. Compute the target path and the `.part` path (`<file>.part`).
2. If the final file already exists and passes SHA-256, skip.
3. If the `.part` file exists, get its length `existing_len`.
4. Issue a GET with `Range: bytes={existing_len}-`.
5. If the server returns `206 Partial Content`, open the `.part` file in append mode and stream the body.
6. If the server returns `200 OK` (range unsupported) or any other non-206, delete the `.part` and start over.
7. After the response completes, verify SHA-256; on success, atomically rename `.part` to the final name.

### Why a single `.part` file is enough

The files are each one continuous blob and HF supports byte ranges. A single offset per file is simpler and sufficient; a multi-range map is over-engineering for ~4 files. The worst-case resume cost is re-downloading the current file from scratch if the partial file is corrupt (caught by SHA-256).

### Implementation sketch

```rust
use reqwest::header::RANGE;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use std::path::Path;

async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_size: u64,
    channel: &tauri::ipc::Channel<DownloadEvent>,
) -> anyhow::Result<()> {
    let part = dest.with_extension("part");
    let existing_len = if part.exists() {
        tokio::fs::metadata(&part).await?.len()
    } else {
        0
    };

    let mut request = client.get(url);
    if existing_len > 0 {
        request = request.header(RANGE, format!("bytes={existing_len}-"));
    }

    let response = request.send().await?;
    let status = response.status();

    let (append, start_len) = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        (true, existing_len)
    } else if status.is_success() {
        // Server ignored range; restart.
        tokio::fs::remove_file(&part).await.ok();
        (false, 0)
    } else {
        anyhow::bail!("download failed: {status}");
    };

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .open(&part)
        .await?;
    let mut file = BufWriter::new(file);

    if !append {
        file.get_mut().set_len(0).await?;
    }

    let mut downloaded = start_len;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        channel.send(DownloadEvent::Progress { file: ..., downloaded, total: expected_size })?;
    }

    file.flush().await?;
    drop(file);

    // SHA-256 verification happens here, then rename.
    verify_and_rename(&part, dest, expected_sha256).await?;
    Ok(())
}
```

### Hugging Face URL pattern

Direct file downloads use:

```
https://huggingface.co/{repo}/resolve/{revision}/{file_path}
```

For example:

```
https://huggingface.co/csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25/resolve/main/encoder.int8.onnx
```

This works for both LFS and Xet-backed files; HF serves them over plain HTTP with `Accept-Ranges: bytes` support for the actual blob ([HF Xet docs](https://huggingface.co/docs/hub/xet/using-xet-storage)).

---

## 4. Streaming progress to the Svelte frontend

### Use Tauri Channels, not global events or polling

Tauri 2 documentation distinguishes three mechanisms:

- **Commands** for RPC-style request/response.
- **Events** (`emit` / `listen`) for small, infrequent, multi-consumer messages. Payloads are JSON strings and ordering is not guaranteed under async load.
- **Channels** (`tauri::ipc::Channel`) for ordered, typed, high-throughput streaming. The docs explicitly call channels "the recommended mechanism for streaming data such as streamed HTTP responses to the frontend" ([Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/)).

For a 650 MB download we want many small progress messages (one per chunk or throttled to ~10 Hz); channels are the right tool.

### Rust side

```rust
use tauri::ipc::Channel;
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum DownloadEvent {
    Started { file: String, size: u64 },
    Progress { file: String, downloaded: u64, total: u64 },
    FileDone { file: String },
    Verifying { file: String },
    Verified { file: String },
    Error { file: String, message: String },
    Finished,
}

#[tauri::command]
async fn download_model_pack(channel: Channel<DownloadEvent>) -> Result<(), String> {
    // spawn the work; channel.send(...) drives the UI
}
```

### Frontend side

```typescript
import { invoke, Channel } from "@tauri-apps/api/core";

export type DownloadEvent =
  | { event: "started"; data: { file: string; size: number } }
  | { event: "progress"; data: { file: string; downloaded: number; total: number } }
  | { event: "fileDone"; data: { file: string } }
  | { event: "verifying"; data: { file: string } }
  | { event: "verified"; data: { file: string } }
  | { event: "error"; data: { file: string; message: string } }
  | { event: "finished" };

export async function downloadModelPack(
  onEvent: (event: DownloadEvent) => void,
): Promise<void> {
  const channel = new Channel<DownloadEvent>();
  channel.onmessage = onEvent;
  await invoke("download_model_pack", { channel });
}
```

### Throttling

Sending an event for every network chunk would saturate the frontend. Throttle progress emissions to ~10 per second per file using `tokio::time::{interval, Instant}` or a simple byte threshold (e.g., emit only when `downloaded % 256 KiB == 0` or every 100 ms, whichever comes first).

---

## 5. SHA-256 verification

### Where and when

Verify each file **immediately after it is fully written**, before renaming from `.part` to the final filename. This bounds corruption to one file and avoids ever presenting a half-verified model directory to `asr.rs`.

If verification fails, delete both the `.part` and any final file, then retry that file from scratch (with backoff). Do not advance to the next file until the current one is verified.

### Implementation

The `sha2` crate is already in `Cargo.toml`. Stream the file through `Sha256` instead of reading it whole:

```rust
use sha2::{Digest, Sha256};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, BufReader};

async fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
```

### Optional: hash while downloading

For very large files you can hash chunks as they arrive and avoid a second disk read. This is a small optimization; with modern SSDs the separate verify pass is simpler and fast enough for ~650 MB. The recommendation is to verify after download and keep the code simple unless profiling shows the second read is a bottleneck.

---

## 6. Suggested module layout

Add a new Rust module `src-tauri/src/model_pack.rs` (or `models.rs`) with:

- `ModelManifest` / `ModelFile` structs and `manifest.json` embedding.
- `resolve_model_dir()` — detect a valid local pack (reuse `asr::resolve_asr_model_dir` logic).
- `download_model_pack(channel)` — the Tauri command.
- `download_file(...)` — single-file resume + stream + verify.
- `verify_and_rename(...)` — SHA-256 + atomic rename.

Wire the command into `lib.rs` and export a matching TypeScript wrapper in `src/lib/tauri.ts`.

---

## Sources

- Tauri 2 — Calling the Frontend from Rust (events and channels): https://v2.tauri.app/develop/calling-frontend/
- Tauri 2 — Calling Rust from the Frontend (commands and channels): https://v2.tauri.app/develop/calling-rust/
- Tauri 2 — HTTP client plugin: https://v2.tauri.app/plugin/http-client/
- reqwest docs — `Client`: https://docs.rs/reqwest/latest/reqwest/struct.Client.html
- reqwest docs — `Response` (chunk/bytes_stream): https://docs.rs/reqwest/latest/reqwest/struct.Response.html
- Hugging Face repo for the pinned 160 ms Nemotron pack: https://huggingface.co/csukuangfj2/sherpa-onnx-nemotron-speech-streaming-en-0.6b-160ms-int8-2026-04-25/tree/main
- Hugging Face Xet storage overview: https://huggingface.co/docs/hub/xet/using-xet-storage
- sherpa-onnx NeMo model index: https://k2-fsa.github.io/sherpa/onnx/nemo/index.html
- reqwest 0.13 rustls-default announcement: https://seanmonstar.com/blog/reqwest-v013-rustls-default/
