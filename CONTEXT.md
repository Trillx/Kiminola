# Kiminola — Domain Glossary

Canonical language for the project. Implementation details do not belong here.

## Terms

- **Meeting** — a single recorded conversation the user captures with Kiminola. Has one capture session, one transcript, and one set of notes.
- **Capture session** — the active recording of a Meeting, from manual start to manual stop.
- **Mic channel** — the audio channel captured from the user's microphone. Labeled **"You"** in the transcript.
- **System channel** — the audio channel captured from system/loopback audio (what the meeting app plays). Labeled **"Others"** in the transcript.
- **Live transcript** — the incrementally-streaming text produced from both channels during a capture session, labeled by channel.
- **Notepad** — the user's own rough notes typed during a Meeting. Optional; enhancement works without it.
- **Note enhancement** — the post-meeting LLM pass that produces structured notes: a baseline summary from the transcript alone, merged with the Notepad contents when present.
- **Model pack** — a downloadable on-device ASR model the user installs to power transcription. Audio never leaves the machine.
- **Model manifest** — the embedded description of a Model pack: source repo, revision, file list, sizes, and verification hashes.
- **First-run wizard** — the mandatory onboarding flow a new user completes before accessing the library. Steps: microphone permission, Model pack download, optional AI Provider configuration.
- **Onboarding state** — the persisted record of which first-run wizard steps have been completed, used to gate access to the library and recording.
- **Provider** — a pluggable cloud LLM backend used for Note enhancement (e.g. OpenRouter, direct API keys). Receives transcript *text*, never audio.
