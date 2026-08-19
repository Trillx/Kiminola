# Accessible Meeting prompt mechanics

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: Accessible Meeting prompt behavior and wording

## Question

Which notification surface should carry the Meeting prompt so it is accessible and predictable? Decide whether to use a standard Windows notification/toast, an in-app overlay, or both; how keyboard and screen-reader users reach all actions; whether it remains in Notification Center; and how timeout/dismissal behaves without causing accidental recording.

## Resolution

Use a standard Windows notification/toast as the primary Meeting prompt, with the same pending choice visible inside Kiminola until the user acts. This gives the operating system a predictable notification surface while preserving an in-app fallback.

The exact keyboard, screen-reader, Notification Center, timeout, and dismissal mechanics remain open for a research ticket.
