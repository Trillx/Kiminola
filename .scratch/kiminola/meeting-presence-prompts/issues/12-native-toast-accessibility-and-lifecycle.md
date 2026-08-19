# Native toast accessibility and lifecycle

Type: research
Status: resolved
Claimed by: Research subagent
Blocked by: Accessible Meeting prompt mechanics

## Question

What do official Windows notification/toast APIs guarantee for keyboard navigation, screen readers, contrast, action buttons, Notification Center persistence, expiration, and dismissal? Determine the app identity/registration requirements and the safest way to keep the in-app pending prompt synchronized without allowing a stale toast action to start recording unexpectedly.

## Resolution

Keep a canonical in-app pending prompt and use the native Windows toast as a secondary surface. Every toast action carries a validated prompt identity; body clicks open or restore the pending prompt, and only a valid explicit **Start recording** action can enter the recording state. Expired, dismissed, superseded, unknown, or missing identities are rejected as recording actions.

The in-app prompt remains functional when notifications are disabled, routed to Notification Center, suppressed by Do Not Disturb, unavailable, or blocked by packaging/elevation constraints. A Windows notification registration and NSIS x64/ARM64 packaging spike is still required before implementation.

Research asset: `research/native-toast-accessibility-and-lifecycle`, commit `47dceb0`.
