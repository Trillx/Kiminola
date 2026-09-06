//! Startup coordination: a bounded, diagnosable guard that keeps a failed or
//! hung first launch from blocking later launches forever. The coordinator
//! waits on a named Windows mutex with a bounded timeout; owned and abandoned
//! acquisitions are coordinated through an RAII lease held across Tauri
//! build/setup, while timeout, creation, and wait failures degrade explicitly
//! to the Tauri single-instance plugin with a local diagnostic. Nothing here
//! may panic in production.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Opaque OS handle for the startup mutex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MutexHandle(pub isize);

/// What the bounded OS wait returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WaitOutcome {
    /// The mutex was acquired cleanly.
    Owned,
    /// The previous owner exited without releasing; we now own it.
    Abandoned,
    /// The bounded wait elapsed while another process held the mutex.
    Timeout,
    /// The wait itself failed (Win32 error text).
    Failed(String),
}

/// The OS boundary behind startup coordination, injected so tests can script
/// every wait outcome deterministically.
pub(crate) trait StartupMutex {
    fn create(&self, name: &str) -> Result<MutexHandle, String>;
    fn wait(&self, handle: &MutexHandle, timeout: Duration) -> WaitOutcome;
    fn release(&self, handle: &MutexHandle);
    fn close(&self, handle: MutexHandle);
}

/// Production policy: a 10-second bounded wait on the app startup mutex.
pub(crate) struct StartupCoordinationConfig {
    pub mutex_name: String,
    pub wait_timeout: Duration,
}

impl Default for StartupCoordinationConfig {
    fn default() -> Self {
        Self {
            mutex_name: r"Local\com.kiminola.app-startup-lock".to_string(),
            wait_timeout: Duration::from_secs(10),
        }
    }
}

/// Why startup coordination degraded, with enough context to diagnose locally.
#[derive(Debug)]
pub(crate) struct StartupDiagnostic {
    pub operation: &'static str,
    pub mutex_name: String,
    pub result: String,
    pub timeout: Duration,
    pub elapsed: Duration,
}

impl fmt::Display for StartupDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "startup coordination degraded: {} failed for {}: {} (elapsed {:?}; bounded wait {:?}); \
             continuing with the single-instance plugin fallback",
            self.operation, self.mutex_name, self.result, self.elapsed, self.timeout
        )
    }
}

fn diagnostic_log_path() -> PathBuf {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("Kiminola")
            .join("logs")
            .join("startup-coordination.log");
    }

    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir)
        .join("logs")
        .join("startup-coordination.log")
}

fn write_diagnostic_file(path: &Path, diagnostic: &StartupDiagnostic) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{diagnostic}")
}

/// Persist a degraded-startup diagnostic where GUI-subsystem launches can be
/// inspected later. Logging is best-effort and must never block app fallback.
pub(crate) fn emit_local_diagnostic(diagnostic: &StartupDiagnostic) {
    eprintln!("{diagnostic}");
    let path = diagnostic_log_path();
    if let Err(error) = write_diagnostic_file(&path, diagnostic) {
        eprintln!(
            "could not persist startup coordination diagnostic to {}: {error}",
            path.display()
        );
    }
}

/// RAII ownership of the startup mutex, held across Tauri build/setup and
/// released (then closed) on drop before the event loop starts.
pub(crate) struct StartupLease<'a> {
    handle: MutexHandle,
    adapter: &'a dyn StartupMutex,
}

impl Drop for StartupLease<'_> {
    fn drop(&mut self) {
        self.adapter.release(&self.handle);
        self.adapter.close(self.handle);
    }
}

/// The coordination decision for this launch.
pub(crate) enum StartupCoordination<'a> {
    /// Owned (or abandoned-then-owned) mutex; holds the lease until dropped.
    Coordinated(StartupLease<'a>),
    /// Bounded failure; the single-instance plugin remains the backstop.
    Degraded(StartupDiagnostic),
}

