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
        let manifest: ModelManifest =
            serde_json::from_str(MANIFEST_JSON).expect("embedded manifest.json is valid JSON");
        // Production files must always be content-verified. Placeholder hashes
        // remain available to test fixtures only; the embedded production
        // manifest may never carry one.
        assert!(
            manifest
                .files
                .iter()
                .all(|f| !f.sha256.starts_with("PLACEHOLDER")),
            "embedded production manifest must not contain placeholder hashes"
        );
        manifest
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
        if !is_model_file_present_at(&path, file) {
            return false;
        }
    }
    true
}

fn is_model_file_present_at(path: &Path, file: &ModelFile) -> bool {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if meta.len() != file.bytes {
        return false;
    }

    // Placeholder hashes exist only in test fixtures; the embedded production
    // manifest is rejected by `manifest()` if it carries one. Size remains the
    // strongest available check for such fixtures.
    if file.sha256.starts_with("PLACEHOLDER") {
        return true;
    }

    match sha256_file(path) {
        Ok(hash) => hash.eq_ignore_ascii_case(&file.sha256),
        Err(_) => false,
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut reader = std::io::BufReader::new(File::open(path)?);
    std::io::copy(&mut reader, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

async fn is_model_file_present_async(path: PathBuf, file: ModelFile) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || is_model_file_present_at(&path, &file))
        .await
        .map_err(|e| format!("model file health check panicked: {e}"))
}

async fn sha256_file_async(path: PathBuf) -> Result<String, String> {
    tokio::task::spawn_blocking(move || sha256_file(&path))
        .await
        .map_err(|e| format!("model hash task panicked: {e}"))?
        .map_err(|e| format!("hash model file: {e}"))
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
            HeaderValue::from_static("KimiNola/0.1.1"),
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
        drop(part_file);
        let hash = sha256_file_async(part_path.clone()).await?;
        if !hash.eq_ignore_ascii_case(&file.sha256) {
            // Placeholder hashes are a test-fixture convenience (the embedded
            // production manifest never carries one); skip verification only
            // for those rather than fail every fixture download.
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
        if is_model_file_present_async(final_path, file.clone()).await? {
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

        download_file(file, &dir, &on_progress, overall_downloaded, overall_total).await?;
        overall_downloaded += file.bytes;
    }

    Ok(())
}

#[tauri::command]
pub async fn check_model_pack(app: AppHandle) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || is_model_pack_present(&app))
        .await
        .map_err(|e| format!("model health check panicked: {e}"))
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
    use std::sync::Mutex;

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    #[ignore = "requires the provisioned production model pack"]
    fn production_model_pack_matches_embedded_manifest() {
        let directory = std::env::var_os("KIMINOLA_ASR_MODEL_DIR")
            .map(PathBuf::from)
            .expect("KIMINOLA_ASR_MODEL_DIR is required for hardware validation");
        assert!(
            is_model_pack_present_at(&directory, manifest()),
            "provisioned model pack at '{}' does not match the embedded production manifest",
            directory.display()
        );
    }

    fn microphone_tone_amplitude(samples: &[f32], sample_rate: u32, frequency: f32) -> f32 {
        if samples.is_empty() || sample_rate == 0 {
            return 0.0;
        }
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        let radians_per_sample = 2.0 * std::f32::consts::PI * frequency / sample_rate as f32;
        let (in_phase, quadrature) = samples.iter().enumerate().fold(
            (0.0_f32, 0.0_f32),
            |(in_phase, quadrature), (index, sample)| {
                let phase = radians_per_sample * index as f32;
                let centered = *sample - mean;
                (
                    in_phase + centered * phase.cos(),
                    quadrature + centered * phase.sin(),
                )
            },
        );
        2.0 * in_phase.hypot(quadrature) / samples.len() as f32
    }

    #[test]
    fn microphone_tone_amplitude_identifies_the_known_stimulus() {
        let sample_rate = 48_000_u32;
        let samples: Vec<f32> = (0..sample_rate)
            .map(|sample| {
                (2.0 * std::f32::consts::PI * 440.0 * sample as f32 / sample_rate as f32).sin()
                    * 0.2
            })
            .collect();

        let target = microphone_tone_amplitude(&samples, sample_rate, 440.0);
        let off_target = microphone_tone_amplitude(&samples, sample_rate, 520.0);
        assert!(target > 0.19, "known tone amplitude was {target}");
        assert!(
            target > off_target * 20.0,
            "known tone {target} was not isolated from off-target energy {off_target}"
        );
    }

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
                    sha256: "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4"
                        .into(),
                },
                ModelFile {
                    name: "tokens".into(),
                    path: "nested/tokens.txt".into(),
                    bytes: 3,
                    sha256: "a665a45920422f9d417e4867efdc4fb8a04a1f3fff1fa07e998e86f7f7a27ae3"
                        .into(),
                },
            ],
        };

        fs::write(root.join("encoder.onnx"), b"1234").unwrap();
        assert!(!is_model_pack_present_at(&root, &manifest));

        fs::write(nested.join("tokens.txt"), b"too long").unwrap();
        assert!(!is_model_pack_present_at(&root, &manifest));

        fs::write(nested.join("tokens.txt"), b"123").unwrap();
        assert!(is_model_pack_present_at(&root, &manifest));

        fs::write(root.join("encoder.onnx"), b"5678").unwrap();
        assert!(!is_model_pack_present_at(&root, &manifest));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn model_pack_validation_allows_size_only_placeholder_hashes() {
        let temp_id = NEXT_TEMP_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "kiminola-model-pack-placeholder-test-{}-{temp_id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let manifest = ModelManifest {
            name: "test".into(),
            repo: "test/repo".into(),
            revision: "main".into(),
            total_bytes: 4,
            files: vec![ModelFile {
                name: "tokens".into(),
                path: "tokens.txt".into(),
                bytes: 4,
                sha256: "PLACEHOLDER_FILL_BEFORE_RELEASE_tokens_txt".into(),
            }],
        };

        fs::write(root.join("tokens.txt"), b"5678").unwrap();
        assert!(is_model_pack_present_at(&root, &manifest));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn production_manifest_pins_revision_and_tokens_digest() {
        let m = manifest();
        assert_eq!(
            m.revision, "237e551abd7a411ef92d3595454d9f6ab5fe7d6c",
            "production manifest must pin the verified upstream revision"
        );
        let tokens = m
            .files
            .iter()
            .find(|f| f.path == "tokens.txt")
            .expect("manifest has a tokens.txt entry");
        assert_eq!(
            tokens.sha256, "dc0b4584ab2e4ddbf888425c076c61b736e7356a015250db7d307e6f1a8188ff",
            "tokens.txt must carry the verified SHA-256"
        );
        assert!(
            m.files.iter().all(|f| !f.sha256.starts_with("PLACEHOLDER")),
            "production files must not bypass content verification via placeholder hashes"
        );
    }

    #[test]
    fn production_tokens_txt_health_accepts_valid_rejects_corrupt() {
        // The real production manifest entry for tokens.txt.
        let tokens = manifest()
            .files
            .iter()
            .find(|f| f.path == "tokens.txt")
            .expect("manifest has a tokens.txt entry")
            .clone();
        // Pack-level health over the single production entry: the same check
        // `check_model_pack` reports, without staging the 650MB ONNX files.
        // This test covers health only; fetch selection is covered separately
        // at `is_model_file_present_async` below.
        let pack_view = ModelManifest {
            name: "production-tokens-view".into(),
            repo: manifest().repo.clone(),
            revision: manifest().revision.clone(),
            total_bytes: tokens.bytes,
            files: vec![tokens],
        };

        let temp_id = NEXT_TEMP_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "kiminola-model-pack-corrupt-tokens-test-{}-{temp_id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("tokens.txt");

        // Verified upstream content (SHA-256 dc0b4584…8188ff): pack healthy.
        let valid = include_bytes!("../tests/fixtures/tokens.txt");
        assert_eq!(valid.len() as u64, pack_view.files[0].bytes);
        fs::write(&path, valid).unwrap();
        assert!(is_model_pack_present_at(&root, &pack_view));

        // Same size, corrupted content: pack unhealthy.
        let mut corrupted = valid.to_vec();
        corrupted[0] ^= 0xFF;
        fs::write(&path, corrupted).unwrap();
        assert!(!is_model_pack_present_at(&root, &pack_view));

        fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn download_selects_refetch_for_corrupted_production_tokens_txt() {
        // The exact per-file decision seam `download_model_pack` uses
        // (models.rs: `if is_model_file_present_async(...) { skip } else {
        // download_file(...) }`): Ok(false) means the file is re-fetched.
        let tokens = manifest()
            .files
            .iter()
            .find(|f| f.path == "tokens.txt")
            .expect("manifest has a tokens.txt entry")
            .clone();

        let temp_id = NEXT_TEMP_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "kiminola-model-fetch-selection-test-{}-{temp_id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join(&tokens.path);

        // Valid production bytes: file present, no fetch — no network I/O.
        let valid = include_bytes!("../tests/fixtures/tokens.txt");
        fs::write(&path, valid).unwrap();
        assert!(is_model_file_present_async(path.clone(), tokens.clone())
            .await
            .unwrap());

        // Same size, corrupted content: file absent, fetch required.
        let mut corrupted = valid.to_vec();
        corrupted[0] ^= 0xFF;
        fs::write(&path, corrupted).unwrap();
        assert!(!is_model_file_present_async(path, tokens).await.unwrap());

        fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn async_model_file_validation_matches_manifest_hash() {
        let temp_id = NEXT_TEMP_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "kiminola-model-pack-async-test-{}-{temp_id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let file = ModelFile {
            name: "encoder".into(),
            path: "encoder.onnx".into(),
            bytes: 4,
            sha256: "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4".into(),
        };
        let path = root.join(&file.path);

        fs::write(&path, b"1234").unwrap();
        assert!(is_model_file_present_async(path.clone(), file.clone())
            .await
            .unwrap());

        fs::write(&path, b"5678").unwrap();
        assert!(!is_model_file_present_async(path, file).await.unwrap());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    #[ignore = "requires the configured physical microphone and an audible 440 Hz runner stimulus"]
    fn physical_microphone_captures_known_tone() {
        fn record_samples<I>(samples: I, captured: &Mutex<Vec<f32>>)
        where
            I: Iterator<Item = f32>,
        {
            captured
                .lock()
                .expect("microphone samples lock")
                .extend(samples);
        }

        let expected_device_name = std::env::var("KIMINOLA_EXPECTED_MICROPHONE_NAME")
            .expect("KIMINOLA_EXPECTED_MICROPHONE_NAME must identify the provisioned microphone");
        assert!(
            !expected_device_name.trim().is_empty(),
            "KIMINOLA_EXPECTED_MICROPHONE_NAME must not be empty"
        );
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .expect("the hardware runner must expose a default input device");
        let device_name = device
            .name()
            .expect("the default input device must expose its name");
        assert!(
            device_name.eq_ignore_ascii_case(expected_device_name.trim()),
            "default input device '{device_name}' does not match provisioned physical microphone '{}'",
            expected_device_name.trim()
        );
        let normalized_device_name = device_name.to_ascii_lowercase();
        for virtual_marker in [
            "loopback",
            "stereo mix",
            "what u hear",
            "virtual",
            "voicemeeter",
            "cable output",
        ] {
            assert!(
                !normalized_device_name.contains(virtual_marker),
                "configured input device '{device_name}' appears to be virtual or loopback"
            );
        }

        let supported = device
            .default_input_config()
            .expect("the default microphone must expose an input format");
        let sample_rate = supported.sample_rate().0;
        let channels = usize::from(supported.channels());
        let sample_format = supported.sample_format();
        let config = supported.config();
        let captured = Arc::new(Mutex::new(Vec::<f32>::new()));
        let stream_failed = Arc::new(AtomicBool::new(false));

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let captured = Arc::clone(&captured);
                let stream_failed = Arc::clone(&stream_failed);
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        record_samples(data.iter().copied(), &captured);
                    },
                    move |_| stream_failed.store(true, AtomicOrdering::Relaxed),
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let captured = Arc::clone(&captured);
                let stream_failed = Arc::clone(&stream_failed);
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        record_samples(
                            data.iter()
                                .map(|sample| f32::from(*sample) / f32::from(i16::MAX)),
                            &captured,
                        );
                    },
                    move |_| stream_failed.store(true, AtomicOrdering::Relaxed),
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let captured = Arc::clone(&captured);
                let stream_failed = Arc::clone(&stream_failed);
                device.build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        record_samples(
                            data.iter()
                                .map(|sample| (f32::from(*sample) - 32_768.0) / 32_768.0),
                            &captured,
                        );
                    },
                    move |_| stream_failed.store(true, AtomicOrdering::Relaxed),
                    None,
                )
            }
            other => panic!("unsupported hardware microphone sample format: {other:?}"),
        }
        .expect("the hardware runner microphone stream must open");

        stream.play().expect("the microphone stream must start");
        std::thread::sleep(Duration::from_secs(3));
        drop(stream);

        assert!(
            !stream_failed.load(AtomicOrdering::Relaxed),
            "the physical microphone stream reported an asynchronous error"
        );
        let captured = captured.lock().expect("microphone samples lock");
        let mono: Vec<f32> = captured
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        assert!(
            mono.len() >= sample_rate as usize,
            "the physical microphone captured only {} frames",
            mono.len()
        );
        let peak = mono.iter().map(|sample| sample.abs()).fold(0.0, f32::max);
        let target_amplitude = microphone_tone_amplitude(&mono, sample_rate, 440.0);
        let off_target_amplitude = [320.0_f32, 360.0, 520.0, 700.0]
            .into_iter()
            .map(|frequency| microphone_tone_amplitude(&mono, sample_rate, frequency))
            .fold(0.0, f32::max);
        let rms =
            (mono.iter().map(|sample| sample * sample).sum::<f32>() / mono.len() as f32).sqrt();
        let capture_mode = std::env::var("KIMINOLA_MICROPHONE_CAPTURE_MODE")
            .expect("KIMINOLA_MICROPHONE_CAPTURE_MODE must be baseline or stimulus");
        assert!(
            matches!(capture_mode.as_str(), "baseline" | "stimulus"),
            "KIMINOLA_MICROPHONE_CAPTURE_MODE must be baseline or stimulus"
        );
        let metrics_path = std::env::var("KIMINOLA_MICROPHONE_METRICS_PATH")
            .expect("KIMINOLA_MICROPHONE_METRICS_PATH must name the metrics output file");
        fs::write(
            metrics_path,
            format!(
                "{{\"target_amplitude\":{target_amplitude},\"off_target_amplitude\":{off_target_amplitude},\"rms\":{rms},\"peak\":{peak}}}"
            ),
        )
        .expect("microphone capture metrics must be written");
        if capture_mode == "baseline" {
            return;
        }
        assert!(
            peak >= 0.005,
            "the physical microphone was silent (normalized peak {peak})"
        );
        assert!(
            target_amplitude >= 0.001,
            "the physical microphone did not capture the 440 Hz stimulus (amplitude {target_amplitude})"
        );
        assert!(
            target_amplitude > off_target_amplitude * 2.0,
            "440 Hz amplitude {target_amplitude} was not isolated from off-target amplitude {off_target_amplitude}"
        );
        assert!(
            target_amplitude >= rms * 0.15,
            "440 Hz amplitude {target_amplitude} was too small relative to captured RMS {rms}"
        );
    }
}
