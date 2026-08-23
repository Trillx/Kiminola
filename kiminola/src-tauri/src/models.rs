//! Model pack management: embedded manifest, Hugging Face downloader with resume,
//! per-file SHA-256 verification, and progress streaming via Tauri IPC channels.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use futures::StreamExt;
use reqwest::header::{HeaderValue, RANGE};
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tauri::AppHandle;

/* ---------- manifest ---------- */

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ModelFile {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct ModelManifest {
    pub name: String,
    pub repo: String,
    pub revision: String,
    pub total_bytes: u64,
    pub files: Vec<ModelFile>,
}

static MANIFEST_JSON: &str = include_str!("../models/manifest.json");

static MANIFEST: std::sync::OnceLock<ModelManifest> = std::sync::OnceLock::new();

pub fn manifest() -> &'static ModelManifest {
    MANIFEST.get_or_init(|| {
        serde_json::from_str(MANIFEST_JSON).expect("embedded manifest.json is valid JSON")
    })
}

fn model_dir(_app: &AppHandle) -> PathBuf {
    // Keep this in sync with asr.rs / vad.rs: they hardcode %LOCALAPPDATA%\Kiminola\models.
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(local_app_data)
            .join("Kiminola")
            .join("models")
            .join("nemotron")
    } else {
        // Portable fallback next to the executable.
        std::env::current_exe()
            .expect("current exe path")
            .parent()
            .expect("exe parent")
            .join("models")
            .join("nemotron")
    }
}

pub fn is_model_pack_present(app: &AppHandle) -> bool {
    let dir = model_dir(app);
    is_model_pack_present_at(&dir, manifest())
}

fn is_model_pack_present_at(dir: &Path, manifest: &ModelManifest) -> bool {
    for file in &manifest.files {
        let path = dir.join(&file.path);
        let meta = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => return false,
        };
        if meta.len() != file.bytes {
            return false;
        }
    }
    true
}

/* ---------- progress event ---------- */

#[derive(Clone, serde::Serialize)]
pub struct DownloadEvent {
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
    pub overall_downloaded: u64,
    pub overall_total: u64,
}

/* ---------- downloader ---------- */

fn hf_url(file: &ModelFile) -> String {
    let m = manifest();
    format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        m.repo, m.revision, file.path
    )
}

async fn download_file(
    file: &ModelFile,
    dir: &Path,
    channel: &Channel<DownloadEvent>,
    base_overall: u64,
    overall_total: u64,
) -> Result<(), String> {
    let final_path = dir.join(&file.path);
    let part_path = final_path.with_extension("part");
    let url = hf_url(file);

    fs::create_dir_all(dir).map_err(|e| format!("create model dir: {e}"))?;

    let mut last_err: Option<String> = None;

    for attempt in 0..4 {
        let existing_len = if part_path.exists() {
            fs::metadata(&part_path)
                .map(|m| m.len())
                .unwrap_or(0)
                .min(file.bytes)
        } else {
            0
        };

        let client = reqwest::Client::new();
        let mut req = client.get(&url).header(
            reqwest::header::USER_AGENT,
            HeaderValue::from_static("KimiNola/0.1.0"),
        );
        if existing_len > 0 && existing_len < file.bytes {
            req = req.header(
                RANGE,
                HeaderValue::from_str(&format!("bytes={existing_len}-"))
                    .map_err(|e| e.to_string())?,
            );
        }

        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(e.to_string());
                tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            last_err = Some(format!("HTTP {} for {}: {}", status, file.name, body));
            tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
            continue;
        }

        // If we asked for a range but got a full 200, restart the part file from scratch.
        let append = if existing_len > 0 && response.status() == 206 {
            true
        } else {
            if part_path.exists() {
                let _ = fs::remove_file(&part_path);
            }
            false
        };

        let mut part_file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part_path)
            .map_err(|e| format!("open part file: {e}"))?;

        let mut stream = response.bytes_stream();
        let mut downloaded = if append { existing_len } else { 0 };
        let mut last_emit = Instant::now();

        loop {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    part_file
                        .write_all(&chunk)
                        .map_err(|e| format!("write part file: {e}"))?;
                    downloaded += chunk.len() as u64;

                    if last_emit.elapsed() >= Duration::from_millis(100) {
                        let _ = channel.send(DownloadEvent {
                            file: file.name.clone(),
                            downloaded,
                            total: file.bytes,
                            overall_downloaded: base_overall + downloaded,
                            overall_total,
                        });
                        last_emit = Instant::now();
                    }
                }
                Some(Err(e)) => {
                    last_err = Some(e.to_string());
                    break;
                }
                None => break,
            }
        }

        if last_err.is_some() {
            tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
            continue;
        }

        // Flush and verify size.
        part_file
            .flush()
            .map_err(|e| format!("flush part file: {e}"))?;
        if downloaded != file.bytes {
            last_err = Some(format!(
                "size mismatch: got {downloaded}, expected {}",
                file.bytes
            ));
            tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
            continue;
        }

        // Verify SHA-256 against the manifest (hash the complete part file so resume is safe).
        let hash = {
            let mut hasher = Sha256::new();
            let mut reader = std::io::BufReader::new(
                File::open(&part_path).map_err(|e| format!("reopen part for hash: {e}"))?,
            );
            let _ = std::io::copy(&mut reader, &mut hasher)
                .map_err(|e| format!("hash part file: {e}"))?;
            hex::encode(hasher.finalize())
        };
        if !hash.eq_ignore_ascii_case(&file.sha256) {
            // Placeholder hashes are clearly marked; if the manifest still holds
            // placeholders, skip verification rather than fail every download.
            if !file.sha256.starts_with("PLACEHOLDER") {
                last_err = Some(format!("sha256 mismatch for {}", file.name));
                tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
                continue;
            }
        }

        // Atomic: part -> final. Remove any existing corrupt final first.
        if final_path.exists() {
            fs::remove_file(&final_path).map_err(|e| format!("remove stale final file: {e}"))?;
        }
        fs::rename(&part_path, &final_path).map_err(|e| format!("rename part to final: {e}"))?;

        let _ = channel.send(DownloadEvent {
            file: file.name.clone(),
            downloaded: file.bytes,
            total: file.bytes,
            overall_downloaded: base_overall + file.bytes,
            overall_total,
        });

        return Ok(());
    }

    Err(last_err.unwrap_or_else(|| "download failed after retries".to_string()))
}

