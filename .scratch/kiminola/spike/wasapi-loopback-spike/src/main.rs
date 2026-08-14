//! Spike: WASAPI loopback capture on Windows ARM64 (Kiminola ticket 11)
//!
//! Modes:
//!   classic [seconds]        whole-endpoint loopback (AUDCLNT_STREAMFLAGS_LOOPBACK)
//!   process <pid> [seconds]  per-process loopback (ActivateAudioInterfaceAsync)
//!   genwav <path> [seconds]  write a 44.1 kHz 16-bit stereo test-tone WAV
//!
//! PASS criterion per mode: packets flow (GetNextPacketSize > 0 repeatedly)
//! AND captured data is non-silent (max amplitude above threshold).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{implement, w, HRESULT, IUnknown, Interface, Ref};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eConsole, eRender, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, BLOB, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("classic");

    match mode {
        "genwav" => {
            let path = args.get(2).map(String::as_str).unwrap_or("tone.wav");
            let secs: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
            gen_wav(path, secs);
            return Ok(());
        }
        "classic" => {
            let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
            let client = activate_classic_loopback()?;
            run_capture(&client, secs, "classic", false)?;
        }
        "process" => {
            let pid: u32 = args
                .get(2)
                .and_then(|s| s.parse().ok())
                .expect("usage: process <pid> [seconds]");
            let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8);
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
            let client = activate_process_loopback(pid)?;
            run_capture(&client, secs, &format!("process(pid={pid})"), true)?;
        }
        other => {
            eprintln!("unknown mode: {other}");
            std::process::exit(2);
        }
    }
    Ok(())
}

/// Classic whole-render-endpoint loopback.
fn activate_classic_loopback() -> windows::core::Result<IAudioClient> {
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let state = device.GetState()?;
        println!("[classic] default render endpoint state: {:?}", state);
        device.Activate(CLSCTX_ALL, None)
    }
}

type ActivateResult = Arc<Mutex<Option<(HRESULT, Option<IUnknown>)>>>;

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct CompletionHandler {
    event: HANDLE,
    result: ActivateResult,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for CompletionHandler_Impl {
    fn ActivateCompleted(
        &self,
        op: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let mut hr = HRESULT::default();
        let mut unk: Option<IUnknown> = None;
        unsafe { op.ok()?.GetActivateResult(&mut hr, &mut unk)? };
        *self.result.lock().unwrap() = Some((hr, unk));
        unsafe { SetEvent(self.event)? };
        Ok(())
    }
}

/// Per-process(-tree) loopback via ActivateAudioInterfaceAsync.
fn activate_process_loopback(pid: u32) -> windows::core::Result<IAudioClient> {
    let params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };

    // PROPVARIANT wrapping the activation params as a VT_BLOB (per MS sample).
    let mut pv = PROPVARIANT::default();
    pv.Anonymous.Anonymous = std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
        vt: VT_BLOB,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: PROPVARIANT_0_0_0 {
            blob: BLOB {
                cbSize: std::mem::size_of_val(&params) as u32,
                pBlobData: &params as *const _ as *mut u8,
            },
        },
    });

    let event = unsafe { CreateEventW(None, true, false, None)? };
    let result: ActivateResult = Arc::new(Mutex::new(None));
    let handler: IActivateAudioInterfaceCompletionHandler = CompletionHandler {
        event,
        result: result.clone(),
    }
    .into();

    let _op = unsafe {
        ActivateAudioInterfaceAsync(
            w!("VAD\\Process_Loopback"),
            &IAudioClient::IID,
            Some(&pv as *const PROPVARIANT),
            &handler,
        )?
    };
    let wait = unsafe { WaitForSingleObject(event, 15_000) };
    if wait != WAIT_OBJECT_0 {
        eprintln!("process-loopback activation timed out");
        std::process::exit(3);
    }

    let (hr, unk) = result.lock().unwrap().take().expect("no activation result");
    println!("[process] async activation completed, hr=0x{:08x}", hr.0 as u32);
    hr.ok()?;
    let unk = unk.expect("activation returned no interface");
    let client = unk.cast::<IAudioClient>()?;

    // Now that activation is done, defang the PROPVARIANT so its Drop never
    // tries to free the stack-resident blob.
    pv.Anonymous.Anonymous = std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
        vt: windows::Win32::System::Variant::VT_EMPTY,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: PROPVARIANT_0_0_0 {
            blob: BLOB {
                cbSize: 0,
                pBlobData: std::ptr::null_mut(),
            },
        },
    });
    Ok(client)
}

