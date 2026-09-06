# Google Calendar read-only provider contract

Type: research
Status: resolved
Blocked by: None

## Question

What do Google's first-party Calendar and OAuth documents support for Kimi Nola's read-only desktop integration?

Cover the installed/desktop OAuth flow suitable for a Windows Tauri app, least-privilege read-only scopes, refresh-token and revocation behavior, account/calendar discovery, event listing across the current-day-plus-seven-day window, recurring-event expansion, all-day events, time zones, incremental sync tokens or ETags, pagination, rate limits, and error states. Identify Google Cloud app-verification/testing requirements that affect an open-source BYOK-style desktop app.

Recommend a provider contract using only primary sources. Capture the findings, citations, and unresolved risks in `research/google-calendar-read-only-provider-contract.md` on a throwaway `research/google-calendar-read-only-provider-contract` branch; this ticket will receive the context pointer when resolved.

## Resolution

Google's first-party documentation supports a public Desktop OAuth client using the system browser, a loopback callback on `127.0.0.1` with a random port, PKCE/S256, and a fresh state value. The read-only contract should request `calendar.calendarlist.readonly` plus `calendar.events.readonly`; optional `openid email` scopes may provide a stable account identity and display label.

Calendar discovery uses `calendarList.list`; event retrieval uses `events.list` with `singleEvents=true`, `showDeleted=true`, pagination, and a minimal projection containing identity, title, start/end, recurrence linkage, update time, and ETag. Google sync tokens cannot be combined with `timeMin`/`timeMax`, so the bounded current-day-plus-seven-day cache requires a full refresh when the local date window rolls forward and after a `410`, with incremental sync between those points. ETags are only an optimization, not the sync cursor. All-day events remain display/link-only, and provider fields such as attendees, descriptions, locations, and join URLs stay outside the projection.

The report also identifies Google sensitive-scope verification, seven-day Testing-project refresh expiry, quota/backoff, Windows-to-IANA timezone mapping, private/availability-only calendars, and official-build-versus-fork client identity as launch risks.

Research asset: throwaway branch `research/google-calendar-read-only-provider-contract`, commit `e6c8c89fb658f268c35eb06ee9ee248aa508924f` (verified; report contains only the cited research file).
