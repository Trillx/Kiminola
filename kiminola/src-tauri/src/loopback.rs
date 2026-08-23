use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as sync_mpsc;
use std::sync::{Arc, Mutex as SyncMutex};

use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use windows::core::{implement, w, IUnknown, Interface, HRESULT, PROPVARIANT};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eConsole, eRender, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, BLOB, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, Sleep, WaitForSingleObject};

use crate::recording_session::{AudioBuffer, AudioPressureCounters};

const REFTIMES_PER_MILLISEC: i64 = 10000;
const BUFFER_DURATION_100NS: i64 = 100 * REFTIMES_PER_MILLISEC; // 100 ms

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const VT_BLOB: u16 = 65;

type ActivateResult = Arc<SyncMutex<Option<(HRESULT, Option<usize>)>>>;

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ProcessLoopbackActivation {
    event: HANDLE,
    result: ActivateResult,
}

impl Drop for ProcessLoopbackActivation {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

impl IActivateAudioInterfaceCompletionHandler_Impl for ProcessLoopbackActivation_Impl {
    fn ActivateCompleted(
        &self,
        operation: Option<&IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let mut activation_result = HRESULT::default();
        let mut activated_interface = None;
        if let Some(operation) = operation {
            if let Err(error) = unsafe {
                operation.GetActivateResult(&mut activation_result, &mut activated_interface)
            } {
                activation_result = error.code();
            }
        } else {
            activation_result = HRESULT(0x8000_4003u32 as i32); // E_POINTER
        }

        let activated_interface =
            activated_interface.map(|interface| interface.into_raw() as usize);
        *self.result.lock().unwrap() = Some((activation_result, activated_interface));
        unsafe { SetEvent(self.event)? };
        Ok(())
    }
}

/// ABI-compatible VT_BLOB PROPVARIANT. windows 0.58 keeps the raw union
/// private, so the generated API receives this stable C layout by pointer.
#[repr(C)]
struct BlobPropVariant {
    vt: u16,
    reserved1: u16,
    reserved2: u16,
    reserved3: u16,
    blob: BLOB,
}

#[derive(Clone, Copy)]
enum SampleEncoding {
    I16,
    F32,
}

struct ActiveCapture {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    sample_rate: u32,
    channels: usize,
    encoding: SampleEncoding,
    label: &'static str,
    process_tree: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct LoopbackStart {
    pub sample_rate: u32,
    pub process_tree: bool,
}

/// Captures a meeting process tree when a PID is supplied, falling back to the
/// default render endpoint when process activation is unavailable. Sends mono
/// f32 buffers to the supplied channel until `cancel` is set.
///
/// `started_tx` is signaled once with the loopback sample rate once the client is
/// initialized and running, or with an error if initialization fails.
pub fn capture_loopback(
    audio_tx: mpsc::Sender<AudioBuffer>,
    cancel: Arc<AtomicBool>,
    started_tx: sync_mpsc::Sender<Result<LoopbackStart, String>>,
    target_process_id: Option<u32>,
    pressure: Arc<AudioPressureCounters>,
) {
    let result = unsafe {
        capture_loopback_inner(
            &audio_tx,
            &cancel,
            &started_tx,
            target_process_id,
            &pressure,
        )
    };
    let _ = started_tx.send(result);
}

unsafe fn capture_loopback_inner(
    audio_tx: &mpsc::Sender<AudioBuffer>,
    cancel: &Arc<AtomicBool>,
    started_tx: &sync_mpsc::Sender<Result<LoopbackStart, String>>,
    target_process_id: Option<u32>,
    pressure: &AudioPressureCounters,
) -> Result<LoopbackStart, String> {
    CoInitializeEx(None, COINIT_MULTITHREADED)
        .ok()
        .map_err(|e| format!("COM init failed: {e}"))?;

    let active = if let Some(process_id) = target_process_id {
        match open_process_capture(process_id) {
            Ok(capture) => capture,
            Err(error) => {
                eprintln!(
                    "[loopback] process-tree capture unavailable for PID {process_id}: {error}; falling back to the default output"
                );
                open_classic_capture()?
            }
        }
    } else {
        open_classic_capture()?
    };

    eprintln!(
        "[loopback] {} capture started: {} Hz, {} channel(s)",
        active.label, active.sample_rate, active.channels
    );

    let started = LoopbackStart {
        sample_rate: active.sample_rate,
        process_tree: active.process_tree,
    };
    let _ = started_tx.send(Ok(started));
    let mut dropped_buffers = 0u64;

    'capture_loop: while !cancel.load(Ordering::Relaxed) {
        let packet_length = match active.capture.GetNextPacketSize() {
            Ok(n) => n,
            Err(e) => {
                eprintln!("loopback GetNextPacketSize failed: {e}");
                break;
            }
        };

        if packet_length == 0 {
            Sleep(10);
            continue;
        }

        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut frames_available = 0u32;
        let mut flags = 0u32;
        if let Err(e) =
            active
                .capture
                .GetBuffer(&mut data_ptr, &mut frames_available, &mut flags, None, None)
        {
            eprintln!("loopback GetBuffer failed: {e}");
            break;
        }

        if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 == 0
            && !data_ptr.is_null()
            && frames_available > 0
        {
            let samples = read_buffer(
                data_ptr,
                frames_available as usize,
                active.channels,
                active.encoding,
            );
            if !samples.is_empty() {
                let sample_count = samples.len();
                match audio_tx.try_send(AudioBuffer::Loopback(samples)) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        pressure.add_loopback(sample_count);
                        dropped_buffers = dropped_buffers.saturating_add(1);
                        if dropped_buffers.is_power_of_two() {
                            eprintln!(
                                "[loopback] capture queue full; dropped {dropped_buffers} buffer(s)"
                            );
                        }
                    }
                    Err(TrySendError::Closed(_)) => break 'capture_loop,
                }
            }
        }

