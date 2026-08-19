# Initial app coverage and unknown apps

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: Windows meeting-presence signal contract, Detection confidence and prompt threshold

## Question

Which meeting-app families should receive first-class recognition in the initial implementation—Granola, Zoom, Microsoft Teams, Google Meet, Webex, or another set? Decide how an unknown app with two local signals behaves, including whether it may show a generic “possible meeting” prompt or remains quiet until the user explicitly enables it.

## Resolution

Initial first-class recognition covers **Granola**, **Zoom**, **Microsoft Teams**, **Google Meet**, and **Webex**. An unknown app may follow the same two-signal path and show a generic possible-meeting prompt; it does not require a provider API or app-specific integration before the user can act.

The privacy-preserving app identity shown in that prompt remains open for a follow-on ticket.
