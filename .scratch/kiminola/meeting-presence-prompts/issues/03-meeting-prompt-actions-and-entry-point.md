# Meeting prompt actions and entry point

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: None

## Question

When a Meeting prompt appears, what should the user be able to do immediately? Decide whether the primary action opens a blank Notepad, starts an explicitly confirmed Meeting, offers both actions equally, or uses another entry point. Define what happens when the user dismisses the prompt and whether a Notepad can later be converted into a Meeting without losing notes.

## Resolution

The Meeting prompt offers three explicit choices:

- **Jot notes** opens a Notepad without recording audio.
- **Start recording** begins a Meeting only after the user clicks it.
- **Not now** dismisses the prompt.

Jot notes is the primary action; Start recording is secondary. The lifecycle and relationship between a prompt-created Notepad and a later Meeting are deferred to a follow-on ticket.
