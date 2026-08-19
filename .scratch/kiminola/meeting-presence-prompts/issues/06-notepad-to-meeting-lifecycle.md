# Notepad-to-Meeting lifecycle

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: Meeting prompt actions and entry point

## Question

When the user chooses **Jot notes**, should Kiminola create a standalone Notepad, a Meeting placeholder, or another object? Decide how the user later starts recording from that surface, whether existing notes remain attached to the resulting Meeting, and what happens when the user never starts a recording.

## Resolution

**Jot notes** creates a standalone **Note draft**, not a Meeting placeholder. It has no capture session or audio. If the user later explicitly starts recording from that Note draft, Kiminola creates the Meeting and carries the existing notes into it. If recording never starts, the Note draft remains notes-only.

The persistence, library placement, naming, and discard behavior of Note drafts are deferred to a follow-on ticket.
