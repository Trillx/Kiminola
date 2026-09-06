# Provider test setup and packaged OAuth validation

Type: task
Status: open
Blocked by: None

## Question

What provider app registrations, test identities, and packaged-build checks must be provisioned before calendar implementation can be validated end to end?

This is a maintainer-in-the-loop setup task. Do not place secrets, refresh tokens, or private credentials in the repository.

Checklist:

- **Google**: create or designate separate test and production Google Cloud projects; enable Calendar API; configure the Desktop OAuth client and sensitive-scope consent/verification path for `calendar.calendarlist.readonly` and `calendar.events.readonly`; record the official client ID location outside the repository; provide at least one test account with timed, recurring, all-day, cancelled, overlapping, and DST-boundary events.
- **Microsoft**: create or designate separate development and production Entra app registrations supporting both work/school and personal Microsoft accounts; add the Mobile and desktop applications platform and the exact loopback redirect used by the packaged Tauri app; enable public-client flows; grant delegated `Calendars.ReadBasic` and `offline_access`; provide work/school and personal test identities, including an admin-consent case.
- **Packaged validation**: test official x64 and ARM64 Windows artifacts, system-browser sign-in, loopback callback, multiple accounts, calendar selection, refresh/delta rollover, stale/reconnect behavior, disconnect keep/delete choices, and no-secret/no-calendar-data-to-LLM boundaries.
- **Record the result**: document only non-secret setup facts, client-ID/configuration locations, test-account roles, redirect behavior, and any provider limitations needed by implementation. Keep all private keys, client secrets if a provider ever requires them, and refresh tokens in approved local/credential-store locations.

Resolve this task when the test prerequisites and validation matrix are available to the implementation work, not when production calendar code is complete.
