# Calendar refresh and freshness policy

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: None

## Question

What exact freshness policy should govern Calendar sync while the main window or background companion is active?

Lock the periodic refresh interval, manual-refresh behavior, current-day-plus-seven-day window, provider incremental-sync versus full-refresh fallback, recurrence expansion, Windows-local time comparisons, stale-state transitions, offline behavior, token-expiry handling, throttling/backoff, and whether removed or deselected events disappear from upcoming views. Linked event snapshots must remain stable even when the live cache changes.

## Resolution

Calendar sync follows the existing Background companion lifecycle without creating a second aggressive poller:

- Sync starts when the app or companion activates, then runs every 15 minutes with small per-calendar jitter. A **Refresh now** action refreshes all selected calendars independently and reports each result.
- A cache is fresh for 30 minutes after its last successful refresh. After that it is stale; stale, offline, or needs-reauthorization data remains visible but cannot enrich a new Meeting prompt. Token expiry/revocation moves the calendar to needs-reauthorization immediately while preserving its local data.
- A full refresh runs on first connection, calendar selection, local-date-window rollover, or an unusable provider cursor. Normal cycles use the provider's delta/sync-token mechanism. The settled window remains the current local day plus the next seven days, with recurring timed occurrences expanded and all-day events display/link-only.
- On a date-window rollover, the previous cache remains visible as stale until the new full refresh succeeds; then the new cache replaces it atomically. A failed request receives three total attempts with exponential backoff, provider `Retry-After`, jitter, and no overlapping refreshes.
- Each selected calendar commits only after its complete page/delta chain succeeds. A failed calendar retains its prior cache and its own error state without blocking healthy calendars.
- Provider-cancelled or deleted events are removed from the live cache after a successful sync. Any explicit Meeting link remains as unavailable historical context. Deselecting a calendar follows the domain decision's explicit retain-or-erase choice.
