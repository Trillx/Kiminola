# Event-aware Meeting prompt contract

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: None

## Question

How should a fresh Calendar event enrich the existing local Meeting presence detector and prompt without becoming capture authority?

Decide how active events are matched to local process/window plus Core Audio evidence, how competing candidate events are shown, the timing tolerance around event start/end, prompt wording and actions, explicit event confirmation, editable initial titles, stale-action protection, suppression/repeat behavior, and the behavior when no event or only stale event data exists. Reuse the resolved decisions in `.scratch/kiminola/meeting-presence-prompts/` rather than reopening them; the calendar event alone must never prompt or start recording.

## Resolution

Calendar context enriches the existing local Meeting presence prompt without becoming a new detection authority:

- A Calendar event is eligible only when it is fresh, timed, and within 15 minutes before its start through 15 minutes after its end. Matching uses the time window only; the existing local app/window plus Core Audio evidence remains independent and mandatory. All-day events and stale events do not enrich prompts.
- If exactly one event candidate overlaps, show it directly. If multiple candidates overlap, require an event chooser rather than guessing. The candidate line is separate from the existing uncertain wording: “You may be in a meeting. Want to jot notes?” plus “Possible event: [title] · [time].” The prompt continues to state that Kimi Nola is not recording.
- The existing actions remain Jot notes, Start recording, and Not now. Start recording is the explicit confirmation that links the displayed/selected event and uses its title as the editable initial Meeting title. Jot notes remains a standalone Note draft with no calendar link; Not now follows the existing suppression rules.
- The prompt and candidate are revalidated at action time. If the event is stale, cancelled, removed, or no longer available, the user may explicitly start without a calendar event or cancel; Kimi Nola never silently links stale context.
- If no fresh candidate exists, show the existing generic Meeting prompt without calendar context. Event changes never bypass the resolved one-prompt-per-app-session suppression, snooze, or app-ignore behavior.
