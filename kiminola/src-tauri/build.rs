#[path = "build_support/windows_runtime.rs"]
mod windows_runtime;

fn main() {
    // SQLx embeds these files. Rebuild when a migration is added or changed.
    println!("cargo:rerun-if-changed=migrations");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
    )
    .expect("failed to run tauri-build");

    // `muda` uses TaskDialogIndirect when Tauri's tray feature enables
    // Common Controls v6. Tauri embeds this dependency in the app manifest,
    // but Cargo's Rust test executables do not receive that resource. Embed
    // the same manifest in every Windows MSVC link target so the loader
    // resolves comctl32.dll v6 before any unit test starts.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Windows searches the executable directory before System32 and PATH.
        // PATH alone can therefore select an incompatible system ONNX Runtime
        // for Cargo tests, whose executables live in <profile>/deps.
        println!("cargo:rerun-if-changed=build_support/windows_runtime.rs");
        println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");
        let lib_dir = std::env::var_os("SHERPA_ONNX_LIB_DIR")
            .expect("run scripts/prepare-native-deps.ps1 before building on Windows");
        let out_dir = std::env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR");
        for path in windows_runtime::stage_test_runtime(
            std::path::Path::new(&out_dir),
            std::path::Path::new(&lib_dir),
        )
        .expect("failed to stage native DLLs beside Cargo test executables")
        {
            println!("cargo:rerun-if-changed={}", path.display());
        }

        let manifest =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
