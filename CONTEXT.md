# Kiminola — Domain Glossary

Canonical language for the project. Implementation details do not belong here.

## Terms

- **Meeting** — a single recorded conversation the user captures with Kiminola. Has one capture session, one transcript, and one set of notes.
- **Space** — an organizational library container. Spaces may contain nested Spaces and Meetings.
- **Meeting hierarchy** — the organizational parent/child relationship between Meetings. A child Meeting remains a separate recording with its own transcript and notes; hierarchy does not roll content up.
- **Library location** — the single direct container assigned to a Meeting: either a Space or another Meeting, never both.
- **Capture session** — the active recording of a Meeting, from manual start to manual stop.
- **Mic channel** — the audio channel captured from the user's microphone. Labeled **"You"** in the transcript.
- **System channel** — the audio channel captured from system/loopback audio (what the meeting app plays). Labeled **"Others"** in the transcript.
- **Live transcript** — the incrementally-streaming text produced from both channels during a capture session, labeled by channel.
- **Note draft** — a notes-only artifact created when the user chooses Jot notes from a Meeting prompt. It has no capture session or audio and can be attached to a Meeting only after the user explicitly starts recording.
- **Notepad** — the user's own rough notes typed in a Note draft or during a Meeting. Optional; enhancement works without it.
- **Meeting presence hint** — a local, non-authoritative signal that a supported meeting application or its audio activity suggests the user may be in a meeting. It is not a Capture session and never records audio by itself.
- **Meeting prompt** — a user-visible notification shown after a Meeting presence hint, offering a choice such as opening a Notepad or starting a Capture session. It never starts capture without explicit confirmation.
- **Companion layout** — a temporary side-by-side arrangement used when the user starts a Capture session from a Meeting prompt: the meeting application remains visible alongside Kimi Nola's Notepad. It is a starting arrangement, not a locked layout; the user may resize or reposition the windows.
- **Background companion** — Kimi Nola's resident mode after the main window closes, limited to local Meeting presence hints and prompts. It never captures audio and ends only when the user explicitly quits or disables it.
- **Note enhancement** — the post-meeting LLM pass that produces structured notes: a baseline summary from the transcript alone, merged with the Notepad contents when present.
- **Model pack** — a downloadable on-device ASR model the user installs to power transcription. Audio never leaves the machine.
- **Model manifest** — the embedded description of a Model pack: source repo, revision, file list, sizes, and verification hashes.
- **First-run wizard** — the mandatory onboarding flow a new user completes before accessing the library. Steps: microphone permission, Model pack download, optional AI Provider configuration.
- **Onboarding state** — the persisted record of which first-run wizard steps have been completed, used to gate access to the library and recording.
- **Provider** — a pluggable cloud LLM backend used for Note enhancement (e.g. OpenRouter, direct API keys). Receives transcript *text*, never audio.
