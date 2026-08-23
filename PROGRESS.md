# Kimi Nola improvement progress

Append one concise line per committed iteration: date, change, reason, and validation result.

- 2026-08-22 — Blocked tray Quit during active recording so in-memory transcript and notes cannot be discarded accidentally; restored and focused the recording view with an accessible explanation. PASS: 40 Rust tests, 8 frontend tests, Rust check, Svelte check (0 errors/warnings), and production build.
- 2026-08-22 — Continuously checkpointed recording notes into an existing SQLite note draft so handwritten notes survive crashes and save failures; serialized writes prevent stale overwrites and intentional cancel cleans up auto-created drafts. PASS: 40 Rust tests, 11 frontend tests, Rust check, Svelte check (0 errors/warnings), and production build.
