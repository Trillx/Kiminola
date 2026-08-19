# Detector metadata retention

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: Prompt app-identity privacy

## Question

Should detector metadata remain in memory only for the current hint/prompt and be discarded when the prompt resolves, the app session ends, or the companion quits? Decide whether any detector history or diagnostics may persist, and how the privacy boundary is communicated in Settings.

## Resolution

Detector metadata is memory-only for the current hint and prompt. It is discarded when the prompt resolves, the app session ends, or the Background companion quits. No detector history is persisted. Diagnostics may retain only aggregate health/error information and must not contain paths, URLs, window titles, calendar/provider metadata, audio, transcripts, or meeting content.
