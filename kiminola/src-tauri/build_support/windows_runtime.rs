use std::io;
use std::path::{Path, PathBuf};

pub const RUNTIME_DLLS: [&str; 4] = [
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll",
];

/// Put the selected native package beside Cargo's test/bench executables.
/// Sherpa already stages app and example executables, but omits `deps`.
/// Return both inputs and outputs so Cargo repairs missing/stale staged DLLs.
pub fn stage_test_runtime(out_dir: &Path, lib_dir: &Path) -> io::Result<Vec<PathBuf>> {
    // Cargo supplies <output>/<profile>/build/<package-hash>/out, including
    // --target, CARGO_TARGET_DIR, and custom profiles. Do not guess from cwd.
    let build_dir = out_dir.ancestors().nth(2);
    let profile_dir = out_dir.ancestors().nth(3);
    if out_dir.file_name() != Some(std::ffi::OsStr::new("out"))
        || build_dir.and_then(Path::file_name) != Some(std::ffi::OsStr::new("build"))
        || profile_dir.is_none()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unrecognized Cargo OUT_DIR: {}", out_dir.display()),
        ));
    }
    let deps_dir = profile_dir.unwrap().join("deps");

    // Validate the complete package before writing any destination files.
    let sources = RUNTIME_DLLS
        .iter()
        .map(|name| {
            let source = lib_dir.join(name);
            let bytes = std::fs::read(&source).map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", source.display()))
            })?;
            Ok((source, deps_dir.join(name), bytes))
        })
        .collect::<io::Result<Vec<_>>>()?;
    std::fs::create_dir_all(&deps_dir)?;
    let mut watched = Vec::new();
    for (source, destination, bytes) in sources {
        // Avoid rewriting loaded DLLs or changing output mtimes needlessly.
        if std::fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
            std::fs::write(&destination, &bytes).map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", destination.display()))
            })?;
        }
        watched.extend([source, destination]);
    }
    Ok(watched)
}
