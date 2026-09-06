# Microsoft Graph read-only provider contract

Type: research
Status: resolved
Blocked by: None

## Question

What do Microsoft's first-party Microsoft Graph and identity documents support for Kimi Nola's read-only Outlook/Microsoft 365 desktop integration?

Cover the public-client desktop OAuth flow suitable for a Windows Tauri app, least-privilege calendar read scopes, refresh-token and consent/revocation behavior, account/calendar discovery, event listing across the current-day-plus-seven-day window, recurring-event expansion, all-day events, time zones, delta queries or ETags, pagination, throttling, and error states. Identify Entra app-registration, redirect, tenant, and testing requirements that affect an open-source Windows desktop app.

Recommend a provider contract using only primary sources. Capture the findings, citations, and unresolved risks in `research/microsoft-graph-read-only-provider-contract.md`; this ticket will receive the verified context pointer when resolved.

## Resolution

Microsoft's first-party documentation supports a delegated public-client authorization-code flow with PKCE through the system browser. The MVP should prefer a loopback callback (`http://localhost`, with the exact registered path and implementation-specific port behavior validated in the packaged app); the reviewed Microsoft documentation does not provide a Tauri-specific custom-scheme recipe. No client secret or certificate belongs in the desktop binary.

The least-privilege delegated scope for the initial contract is `Calendars.ReadBasic`, plus `offline_access` for background/open-app refresh. `User.Read` is optional only if the UI needs a Graph profile lookup. Discover calendars with the current `GET /me/calendars` endpoint, then query each selected calendar's `calendarView` over the explicit eight-date local window, using ISO 8601 offsets and `Prefer: outlook.timezone` with the Windows time-zone name. `calendarView` expands recurring occurrences and exceptions; all-day events remain display/link-only.

Use per-calendar `calendarView/delta` links for incremental refresh, following every `@odata.nextLink` and saving an opaque `@odata.deltaLink` only after the full page chain succeeds. Perform a full sync when the local window rolls forward, the selected calendar changes, or a delta cursor becomes unusable. Treat ETags/change keys as version hints, not sync cursors; respect `Retry-After` on HTTP 429 and preserve stale cache state after bounded retries.

The report identifies unresolved integration risks: the exact `Calendars.ReadBasic` field projection and delta behavior need work/school and personal-account testing; loopback registration must be validated in the packaged Tauri build; Microsoft account audience still needs a product decision (organizational-only versus organizational plus personal); and shared/delegated calendars would require a separate permission decision.

Research asset: [Microsoft Graph read-only provider contract](../research/microsoft-graph-read-only-provider-contract.md), verified on `feature/Calander-sync`; the report is uncommitted and contains no production-code changes.