pub(crate) fn coordinate_startup<'a>(
    adapter: &'a dyn StartupMutex,
    config: &StartupCoordinationConfig,
) -> StartupCoordination<'a> {
    let started = Instant::now();
    let handle = match adapter.create(&config.mutex_name) {
        Ok(handle) => handle,
        Err(result) => {
            return StartupCoordination::Degraded(StartupDiagnostic {
                operation: "create startup mutex",
                mutex_name: config.mutex_name.clone(),
                result,
                timeout: config.wait_timeout,
                elapsed: started.elapsed(),
            });
        }
    };

    match adapter.wait(&handle, config.wait_timeout) {
        WaitOutcome::Owned | WaitOutcome::Abandoned => {
            StartupCoordination::Coordinated(StartupLease { handle, adapter })
        }
        WaitOutcome::Timeout => {
            adapter.close(handle);
            StartupCoordination::Degraded(StartupDiagnostic {
                operation: "wait for startup mutex",
                mutex_name: config.mutex_name.clone(),
                result: format!("timed out after {:?}", config.wait_timeout),
                timeout: config.wait_timeout,
                elapsed: started.elapsed(),
            })
        }
        WaitOutcome::Failed(result) => {
            adapter.close(handle);
            StartupCoordination::Degraded(StartupDiagnostic {
                operation: "wait for startup mutex",
                mutex_name: config.mutex_name.clone(),
                result,
                timeout: config.wait_timeout,
                elapsed: started.elapsed(),
            })
        }
    }
}

/// The production Win32 adapter. Non-Windows builds get a no-op adapter so
/// `cargo check` keeps working off-platform; the app targets Windows only.
#[cfg(target_os = "windows")]
pub(crate) struct WindowsStartupMutex;

#[cfg(target_os = "windows")]
impl StartupMutex for WindowsStartupMutex {
    fn create(&self, name: &str) -> Result<MutexHandle, String> {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::System::Threading::CreateMutexW;

        let wide = HSTRING::from(name);
        unsafe { CreateMutexW(None, false, PCWSTR(wide.as_ptr())) }
            .map(|handle| MutexHandle(handle.0 as isize))
            .map_err(|error| format!("CreateMutexW failed: {error}"))
    }

    fn wait(&self, handle: &MutexHandle, timeout: Duration) -> WaitOutcome {
        use windows::Win32::Foundation::{HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::WaitForSingleObject;

        let millis = timeout.as_millis().min(u32::MAX as u128) as u32;
        let result = unsafe { WaitForSingleObject(HANDLE(handle.0 as *mut _), millis) };
        if result == WAIT_OBJECT_0 {
            WaitOutcome::Owned
        } else if result == WAIT_ABANDONED {
            WaitOutcome::Abandoned
        } else if result == WAIT_TIMEOUT {
            WaitOutcome::Timeout
        } else {
            WaitOutcome::Failed(format!(
                "WaitForSingleObject returned {result:?}: {}",
                windows::core::Error::from_win32()
            ))
        }
    }

    fn release(&self, handle: &MutexHandle) {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Threading::ReleaseMutex;

        let _ = unsafe { ReleaseMutex(HANDLE(handle.0 as *mut _)) };
    }

    fn close(&self, handle: MutexHandle) {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};

        let _ = unsafe { CloseHandle(HANDLE(handle.0 as *mut _)) };
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) struct WindowsStartupMutex;

#[cfg(not(target_os = "windows"))]
impl StartupMutex for WindowsStartupMutex {
    fn create(&self, _name: &str) -> Result<MutexHandle, String> {
        Ok(MutexHandle(0))
    }

    fn wait(&self, _handle: &MutexHandle, _timeout: Duration) -> WaitOutcome {
        WaitOutcome::Owned
    }

    fn release(&self, _handle: &MutexHandle) {}

    fn close(&self, _handle: MutexHandle) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Scripts one create/wait outcome and records release/close calls and the
    /// timeout the coordinator asked for.
    struct FakeMutex {
        create: Result<isize, String>,
        wait: WaitOutcome,
        released: AtomicUsize,
        closed: AtomicUsize,
        wait_timeout_seen: Mutex<Option<Duration>>,
    }

