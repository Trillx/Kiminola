# Model distribution and management UX

Type: grilling
Status: resolved
Blocked by: 01

## Question

How do users get and manage local ASR models? Decide with the user (one question at a time), grounded in ticket 01's findings (model sizes, licenses, sources):

- First-run experience: prompted download vs bundled default model
- Model picker: sizes/quality tiers, disk/RAM cost display
- Download source (Hugging Face?), resume/integrity, storage location
- Model updates and multiple-model management

Feeds the spec's model-management section.

## Resolution

Resolved on 2026-08-13 after a one-question-at-a-time grilling session, grounded in ticket 01's model findings (Nemotron streaming 0.6B INT8, ~632 MB, HF-hosted, redistribution permitted).

### Decisions

- **First-run delivery**: the default model downloads on first run via the onboarding wizard (ticket 09) — no bundled model. Installer stays ~15–25 MB; model delivery decouples from app updates.
- **Model picker**: none at MVP. The app ships Nemotron streaming 0.6B EN as the single default; settings only shows what's installed (size, RAM while running). Multilingual Nemotron is the first post-MVP addition; the download path is model-agnostic, so future models are data, not re-architecture.
- **Download pipeline**: Hugging Face direct (pinned revision), SHA-256 verified after download (redownload on mismatch), HTTP range-request resume. Stored in `%LOCALAPPDATA%\Kiminola\models` — local, not roaming AppData, since it's a re-downloadable artifact.
- **Model updates**: the model manifest (URL, size, SHA-256) is pinned inside the app binary; new model versions arrive via app updates. On next launch after an update that changes the pinned model, the app re-downloads with progress, keeping the old model until the new one verifies.

### Consequences

- Multiple-model management is unneeded at MVP (collapsed by the no-picker decision).
- App installers and auto-updates never carry model blobs; a model re-download happens only when the pinned model version actually changes.
- A GitHub Releases mirror of the model is a config-level fallback if Hugging Face reliability ever becomes a problem.