        if let Err(e) = active.capture.ReleaseBuffer(frames_available) {
            eprintln!("loopback ReleaseBuffer failed: {e}");
            break;
        }
    }

    let _ = active.client.Stop();
    Ok(started)
}

unsafe fn open_process_capture(process_id: u32) -> Result<ActiveCapture, String> {
    let client = activate_process_loopback(process_id)?;
    initialize_capture(client, true, "meeting process")
}

unsafe fn open_classic_capture() -> Result<ActiveCapture, String> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .map_err(|e| format!("failed to create device enumerator: {e}"))?;
    let device = enumerator
        .GetDefaultAudioEndpoint(eRender, eConsole)
        .map_err(|e| format!("no default render endpoint: {e}"))?;
    let client: IAudioClient = device
        .Activate(CLSCTX_ALL, None)
        .map_err(|e| format!("failed to activate default output: {e}"))?;
    initialize_capture(client, false, "default-output")
}

unsafe fn activate_process_loopback(process_id: u32) -> Result<IAudioClient, String> {
    let params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: process_id,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let blob = BlobPropVariant {
        vt: VT_BLOB,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
        blob: BLOB {
            cbSize: std::mem::size_of_val(&params) as u32,
            pBlobData: &params as *const _ as *mut u8,
        },
    };

    let event = CreateEventW(None, true, false, None)
        .map_err(|e| format!("failed to create process-loopback event: {e}"))?;
    let result: ActivateResult = Arc::new(SyncMutex::new(None));
    let handler: IActivateAudioInterfaceCompletionHandler = ProcessLoopbackActivation {
        event,
        result: Arc::clone(&result),
    }
    .into();

    let _operation = ActivateAudioInterfaceAsync(
        w!("VAD\\Process_Loopback"),
        &IAudioClient::IID,
        Some(&blob as *const BlobPropVariant as *const PROPVARIANT),
        &handler,
    )
    .map_err(|e| format!("process-loopback activation failed: {e}"))?;

    if WaitForSingleObject(event, 15_000) != WAIT_OBJECT_0 {
        return Err("process-loopback activation timed out".into());
    }
    let (activation_result, activated_interface) = result
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "process-loopback activation returned no result".to_string())?;
    activation_result
        .ok()
        .map_err(|e| format!("process-loopback activation returned {e}"))?;
    let activated_interface = activated_interface
        .map(|raw| IUnknown::from_raw(raw as *mut _))
        .ok_or_else(|| "process-loopback activation returned no audio client".to_string())?;
    activated_interface
        .cast::<IAudioClient>()
        .map_err(|e| format!("activated interface was not an audio client: {e}"))
}

unsafe fn initialize_capture(
    client: IAudioClient,
    process_virtual: bool,
    label: &'static str,
) -> Result<ActiveCapture, String> {
    let mut process_format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM,
        nChannels: 2,
        nSamplesPerSec: 44_100,
        nAvgBytesPerSec: 44_100 * 4,
        nBlockAlign: 4,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let format_ptr = if process_virtual {
        &mut process_format
    } else {
        let ptr = client
            .GetMixFormat()
            .map_err(|e| format!("failed to get default-output mix format: {e}"))?;
        if ptr.is_null() {
            return Err("default-output mix format was null".into());
        }
        ptr
    };

    let sample_rate = (*format_ptr).nSamplesPerSec;
    let channels = (*format_ptr).nChannels as usize;
    let bits_per_sample = (*format_ptr).wBitsPerSample;
    let format_tag = (*format_ptr).wFormatTag;
    let encoding = match (format_tag, bits_per_sample) {
        (WAVE_FORMAT_PCM, 16) => SampleEncoding::I16,
        (WAVE_FORMAT_IEEE_FLOAT | WAVE_FORMAT_EXTENSIBLE, 32) => SampleEncoding::F32,
        _ => {
            if !process_virtual {
                CoTaskMemFree(Some(format_ptr as *const _ as *const _));
            }
            return Err(format!(
                "unsupported loopback format: tag=0x{format_tag:04x}, {bits_per_sample} bits"
            ));
        }
    };

    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
        | if process_virtual {
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
        } else {
            0
        };
    let initialize_result = client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        flags,
        if process_virtual {
            10_000_000
        } else {
            BUFFER_DURATION_100NS
        },
        0,
        format_ptr,
        None,
    );
    if !process_virtual {
        CoTaskMemFree(Some(format_ptr as *const _ as *const _));
    }
    initialize_result.map_err(|e| format!("failed to initialize {label} loopback: {e}"))?;

    let capture: IAudioCaptureClient = client
        .GetService()
        .map_err(|e| format!("failed to get {label} capture client: {e}"))?;
    client
        .Start()
        .map_err(|e| format!("failed to start {label} loopback: {e}"))?;

    Ok(ActiveCapture {
        client,
        capture,
        sample_rate,
        channels,
        encoding,
        label,
        process_tree: process_virtual,
    })
}