    impl StartupMutex for FakeMutex {
        fn create(&self, _name: &str) -> Result<MutexHandle, String> {
            self.create.clone().map(MutexHandle)
        }

        fn wait(&self, _handle: &MutexHandle, timeout: Duration) -> WaitOutcome {
            *self.wait_timeout_seen.lock().unwrap() = Some(timeout);
            self.wait.clone()
        }

        fn release(&self, _handle: &MutexHandle) {
            self.released.fetch_add(1, Ordering::SeqCst);
        }

        fn close(&self, _handle: MutexHandle) {
            self.closed.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn fake(create: Result<isize, String>, wait: WaitOutcome) -> FakeMutex {
        FakeMutex {
            create,
            wait,
            released: AtomicUsize::new(0),
            closed: AtomicUsize::new(0),
            wait_timeout_seen: Mutex::new(None),
        }
    }

    #[cfg(target_os = "windows")]
    struct NativeProbePaths {
        root: PathBuf,
        ready: PathBuf,
        signal: PathBuf,
    }

    #[cfg(target_os = "windows")]
    struct NativeProbeChild {
        child: Option<std::process::Child>,
        signal: PathBuf,
    }

    #[cfg(target_os = "windows")]
    impl NativeProbeChild {
        fn signal_and_wait(mut self) {
            std::fs::write(&self.signal, b"release")
                .expect("probe release signal should be written");
            self.wait_for_success();
        }

        fn wait_for_abandonment(mut self) {
            self.wait_for_success();
        }

        fn wait_for_success(&mut self) {
            let status = self
                .child
                .take()
                .expect("probe child should still be running")
                .wait()
                .expect("probe child should be waitable");
            assert!(status.success(), "probe child failed with {status}");
        }
    }

    #[cfg(target_os = "windows")]
    impl Drop for NativeProbeChild {
        fn drop(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = std::fs::write(&self.signal, b"cleanup");
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn unique_native_probe(label: &str) -> (String, NativeProbePaths) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiminola-startup-probe-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("probe directory should be created");
        let paths = NativeProbePaths {
            ready: root.join("ready"),
            signal: root.join("signal"),
            root,
        };
        let mutex_name = format!(
            r"Local\com.kiminola.startup-probe.{label}.{}.{unique}",
            std::process::id()
        );
        (mutex_name, paths)
    }

    #[cfg(target_os = "windows")]
    fn spawn_native_probe(
        role: &str,
        mutex_name: &str,
        paths: &NativeProbePaths,
    ) -> NativeProbeChild {
        let child = std::process::Command::new(
            std::env::current_exe().expect("test executable path should be available"),
        )
        .arg("--exact")
        .arg("startup_coordination::tests::native_probe_child")
        .arg("--ignored")
        .arg("--nocapture")
        .env("KIMINOLA_STARTUP_PROBE_ROLE", role)
        .env("KIMINOLA_STARTUP_PROBE_MUTEX", mutex_name)
        .env("KIMINOLA_STARTUP_PROBE_READY", &paths.ready)
        .env("KIMINOLA_STARTUP_PROBE_SIGNAL", &paths.signal)
        .spawn()
        .expect("native probe child should start");
        NativeProbeChild {
            child: Some(child),
            signal: paths.signal.clone(),
        }
    }

    #[cfg(target_os = "windows")]
    fn wait_for_probe(ready: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(
                Instant::now() < deadline,
                "native probe did not become ready within five seconds"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(target_os = "windows")]
    fn cleanup_native_probe(paths: NativeProbePaths) {
        std::fs::remove_dir_all(paths.root).expect("probe directory should be removable");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "child process fixture; run through native probe parent tests"]
    fn native_probe_child() {
        let Ok(role) = std::env::var("KIMINOLA_STARTUP_PROBE_ROLE") else {
            return;
        };
        let mutex_name = std::env::var("KIMINOLA_STARTUP_PROBE_MUTEX")
            .expect("probe mutex name should be provided");
        let ready = PathBuf::from(
            std::env::var_os("KIMINOLA_STARTUP_PROBE_READY")
                .expect("probe ready path should be provided"),
        );
        let signal = PathBuf::from(
            std::env::var_os("KIMINOLA_STARTUP_PROBE_SIGNAL")
                .expect("probe signal path should be provided"),
        );
        let adapter = WindowsStartupMutex;
        let config = StartupCoordinationConfig {
            mutex_name,
            wait_timeout: Duration::from_secs(3),
        };
        let coordination = coordinate_startup(&adapter, &config);
        assert!(matches!(coordination, StartupCoordination::Coordinated(_)));
        std::fs::write(ready, b"ready").expect("probe ready marker should be written");

        let deadline = Instant::now() + Duration::from_secs(10);
        while !signal.exists() {
            assert!(
                Instant::now() < deadline,
                "probe parent did not signal within ten seconds"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        match role.as_str() {
            "hold" => drop(coordination),
            "abandon" => std::process::exit(0),
            other => panic!("unknown native probe role: {other}"),
        }
    }

    #[test]
    fn owned_wait_is_coordinated_and_lease_releases_then_closes_on_drop() {
        let adapter = fake(Ok(7), WaitOutcome::Owned);
        let config = StartupCoordinationConfig::default();

        let coordination = coordinate_startup(&adapter, &config);
        assert!(matches!(coordination, StartupCoordination::Coordinated(_)));
        assert_eq!(adapter.released.load(Ordering::SeqCst), 0);

        drop(coordination);
        assert_eq!(adapter.released.load(Ordering::SeqCst), 1);
        assert_eq!(adapter.closed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn abandoned_wait_is_still_coordinated() {
        let adapter = fake(Ok(9), WaitOutcome::Abandoned);
        let config = StartupCoordinationConfig::default();

        let coordination = coordinate_startup(&adapter, &config);
        assert!(
            matches!(coordination, StartupCoordination::Coordinated(_)),
            "an abandoned mutex is owned by us now and must be coordinated"
        );
    }

    #[test]
    fn timeout_degrades_with_diagnostic_and_closes_without_release() {
        let adapter = fake(Ok(11), WaitOutcome::Timeout);
        let config = StartupCoordinationConfig::default();

        let coordination = coordinate_startup(&adapter, &config);
        match coordination {
            StartupCoordination::Degraded(diagnostic) => {
                assert_eq!(diagnostic.operation, "wait for startup mutex");
                assert_eq!(diagnostic.mutex_name, config.mutex_name);
                assert_eq!(diagnostic.timeout, config.wait_timeout);
                assert!(diagnostic.result.contains("timed out"));
            }
            StartupCoordination::Coordinated(_) => panic!("timeout must degrade"),
        }
        assert_eq!(adapter.released.load(Ordering::SeqCst), 0);
        assert_eq!(adapter.closed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn create_failure_degrades_without_panicking() {
        let adapter = fake(Err("access denied (5)".into()), WaitOutcome::Owned);
        let config = StartupCoordinationConfig::default();

        let coordination = coordinate_startup(&adapter, &config);
        match coordination {
            StartupCoordination::Degraded(diagnostic) => {
                assert_eq!(diagnostic.operation, "create startup mutex");
                assert!(diagnostic.result.contains("access denied"));
            }
            StartupCoordination::Coordinated(_) => panic!("create failure must degrade"),
        }
        assert_eq!(adapter.released.load(Ordering::SeqCst), 0);
        assert_eq!(adapter.closed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn wait_failure_degrades_without_panicking() {
        let adapter = fake(
            Ok(13),
            WaitOutcome::Failed("WAIT_FAILED: the handle is invalid (6)".into()),
        );
        let config = StartupCoordinationConfig::default();

        let coordination = coordinate_startup(&adapter, &config);
        match coordination {
            StartupCoordination::Degraded(diagnostic) => {
                assert_eq!(diagnostic.operation, "wait for startup mutex");
                assert!(diagnostic.result.contains("WAIT_FAILED"));
            }
            StartupCoordination::Coordinated(_) => panic!("wait failure must degrade"),
        }
        assert_eq!(adapter.closed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn default_config_binds_production_mutex_name_and_ten_second_bound() {
        let config = StartupCoordinationConfig::default();
        assert_eq!(config.mutex_name, r"Local\com.kiminola.app-startup-lock");
        assert_eq!(config.wait_timeout, Duration::from_secs(10));

        let adapter = fake(Ok(15), WaitOutcome::Owned);
        let _ = coordinate_startup(&adapter, &config);
        assert_eq!(
            *adapter.wait_timeout_seen.lock().unwrap(),
            Some(Duration::from_secs(10)),
            "the coordinator must pass the bounded timeout to the OS wait"
        );
    }

    #[test]
    fn degraded_diagnostic_reports_operation_mutex_result_and_timeout() {
        let diagnostic = StartupDiagnostic {
            operation: "wait for startup mutex",
            mutex_name: r"Local\com.kiminola.app-startup-lock".into(),
            result: "timed out after 10s".into(),
            timeout: Duration::from_secs(10),
            elapsed: Duration::from_millis(125),
        };
        let rendered = diagnostic.to_string();
        assert!(rendered.contains("wait for startup mutex"));
        assert!(rendered.contains(r"Local\com.kiminola.app-startup-lock"));
        assert!(rendered.contains("timed out after 10s"));
        assert!(rendered.contains("10s"));
        assert!(rendered.contains("125ms"));
        assert!(rendered.contains("single-instance plugin"));
    }

    #[test]
    fn degraded_diagnostic_is_written_to_a_local_file() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kiminola-startup-diagnostic-{}-{unique}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let diagnostic = StartupDiagnostic {
            operation: "wait for startup mutex",
            mutex_name: r"Local\com.kiminola.app-startup-lock".into(),
            result: "WAIT_FAILED: invalid handle (6)".into(),
            timeout: Duration::from_secs(10),
            elapsed: Duration::from_millis(2),
        };

        write_diagnostic_file(&path, &diagnostic).expect("diagnostic should be persisted");

        let contents = std::fs::read_to_string(&path).expect("diagnostic file should be readable");
        assert!(contents.contains("wait for startup mutex"));
        assert!(contents.contains("WAIT_FAILED: invalid handle (6)"));
        assert!(contents.contains("single-instance plugin"));
        std::fs::remove_file(path).expect("temporary diagnostic should be removable");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_holder_forces_a_bounded_timeout() {
        let (mutex_name, paths) = unique_native_probe("timeout");
        let holder = spawn_native_probe("hold", &mutex_name, &paths);
        wait_for_probe(&paths.ready);

        let adapter = WindowsStartupMutex;
        let config = StartupCoordinationConfig {
            mutex_name,
            wait_timeout: Duration::from_millis(200),
        };
        let started = Instant::now();
        let coordination = coordinate_startup(&adapter, &config);
        let wall_elapsed = started.elapsed();

        match coordination {
            StartupCoordination::Degraded(diagnostic) => {
                assert!(diagnostic.result.contains("timed out"));
                assert_eq!(diagnostic.timeout, Duration::from_millis(200));
            }
            StartupCoordination::Coordinated(_) => panic!("a live holder must force timeout"),
        }
        assert!(wall_elapsed >= Duration::from_millis(150));
        assert!(wall_elapsed < Duration::from_secs(2));
        holder.signal_and_wait();
        cleanup_native_probe(paths);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_abandoned_owner_transfers_the_mutex_lease() {
        let (mutex_name, paths) = unique_native_probe("abandoned");
        let holder = spawn_native_probe("abandon", &mutex_name, &paths);
        wait_for_probe(&paths.ready);

        let exit_signal = paths.signal.clone();
        let signaler = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            std::fs::write(exit_signal, b"exit").expect("probe exit signal should be written");
        });
        let adapter = WindowsStartupMutex;
        let config = StartupCoordinationConfig {
            mutex_name,
            wait_timeout: Duration::from_secs(3),
        };

        let coordination = coordinate_startup(&adapter, &config);

        signaler.join().expect("probe signaler should finish");
        assert!(matches!(coordination, StartupCoordination::Coordinated(_)));
        drop(coordination);
        holder.wait_for_abandonment();
        cleanup_native_probe(paths);
    }
}
