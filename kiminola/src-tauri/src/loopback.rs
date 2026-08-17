use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as sync_mpsc;
use std::sync::Arc;

use tokio::sync::mpsc;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVEFORMATEXTENSIBLE,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::Sleep;

use crate::recording_session::AudioBuffer;

const REFTIMES_PER_MILLISEC: i64 = 10000;
const BUFFER_DURATION_100NS: i64 = 100 * REFTIMES_PER_MILLISEC; // 100 ms

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Captures the default render endpoint in loopback mode and sends mono f32
/// buffers to the supplied channel until `cancel` is set.
///
/// `started_tx` is signaled once with the loopback sample rate once the client is
/// initialized and running, or with an error if initialization fails.
pub fn capture_loopback(
    audio_tx: mpsc::Sender<AudioBuffer>,
    cancel: Arc<AtomicBool>,
    started_tx: sync_mpsc::Sender<Result<u32, String>>,
) {
    let result = unsafe { capture_loopback_inner(&audio_tx, &cancel, &started_tx) };
    let _ = started_tx.send(result);
}

unsafe fn capture_loopback_inner(
    audio_tx: &mpsc::Sender<AudioBuffer>,
    cancel: &Arc<AtomicBool>,
    started_tx: &sync_mpsc::Sender<Result<u32, String>>,
) -> Result<u32, String> {
    CoInitializeEx(None, COINIT_MULTITHREADED)
        .ok()
        .map_err(|e| format!("COM init failed: {e}"))?;

    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        .map_err(|e| format!("failed to create device enumerator: {e}"))?;

    let device = enumerator
        .GetDefaultAudioEndpoint(eRender, eConsole)
        .map_err(|e| format!("no default render endpoint: {e}"))?;

    let client: IAudioClient = device
        .Activate(CLSCTX_ALL, None)
        .map_err(|e| format!("failed to activate audio client: {e}"))?;

    let format_ptr = client
        .GetMixFormat()
        .map_err(|e| format!("failed to get mix format: {e}"))?;
    if format_ptr.is_null() {
        return Err("GetMixFormat returned null".into());
    }

    client
        .Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            BUFFER_DURATION_100NS,
            0,
            format_ptr,
            None,
        )
        .map_err(|e| format!("failed to initialize loopback client: {e}"))?;

    let capture: IAudioCaptureClient = client
        .GetService()
        .map_err(|e| format!("failed to get capture client: {e}"))?;

    client
        .Start()
        .map_err(|e| format!("failed to start loopback: {e}"))?;

    let channels = (*format_ptr).nChannels as usize;
    let format_tag = (*format_ptr).wFormatTag;
    let sample_rate = (*format_ptr).nSamplesPerSec;
    CoTaskMemFree(Some(format_ptr as *const _ as *const _));

    let _ = started_tx.send(Ok(sample_rate));

    while !cancel.load(Ordering::Relaxed) {
        let packet_length = match capture.GetNextPacketSize() {
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
        if let Err(e) = capture.GetBuffer(
            &mut data_ptr,
            &mut frames_available,
            &mut flags,
            None,
            None,
        ) {
            eprintln!("loopback GetBuffer failed: {e}");
            break;
        }

        if !data_ptr.is_null() && frames_available > 0 {
            let samples = read_buffer(data_ptr, frames_available as usize, channels, format_tag);
            if !samples.is_empty() {
                let _ = audio_tx.try_send(AudioBuffer::Loopback(samples));
            }
        }

        if let Err(e) = capture.ReleaseBuffer(frames_available) {
            eprintln!("loopback ReleaseBuffer failed: {e}");
            break;
        }
    }

    let _ = client.Stop();
    Ok(sample_rate)
}

unsafe fn read_buffer(
    data_ptr: *mut u8,
    frames: usize,
    channels: usize,
    format_tag: u16,
) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }

    let sample_count = frames * channels;

    if format_tag == WAVE_FORMAT_PCM {
        // Assume 16-bit integer PCM.
        let samples = std::slice::from_raw_parts(data_ptr as *const i16, sample_count);
        samples
            .chunks(channels)
            .map(|chunk| {
                let sum: f32 = chunk.iter().map(|&s| s as f32 / i16::MAX as f32).sum();
                sum / channels as f32
            })
            .collect()
    } else if format_tag == WAVE_FORMAT_IEEE_FLOAT || format_tag == WAVE_FORMAT_EXTENSIBLE {
        let _ext = &*(data_ptr as *const WAVEFORMATEXTENSIBLE);
        // TODO: inspect SubFormat to choose f32 vs i16; default to f32 for now.
        let samples = std::slice::from_raw_parts(data_ptr as *const f32, sample_count);
        samples
            .chunks(channels)
            .map(|chunk| {
                let sum: f32 = chunk.iter().sum();
                sum / channels as f32
            })
            .collect()
    } else {
        // Unsupported format: drop the packet.
        Vec::new()
    }
}
