fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        ),
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
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
