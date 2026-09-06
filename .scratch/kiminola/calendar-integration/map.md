# Kimi Nola — Wayfinder Map: Calendar integration

## Destination

A build-ready post-MVP specification for opt-in, read-only Google Calendar and Microsoft 365/Outlook integration. Users select calendars; Kimi Nola displays upcoming events, uses fresh events to enrich existing local Meeting presence prompts, and lets users explicitly associate an event with a saved Meeting without ever writing to the provider calendar.

## Notes

- **Domain**: local-first meeting transcription and notes. Maintain the root `CONTEXT.md` via `/domain-modeling`.
- **Skills every session should consult**: `/grilling`, `/domain-modeling`, `/research`, and `/prototype` when the ticket calls for them.
- This is a new post-MVP effort; the historical MVP map remains unchanged.
- Ask the owner one decision at a time.
- **Settled during charting**: multiple accounts are allowed per provider; the user explicitly selects calendars; sync is independently opt-in; the cache covers the current day plus the next seven days; refresh runs when the app is open or the background companion is enabled, with a manual refresh action; event times use the Windows device's local time zone; recurring timed events may participate while all-day events are display/link-only; event association requires confirmation; the event title is an editable initial Meeting title; linked event data is an immutable minimal snapshot; stale data is display-only and cannot enrich a new prompt; OAuth is read-only and tokens belong in the Windows credential store.

## Decisions so far

- [Google Calendar read-only provider contract](issues/01-google-calendar-read-only-provider-contract.md) — Use a public Desktop OAuth client through the system browser and loopback PKCE flow with read-only calendar-list/event scopes; use incremental sync between bounded full refreshes, including a full refresh when the local date window rolls forward or a sync token expires.
- [Microsoft Graph read-only provider contract](issues/02-microsoft-graph-read-only-provider-contract.md) — Use delegated public-client PKCE through the system browser with `Calendars.ReadBasic` plus `offline_access`; use `calendarView` for the bounded window and per-calendar delta links between full refreshes, with packaged loopback and account-audience testing still required.
- [Calendar domain and storage lifecycle](issues/03-calendar-domain-and-storage-lifecycle.md) — Separate provider accounts, selected calendars, replaceable eight-day event cache, and zero-or-one durable Meeting snapshots; preserve stale data through token loss, make deletion an explicit user choice, and never backfill or send calendar data to Note enhancement.
- [Calendar refresh and freshness policy](issues/04-calendar-refresh-and-freshness-policy.md) — Refresh on activation and every 15 minutes with jitter; use bounded full refreshes at window boundaries and provider deltas otherwise; keep stale caches through failures, retry three times with backoff, and commit each calendar atomically.
- [Event-aware Meeting prompt contract](issues/05-event-aware-meeting-prompt-contract.md) — Use fresh time-window context only alongside the existing two-signal local detector; show one event or require a chooser, keep uncertainty explicit, link only on confirmed recording, and fall back to the generic prompt when no fresh event exists.
- [Calendar surfaces and flow prototype](issues/06-calendar-surfaces-and-flow-prototype.md) — Adopt A · Home rail: extend the existing Home surface with upcoming events and connection status; keep provider management in Settings, event context in the prompt, and the snapshot secondary on Meeting detail.

## Not yet specified

<!-- no unresolved fog; the remaining open child task is the provider test setup prerequisite -->

## Out of scope

- Creating, editing, deleting, or otherwise writing calendar events; full two-way calendar synchronization.
- Reading or persisting attendee lists, descriptions, locations, join URLs, or meeting-provider content.
- Copying calendar history beyond the current day plus seven days into the cache.
- Calendar-only prompts, silent event association, or automatic audio capture.
- A full replacement calendar application, mobile support, or non-Windows implementation in this effort.