/// Shared capture loop: poll GetNextPacketSize/GetBuffer for `secs`, verify flow.
fn run_capture(
    client: &IAudioClient,
    secs: u64,
    label: &str,
    process_virtual: bool,
) -> windows::core::Result<()> {
    // The virtual process-loopback client returns E_NOTIMPL from GetMixFormat.
    // Microsoft's ApplicationLoopback sample supplies this PCM format and asks
    // the audio engine to convert to it. Physical endpoints still use their
    // actual mix format.
    let process_format = process_virtual.then_some(WAVEFORMATEX {
        wFormatTag: 1, // WAVE_FORMAT_PCM
        nChannels: 2,
        nSamplesPerSec: 44_100,
        nAvgBytesPerSec: 44_100 * 4,
        nBlockAlign: 4,
        wBitsPerSample: 16,
        cbSize: 0,
    });
    let (pwfx, free_mix_format) = if let Some(format) = process_format.as_ref() {
        (format as *const WAVEFORMATEX as *mut WAVEFORMATEX, false)
    } else {
        (unsafe { client.GetMixFormat()? }, true)
    };
    let (rate, channels, bits, tag) = unsafe {
        let f = &*pwfx;
        (f.nSamplesPerSec, f.nChannels, f.wBitsPerSample, f.wFormatTag)
    };
    println!(
        "[{label}] mix format: {rate} Hz, {channels} ch, {bits} bit, tag=0x{tag:04x}{}",
        if tag == 0xfffe { " (extensible)" } else { "" }
    );
    let is_float = tag == 3 || tag == 0xfffe; // extensible on render endpoints is float32

    let init = unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK
                | if process_virtual {
                    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                } else {
                    Default::default()
                },
            10_000_000, // 1 s buffer, hns (matches MS loopback sample)
            0,
            pwfx,
            None,
        )
    };
    if let Err(e) = init {
        eprintln!("[{label}] IAudioClient::Initialize failed: {e:?}");
        return Err(e);
    }
    let capture: IAudioCaptureClient = match unsafe { client.GetService() } {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[{label}] GetService<IAudioCaptureClient> failed: {e:?}");
            return Err(e);
        }
    };
    if let Err(e) = unsafe { client.Start() } {
        eprintln!("[{label}] IAudioClient::Start failed: {e:?}");
        return Err(e);
    }
    println!("[{label}] capture started");

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut packets = 0u64;
    let mut silent_packets = 0u64;
    let mut frames_total = 0u64;
    let mut max_amp = 0.0f32;

    while Instant::now() < deadline {
        let packet_frames = unsafe { capture.GetNextPacketSize()? };
        if packet_frames == 0 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut num_frames: u32 = 0;
        let mut flags: u32 = 0;
        unsafe { capture.GetBuffer(&mut data, &mut num_frames, &mut flags, None, None)? };

        packets += 1;
        frames_total += num_frames as u64;
        if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
            silent_packets += 1;
        } else if !data.is_null() && num_frames > 0 {
            let n = num_frames as usize * channels as usize;
            if is_float && bits == 32 {
                let samples = unsafe { std::slice::from_raw_parts(data as *const f32, n) };
                for &s in samples.iter().step_by(3) {
                    max_amp = max_amp.max(s.abs());
                }
            } else if bits == 16 {
                let samples = unsafe { std::slice::from_raw_parts(data as *const i16, n) };
                for &s in samples.iter().step_by(3) {
                    max_amp = max_amp.max((s as f32 / 32768.0).abs());
                }
            }
        }
        unsafe { capture.ReleaseBuffer(num_frames)? };
    }
    unsafe { client.Stop()? };
    if free_mix_format {
        unsafe { CoTaskMemFree(Some(pwfx as *const _)) };
    }

    let pass = packets > 0 && max_amp > 0.001 && silent_packets < packets;
    println!(
        "[{label}] packets={packets} frames={frames_total} silent_packets={silent_packets} max_amp={max_amp:.4}"
    );
    println!(
        "[{label}] {}",
        if pass {
            "PASS: loopback packets flowed with non-silent audio"
        } else {
            "FAIL: no packets or all-silent data (zero-packet bug / nothing playing?)"
        }
    );
    if !pass {
        std::process::exit(1);
    }
    Ok(())
}

/// Write a 44.1 kHz 16-bit stereo WAV with a two-tone signal (440 + 660 Hz).
fn gen_wav(path: &str, secs: u32) {
    let rate = 44_100u32;
    let frames = rate * secs;
    let mut pcm = Vec::with_capacity(frames as usize * 4);
    for i in 0..frames {
        let t = i as f32 / rate as f32;
        let s = 0.45 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            + 0.25 * (2.0 * std::f32::consts::PI * 660.0 * t).sin();
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&v.to_le_bytes()); // L
        pcm.extend_from_slice(&v.to_le_bytes()); // R
    }
    let data_len = pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // stereo
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes()); // byte rate
    out.extend_from_slice(&4u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&pcm);
    std::fs::write(path, &out).expect("write wav");
    println!("wrote {path} ({secs} s, 44.1 kHz stereo 16-bit)");
}
