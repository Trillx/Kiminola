# Focus-aware and accessible Meeting prompt

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: Prompt trust, suppression, and repeat behavior

## Question

When should Kiminola hold or suppress a Meeting prompt because the user is presenting, in a full-screen app, or otherwise focused? Decide the accessible notification behavior, keyboard/screen-reader actions, and final wording that makes clear the hint is uncertain and no audio is being recorded until the user explicitly starts a Meeting.

## Resolution

Kiminola delays the Meeting prompt while Windows is in full-screen or presentation mode and shows it when the user's focus returns. Detection is not permanently suppressed; only the interruption is deferred.

Accessibility behavior and final wording remain open for a follow-on ticket.
