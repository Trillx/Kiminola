Type: research
Status: resolved

## Question

How does the configured Windows NSIS updater preserve Kimi Nola meeting data and model files, and what runtime checks are still required?

## Answer

The configured current-user NSIS install keeps the application under `%LOCALAPPDATA%\\Kimi Nola`, while the database and ASR model remain under `%LOCALAPPDATA%\\Kiminola\\data` and `%LOCALAPPDATA%\\Kiminola\\models`. The updater uses Tauri's passive Windows mode, which launches the NSIS installer with update and restart arguments after the app has exited. No installer hook was added that deletes user data.

This is a layout/configuration conclusion, not a runtime claim. The release runbook requires a baseline marker, update, one restart, SQLite integrity/content checks, model-file hash checks, and separate x64 and ARM64 runs before publication.