unsafe fn read_buffer(
    data_ptr: *mut u8,
    frames: usize,
    channels: usize,
    encoding: SampleEncoding,
) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }

    let sample_count = frames * channels;

    match encoding {
        SampleEncoding::I16 => {
            let samples = std::slice::from_raw_parts(data_ptr as *const i16, sample_count);
            samples
                .chunks(channels)
                .map(|chunk| {
                    let sum: f32 = chunk.iter().map(|&s| s as f32 / i16::MAX as f32).sum();
                    sum / channels as f32
                })
                .collect()
        }
        SampleEncoding::F32 => {
            let samples = std::slice::from_raw_parts(data_ptr as *const f32, sample_count);
            samples
                .chunks(channels)
                .map(|chunk| {
                    let sum: f32 = chunk.iter().sum();
                    sum / channels as f32
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_activation_blob_matches_propvariant_abi() {
        assert_eq!(
            std::mem::size_of::<BlobPropVariant>(),
            std::mem::size_of::<PROPVARIANT>()
        );
        assert_eq!(
            std::mem::align_of::<BlobPropVariant>(),
            std::mem::align_of::<PROPVARIANT>()
        );
    }

    #[test]
    fn read_buffer_downmixes_i16_stereo() {
        let input = [i16::MAX, i16::MAX, i16::MIN, i16::MIN];
        let output = unsafe { read_buffer(input.as_ptr() as *mut u8, 2, 2, SampleEncoding::I16) };
        assert_eq!(output.len(), 2);
        assert!(output[0] > 0.99);
        assert!(output[1] <= -1.0);
    }

    async fn capture_peak(target_process_id: Option<u32>) -> (LoopbackStart, f32) {
        let (audio_tx, mut audio_rx) = mpsc::channel(256);
        let cancel = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = sync_mpsc::channel();
        let thread_cancel = Arc::clone(&cancel);
        let capture_thread = std::thread::spawn(move || {
            capture_loopback(
                audio_tx,
                thread_cancel,
                started_tx,
                target_process_id,
                Arc::new(AudioPressureCounters::default()),
            );
        });

        let started = started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("loopback should report startup")
            .expect("loopback should start");
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut peak = 0.0f32;
        while tokio::time::Instant::now() < deadline && peak <= 0.001 {
            if let Ok(Some(buffer)) =
                tokio::time::timeout(std::time::Duration::from_millis(500), audio_rx.recv()).await
            {
                if let AudioBuffer::Loopback(samples) = buffer {
                    peak = samples
                        .iter()
                        .fold(peak, |current, sample| current.max(sample.abs()));
                }
            }
        }

        cancel.store(true, Ordering::Relaxed);
        capture_thread.join().expect("capture thread should stop");
        (started, peak)
    }

    #[tokio::test]
    #[ignore = "requires live output audio"]
    async fn classic_loopback_captures_non_silent_audio() {
        let (started, peak) = capture_peak(None).await;
        assert!(!started.process_tree);
        assert!(started.sample_rate > 0);
        assert!(peak > 0.001, "expected non-silent classic-loopback audio");
    }

    #[tokio::test]
    #[ignore = "requires a live audio-producing PID in KIMINOLA_TEST_LOOPBACK_PID"]
    async fn process_loopback_captures_non_silent_audio() {
        let process_id: u32 = std::env::var("KIMINOLA_TEST_LOOPBACK_PID")
            .expect("KIMINOLA_TEST_LOOPBACK_PID is required")
            .parse()
            .expect("loopback PID must be a number");
        let (started, peak) = capture_peak(Some(process_id)).await;
        assert!(
            started.process_tree,
            "test must not pass through classic fallback"
        );
        assert!(started.sample_rate > 0);
        assert!(peak > 0.001, "expected non-silent process-loopback audio");
    }
}
