# Kiminola - Wayfinder Map: Meeting Presence Prompts

## Destination

A build-ready post-MVP spec for a privacy-preserving Meeting Prompt on Windows. Kiminola recognizes a likely active supported meeting app or meeting audio session, shows a non-recording notification inviting the user to jot notes or explicitly start a Meeting, and never captures audio without confirmation.

## Notes

- **Domain**: local-first meeting transcription and notes. Maintain the root `CONTEXT.md` via `/domain-modeling`.
- **Skills**: use `/grilling` for HITL decisions, `/domain-modeling` for vocabulary, `/research` for Windows/app facts, and `/prototype` for notification behavior.
- This is a new post-MVP effort; the historical MVP map remains unchanged.
- A Meeting presence hint is evidence that a meeting may be happening, not proof. A Meeting prompt never starts capture by itself.
- Ask the owner one decision at a time.

## Decisions so far

- [Meeting presence prompts are post-MVP and never auto-start](issues/01-meeting-presence-prompt-scope.md) - detect a likely meeting and notify the user; recording still requires an explicit action.
- [Windows meeting-presence signal contract](issues/02-windows-meeting-presence-signal-contract.md) - fuse local process/window and Core Audio session evidence; optional cloud/browser/calendar signals remain opt-in.
- [Meeting prompt actions and entry point](issues/03-meeting-prompt-actions-and-entry-point.md) - offer Jot notes, Start recording, or Not now; Jot notes is primary and recording remains explicit.
- [Notepad-to-Meeting lifecycle](issues/06-notepad-to-meeting-lifecycle.md) - Jot notes creates a standalone Note draft; explicit later recording carries those notes into a Meeting.
- [Note draft persistence and library behavior](issues/07-note-draft-persistence-and-library.md) - Note drafts live in the same library as Meetings with a distinct type.
- [Note draft saving, naming, and discard](issues/08-note-draft-saving-and-discard.md) - drafts autosave, survive restarts, receive a date/time title, and are never silently deleted.
- [Prompt trust, suppression, and repeat behavior](issues/04-prompt-trust-suppression-and-repeat-behavior.md) - one prompt per app session; Not now suppresses the session, with optional snooze and per-app ignore.
- [Focus-aware and accessible Meeting prompt](issues/09-focus-aware-and-accessible-prompt.md) - defer the prompt during full-screen or presentation mode and show it when focus returns.
- [Accessible Meeting prompt behavior and wording](issues/10-accessible-prompt-behavior-and-wording.md) - use explicit uncertainty, recording-status, and action wording.
- [Accessible Meeting prompt mechanics](issues/11-accessible-prompt-mechanics.md) - use a native Windows toast plus an in-app pending fallback.
- [Native toast accessibility and lifecycle](issues/12-native-toast-accessibility-and-lifecycle.md) - the in-app prompt is canonical and stale toast actions cannot start recording.
- [Background companion lifecycle](issues/13-background-companion-lifecycle.md) - closing the window hides Kimi Nola; explicit Quit exits the companion and stops notifications.
- [Background companion startup and controls](issues/14-background-companion-startup-and-controls.md) - residency is opt-in, Start with Windows is a toggle, and the tray exposes Pause, Open, and Quit.
- [Background companion status visibility](issues/15-background-companion-status.md) - the tray/Settings state is Detecting locally · not recording, Paused, or Off.
- [Detection confidence and prompt threshold](issues/16-detection-confidence-and-prompt-threshold.md) - one signal stays quiet; two independent local signals are required for a visible prompt.
- [Initial app coverage and unknown apps](issues/18-initial-app-coverage-and-unknown-apps.md) - recognize Granola, Zoom, Teams, Meet, and Webex; unknown apps may use the generic two-signal path.
- [Prompt app-identity privacy](issues/19-prompt-app-identity-privacy.md) - known apps use only friendly labels; unknown detections remain generic and sensitive metadata is not exposed or persisted.
- [Detector metadata retention](issues/20-detector-metadata-retention.md) - detector metadata is memory-only; diagnostics contain no sensitive identifiers or meeting content.

## Not yet specified

- Installed NSIS x64/ARM64 identity and terminated-process activation for the optional native toast bridge. The ARM64 NSIS install/start sanity check passed, but native toast remains unproven and best-effort; see [issue 17](issues/17-windows-toast-bridge-packaging-spike.md).

## Implementation status

- Implemented in the Tauri app: opt-in local detector, tray/background lifecycle,
  close-to-background behavior, stale-action-gated prompt actions, persisted
  Note drafts, and explicit recording handoff.
- Native toast registration is attempted when a prompt is raised. If the
  current unpackaged NSIS identity rejects WinRT toast registration, Kimi Nola
  reveals the in-app prompt as the actionable fallback. Packaging identity and
  terminated-process activation remain the next toast-specific verification.

## Out of scope

- Silent or automatic audio capture.
- Sending audio to meeting providers or using provider APIs to read meeting content.
