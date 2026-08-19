# Detection confidence and prompt threshold

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: Windows meeting-presence signal contract

## Question

When should Kiminola interrupt the user with a Meeting prompt? Decide whether one weak local signal creates a quiet possible state while two independent local signals—such as a recognized meeting-app process plus an active Core Audio session—are required for the visible notification.

## Resolution

One weak local signal remains a quiet **possible** state. Kiminola may show a visible Meeting prompt only when two independent local signals agree—for example, a recognized meeting-app process/window family plus an active Core Audio session mapped to that family. Even then, the result is still a question and never capture authority.
