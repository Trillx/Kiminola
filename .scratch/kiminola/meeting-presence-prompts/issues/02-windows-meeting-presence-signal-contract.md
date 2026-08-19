# Windows meeting-presence signal contract

Type: research
Status: resolved
Claimed by: Research subagent
Blocked by: None

## Question

Which locally observable Windows signals can indicate likely meeting presence for Granola, Zoom, Microsoft Teams, Google Meet, Webex, and similar apps without reading meeting content or capturing audio?

Compare process and window detection, Windows Core Audio session activity, foreground-app state, calendar hooks, browser-tab signals, and any supported app APIs. Define confidence, permissions, false-positive behavior, support/version assumptions, and a safe fallback for unknown apps. The result must preserve the rule that a signal only produces a Meeting presence hint and never starts capture.

## Resolution

Use local evidence fusion as the baseline: enumerate known process/window families and observe Windows Core Audio session metadata mapped to process IDs, without opening a microphone or loopback stream. Foreground-window state is supporting evidence only. Calendar, browser-extension, and provider APIs remain optional, explicitly authorized enrichments.

The detector emits a coarse **possible** or **likely** hint with evidence labels; it never emits capture authority. It must not inspect PCM, transcripts, screen pixels, camera frames, full browser contents, or meeting-provider content. The initial implementation should require a separate product decision for when a hint is strong enough to notify.

Research asset: `research/windows-meeting-presence-signal-contract`, commit `811f137`.
