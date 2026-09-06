# Calendar domain and storage lifecycle

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: None

## Question

After the provider contracts are known, what local domain model should represent Calendar accounts, Selected calendars, current event cache entries, and Linked event snapshots without confusing external Calendar events with Kimi Nola Meetings?

Decide the relationship and uniqueness rules between providers, accounts, calendars, event instances, and Meetings; how recurring instances are identified; what minimal fields and freshness/error state belong in SQLite; how explicit linking survives event edits/deletions; how account disconnect, calendar deselection, token loss, and cache deletion behave; and how the migration preserves existing Meetings and privacy boundaries. Secrets must remain outside SQLite.

## Resolution

The local model separates provider authorization, selected calendars, the replaceable event cache, and durable Meeting links:

- A Meeting has zero or one Linked event snapshot. The user can explicitly link, unlink, or replace it; existing Meetings are never backfilled automatically.
- A Calendar event is identified by provider, Calendar account identity, Selected calendar identity, and external event-instance identity. Matching title/time values from different calendars remain separate. Recurring series are expanded into distinct occurrence events, and a link targets the exact occurrence with an optional series reference.
- The live event cache retains only the current local day plus the next seven days. Rows are replaceable and may be removed when they leave the window or the provider reports deletion. A link is an independent snapshot and does not retarget when the provider event changes or disappears.
- A Linked event snapshot retains provider, account identity, calendar identity, external event-instance identity, title, start time, and end time. The UI displays only the minimal provider/title/time information; attendees, descriptions, locations, join URLs, and other provider content are excluded.
- Each Selected calendar owns its enabled state, last attempted refresh, last successful refresh, fresh/stale/needs-reauthorization/error state, and safe retry category. Account status aggregates its calendars; one failed calendar does not hide healthy calendars.
- Credentials and sync authority are removed on disconnect. The user then chooses either to retain already-synced data as visible stale, non-refreshing local history or to erase the cache and linked snapshots. Deselecting one calendar follows the same two-choice rule.
- Token expiry or revocation is non-destructive: preserve local data as stale, stop new prompts, and request reconnection. Data is erased only through the explicit user choice.
- The migration is additive and has no historical event matching. Calendar-derived data remains local and is never sent to Note enhancement. Secrets remain in the Windows credential store, never SQLite.
