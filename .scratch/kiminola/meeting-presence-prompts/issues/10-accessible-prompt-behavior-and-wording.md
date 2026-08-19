# Accessible Meeting prompt behavior and wording

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: Focus-aware and accessible Meeting prompt

## Question

What keyboard, screen-reader, contrast, timeout, and notification-center behavior must the Meeting prompt support? Decide the final plain-language wording so the user understands that Kiminola only detected a possibility, no audio is being recorded, and each action's consequence is explicit.

## Resolution

The default wording should communicate uncertainty and recording status directly:

> You may be in a meeting. Want to jot notes?

The prompt offers **Jot notes**, **Start recording**, and **Not now**, and explicitly states that Kiminola is not recording until the user chooses an action that starts a Meeting.

Keyboard, screen-reader, contrast, timeout, and notification-center behavior remain open for a follow-on ticket.