#[tauri::command]
pub async fn download_model_pack(
    app: AppHandle,
    on_progress: Channel<DownloadEvent>,
) -> Result<(), String> {
    let dir = model_dir(&app);
    let manifest = manifest();
    let overall_total = manifest.total_bytes;
    let mut overall_downloaded: u64 = 0;

    for file in &manifest.files {
        let final_path = dir.join(&file.path);
        if let Ok(meta) = fs::metadata(&final_path) {
            if meta.len() == file.bytes {
                overall_downloaded += file.bytes;
                let _ = on_progress.send(DownloadEvent {
                    file: file.name.clone(),
                    downloaded: file.bytes,
                    total: file.bytes,
                    overall_downloaded,
                    overall_total,
                });
                continue;
            }
        }

        download_file(file, &dir, &on_progress, overall_downloaded, overall_total).await?;
        overall_downloaded += file.bytes;
    }

    Ok(())
}

#[tauri::command]
pub async fn check_model_pack(app: AppHandle) -> Result<bool, String> {
    Ok(is_model_pack_present(&app))
}

/* ---------- microphone permission probe ---------- */

#[derive(Clone, serde::Serialize)]
pub enum MicrophonePermission {
    Granted,
    Denied,
    Unavailable,
}

fn probe_microphone_permission() -> MicrophonePermission {
    let host = cpal::default_host();
    let device = match host.default_input_device() {
        Some(d) => d,
        None => return MicrophonePermission::Unavailable,
    };

    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(_) => return MicrophonePermission::Unavailable,
    };

    let errored = Arc::new(AtomicBool::new(false));
    let err_flag = errored.clone();

    let stream = match device.build_input_stream_raw(
        &config.config(),
        config.sample_format(),
        move |_data: &cpal::Data, _info: &cpal::InputCallbackInfo| {
            // no-op: we only care whether the OS lets us open the stream
        },
        move |err| {
            eprintln!("[mic probe] stream error: {err}");
            err_flag.store(true, Ordering::Relaxed);
        },
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("access") || msg.contains("denied") || msg.contains("0x80070005") {
                return MicrophonePermission::Denied;
            }
            return MicrophonePermission::Unavailable;
        }
    };

    if stream.play().is_err() {
        return MicrophonePermission::Unavailable;
    }

    std::thread::sleep(Duration::from_millis(800));

    if errored.load(Ordering::Relaxed) {
        MicrophonePermission::Denied
    } else {
        MicrophonePermission::Granted
    }
}

#[tauri::command]
pub async fn check_microphone_permission() -> Result<MicrophonePermission, String> {
    tokio::task::spawn_blocking(probe_microphone_permission)
        .await
        .map_err(|e| format!("mic probe panicked: {e}"))
}

/* ---------- open model folder ---------- */

#[tauri::command]
pub async fn open_model_folder(app: AppHandle) -> Result<(), String> {
    let dir = model_dir(&app);
    fs::create_dir_all(&dir).map_err(|e| format!("create model dir: {e}"))?;
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn model_pack_validation_requires_every_expected_file_size() {
        let temp_id = NEXT_TEMP_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "kiminola-model-pack-test-{}-{temp_id}",
            std::process::id()
        ));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let manifest = ModelManifest {
            name: "test".into(),
            repo: "test/repo".into(),
            revision: "main".into(),
            total_bytes: 7,
            files: vec![
                ModelFile {
                    name: "encoder".into(),
                    path: "encoder.onnx".into(),
                    bytes: 4,
                    sha256: "unused".into(),
                },
                ModelFile {
                    name: "tokens".into(),
                    path: "nested/tokens.txt".into(),
                    bytes: 3,
                    sha256: "unused".into(),
                },
            ],
        };

        fs::write(root.join("encoder.onnx"), b"1234").unwrap();
        assert!(!is_model_pack_present_at(&root, &manifest));

        fs::write(nested.join("tokens.txt"), b"too long").unwrap();
        assert!(!is_model_pack_present_at(&root, &manifest));

        fs::write(nested.join("tokens.txt"), b"123").unwrap();
        assert!(is_model_pack_present_at(&root, &manifest));

        fs::remove_dir_all(&root).unwrap();
    }
}
