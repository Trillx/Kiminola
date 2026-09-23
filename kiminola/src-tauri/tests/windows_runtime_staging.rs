#[path = "../build_support/windows_runtime.rs"]
mod windows_runtime;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_runtime::{stage_test_runtime, RUNTIME_DLLS};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "kiminola-runtime-staging-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(root.join("native")).unwrap();
        for name in RUNTIME_DLLS {
            fs::write(root.join("native").join(name), name.as_bytes()).unwrap();
        }
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn stages_complete_runtime_beside_tests_for_cargo_output_layouts() {
    let fixture = Fixture::new();
    // OUT_DIR carries Cargo's actual target directory, target triple and profile.
    // It need not contain a directory named "target", "debug", or "release".
    for layout in [
        "target/debug",
        "target/release",
        "custom cargo output/aarch64-pc-windows-msvc/debug",
        "custom cargo output/x86_64-pc-windows-msvc/release",
        "custom cargo output/aarch64-pc-windows-msvc/ci-test",
    ] {
        let profile = fixture.0.join(layout);
        let out_dir = profile.join("build/kiminola-abc123/out");
        let watched = stage_test_runtime(&out_dir, &fixture.0.join("native")).unwrap();
        for name in RUNTIME_DLLS {
            let source = fixture.0.join("native").join(name);
            let staged = profile.join("deps").join(name);
            assert_eq!(fs::read(&staged).unwrap(), fs::read(&source).unwrap());
            assert!(watched.contains(&source), "watch selected runtime inputs");
            assert!(
                watched.contains(&staged),
                "repair deleted/stale staged DLLs"
            );
            assert!(!profile.join(name).exists(), "leave app staging to sherpa");
        }
        assert_eq!(watched.len(), RUNTIME_DLLS.len() * 2);
    }
}

#[test]
fn replaces_stale_dlls_but_does_not_rewrite_identical_files() {
    let fixture = Fixture::new();
    let profile = fixture.0.join("target/debug");
    let out_dir = profile.join("build/kiminola-abc123/out");
    let source = fixture.0.join("native");
    stage_test_runtime(&out_dir, &source).unwrap();
    let staged = profile.join("deps/onnxruntime.dll");
    let sentinel = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    fs::File::options()
        .write(true)
        .open(&staged)
        .unwrap()
        .set_modified(sentinel)
        .unwrap();
    stage_test_runtime(&out_dir, &source).unwrap();
    assert_eq!(fs::metadata(&staged).unwrap().modified().unwrap(), sentinel);
    fs::write(&staged, b"stale ONNX Runtime").unwrap();
    stage_test_runtime(&out_dir, &source).unwrap();
    assert_eq!(fs::read(&staged).unwrap(), b"onnxruntime.dll");
    fs::remove_file(&staged).unwrap();
    stage_test_runtime(&out_dir, &source).unwrap();
    assert_eq!(fs::read(&staged).unwrap(), b"onnxruntime.dll");
}

#[test]
fn fails_before_staging_if_selected_package_is_incomplete() {
    let fixture = Fixture::new();
    let profile = fixture.0.join("target/debug");
    let source = fixture.0.join("native");
    fs::remove_file(source.join("sherpa-onnx-cxx-api.dll")).unwrap();
    let error = stage_test_runtime(&profile.join("build/kiminola-abc123/out"), &source)
        .expect_err("an incomplete package must not fall back to system DLLs");
    assert!(error.to_string().contains("sherpa-onnx-cxx-api.dll"));
    assert!(!profile.join("deps").exists());
}

#[test]
fn rejects_unrecognized_out_dir_instead_of_writing_elsewhere() {
    let fixture = Fixture::new();
    for malformed in [
        "out",
        "target/debug/not-build/kiminola/out",
        "target/debug/build/kiminola/not-out",
    ] {
        assert!(stage_test_runtime(&fixture.0.join(malformed), &fixture.0.join("native")).is_err());
    }
}
