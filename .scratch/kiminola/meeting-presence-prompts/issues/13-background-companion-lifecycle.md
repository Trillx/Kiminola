# Background companion lifecycle

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: None

## Question

Should closing the Kimi Nola window hide the UI while a lightweight background companion continues local meeting-presence detection and notifications? Define the separate explicit Quit action that stops the companion, and confirm that neither window close nor background residency may start audio capture.

## Resolution

Closing the Kimi Nola window hides the UI while the Background companion remains available for local Meeting presence hints and notifications. An explicit **Quit** action fully exits the companion and stops notifications. Neither window close nor background residency starts audio capture.
