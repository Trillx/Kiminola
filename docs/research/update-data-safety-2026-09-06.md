# Update migration and data safety

Date: 2026-09-06  
Scope: local main at 576b178a9ececde87ad387292b1d754b0d003f01, app version 0.1.2. Source audit and synthetic SQL checks. This does not establish what is installed or published.

## Assessment

Normal installed updates should preserve already-saved data because it lives outside the application installation directory. Database migrations have transaction and history checks. The missing protections are a guaranteed save before updater shutdown, a pre-migration backup with a recovery path, and automated upgrade/failure testing against populated older databases.

No application code or real user data was changed during this investigation.

## What happens during an update

1. The app checks the stable release feed. Download and install require user action. The install caller checks whether the recording route is active before download and again before installation. It does not await pending editor saves. See [update.svelte.ts](../../kiminola/src/lib/update.svelte.ts#L166), [UpdateBanner.svelte](../../kiminola/src/lib/components/UpdateBanner.svelte#L10), and [update-policy.ts](../../kiminola/src/lib/update-policy.ts#L1).
2. Tauri validates the downloaded updater artifact against the embedded public key. The app uses current-user NSIS packaging and passive update installation. See [configuration](../../kiminola/src-tauri/tauri.conf.json#L44) and [official updater documentation](https://v2.tauri.app/plugin/updater/).
3. Windows installation exits the old process, replaces application files, and restarts the app. Normal data roots are separate from the default installation directory. No custom installer hooks are configured. See [configuration](../../kiminola/src-tauri/tauri.conf.json#L44) and [Tauri Windows installer documentation](https://v2.tauri.app/distribute/windows-installer/).
4. On launch, the app opens the existing SQLite database and applies pending embedded migrations. Setup starts a background warm-up; commands share the same initialization through a OnceCell. The pool is returned only after migrations succeed. See [db.rs initialization](../../kiminola/src-tauri/src/db.rs#L35) and [setup](../../kiminola/src-tauri/src/db.rs#L1627).

| Data | Persistent location | Update behavior |
| --- | --- | --- |
| Meetings, notes, transcripts, drafts, settings and templates | %LOCALAPPDATA%\Kiminola\data\kiminola.db | Existing database is opened and migrated in place. |
| ASR models | %LOCALAPPDATA%\Kiminola\models\nemotron | Outside installer directory; reuse depends on the new app's model manifest. |
| Provider API key | Windows keyring, service kiminola, account provider_api_key | Separate from SQLite and app files; not included in a database backup. |

Sources: [database path](../../kiminola/src-tauri/src/db.rs#L20), [model path](../../kiminola/src-tauri/src/models.rs#L48), [keyring identity](../../kiminola/src-tauri/src/llm.rs#L17). If LOCALAPPDATA is unavailable, the database/model code has an executable-relative fallback. The normal installed-layout conclusion does not establish safety for custom portable replacement procedures.

## Migration protections

The locked SQLx version is 0.8.6. It records applied versions and checksums in _sqlx_migrations, skips already-applied scripts, and rejects mismatched or missing migration history. Each SQLite migration and its successful-history entry commit in the same transaction. A failed SQL script rolls back its transaction. Earlier successful scripts in the same upgrade remain committed; the whole upgrade is not one transaction. See the pinned [SQLite migration implementation](https://raw.githubusercontent.com/launchbadge/sqlx/v0.8.6/sqlx-sqlite/src/migrate.rs) and [migration runner](https://raw.githubusercontent.com/launchbadge/sqlx/v0.8.6/sqlx-core/src/migrate/migrator.rs).

The app has nine migration files. They initialize the schema, add enhanced notes/templates/search/drafts/recovery fields, establish library hierarchy, and seed a stable default-space ID. Migration 0007 files older unassigned meetings into Personal. Migration 0004 intentionally replaces built-in template prompts; custom templates are excluded. See the [migration directory](../../kiminola/src-tauri/migrations).

## Findings, in priority order

### 1. Pending edits can be lost when installing an update

Meeting notes and standalone draft notes use a 500 ms autosave delay. Install update only tests whether the current route is /record, then calls candidate.install(). It neither flushes those timers nor waits for writes already running. When an update is already downloaded, editing a note and immediately installing can close the app before that edit becomes durable.

Evidence: [meeting notes autosave](../../kiminola/src/routes/meeting/[id]/+page.svelte#L323), [draft autosave](../../kiminola/src/routes/note/[id]/+page.svelte#L41), [install call](../../kiminola/src/lib/update.svelte.ts#L166), [updater plugin registration](../../kiminola/src-tauri/src/lib.rs#L26). This is a source-confirmed race, not an installed-app reproduction.

Recommended fix: introduce one update preparation operation that blocks new edits/recordings, flushes every pending editor save, awaits durable completion, rechecks backend recording activity, and only then permits installation. A failed save must keep the app open.

### 2. There is no automatic pre-migration backup or restore flow

Initialization opens the original database and directly calls migrate.run(). No backup, integrity check, or restore operation wraps it. Transactions protect against partial SQL execution but do not undo a logically wrong migration that successfully commits.

Evidence: [db.rs](../../kiminola/src-tauri/src/db.rs#L35).

Recommended fix: before pending migrations modify an existing database, create a versioned SQLite-consistent snapshot, verify it, and retain it outside the installation directory. Use SQLite's [backup API or VACUUM INTO](https://sqlite.org/backup.html), with write exclusion coordinated across the app. Do not blindly copy a live database file. Stop before migration if the required backup cannot be completed.

### 3. Migration failure has no dedicated recovery screen

The warm-up logs an error. Later commands retry database initialization and return errors, but there is no startup recovery view with the database path, failed migration, backup selection, and explicit restore/retry choices. The pool is withheld, which avoids normal application queries using a partially upgraded schema, but the user has little guidance.

Evidence: [warm-up failure](../../kiminola/src-tauri/src/db.rs#L1633), [initialization](../../kiminola/src-tauri/src/db.rs#L55), [layout error handling](../../kiminola/src/routes/+layout.svelte#L48).

Recommended fix: expose a database startup state, gate normal UI on successful migration, preserve the failing database for diagnosis, and provide controlled recovery. Never replace an unreadable database with an empty library.

### 4. Reinstalling an older app is not a database rollback

An older binary missing migrations recorded by a newer binary is rejected by SQLx's history validation. No down-migration files or application restore procedure are present. The release policy already calls for a higher patch version after a bad release.

Evidence: [SQLx runner](https://raw.githubusercontent.com/launchbadge/sqlx/v0.8.6/sqlx-core/src/migrate/migrator.rs), [release policy](../RELEASING.md#L72).

Keep a compatible backup for recovery and prefer a forward fix. Restoring an old backup would also discard newer writes, so it requires an explicit recovery decision.

## Verification performed

Executed the repository's SQL scripts using Python 3.13 and SQLite 3.49.1, entirely in memory:

- Constructed each schema baseline from 1 through 9, seeded populated fixtures, and applied remaining scripts through schema 9.
- Compared original columns and content for meetings, notes, transcript segments, settings, custom templates, and drafts where available at the baseline. Included enhanced notes and recording recovery data.
- All nine cases preserved fixtures and passed integrity_check and foreign_key_check. Each fixture meeting was found through FTS search.
- Verified migration 0007 backfills an unassigned older meeting into Personal.
- Injected a SQL error after a schema and data change in a transaction; rollback preserved the original title and removed the added column.

These are synthetic SQL checks using the current migration files. They do not run the SQLx migrator, verify historical release-file checksums, simulate power loss or disk exhaustion, or exercise an installed update.

Existing database tests initialize a fresh database using the full migration set. The updater policy tests check route matching, progress, and release-note formatting. The reviewed CI/release workflows do not automate populated historical upgrade/restore tests. See [database tests](../../kiminola/src-tauri/src/db.rs#L1644), [update tests](../../kiminola/tests/update-policy.test.ts), [CI](../../.github/workflows/ci.yml), and [release workflow](../../.github/workflows/release.yml).

Before calling updates fully validated, run actual SQLx upgrade fixtures, failure/restore tests, and signed installed N-to-N+1 updates on native Windows x64 and ARM64. Compare semantic data and model hashes after restart, including pending edits and interrupted recording drafts. The [release runbook](../RELEASING.md#L51) requires installed validation but is not evidence of a completed run.

## Implementation follow-up

The subsequent fix adds pending-note flushing and app-command draining to the actual updater controller, plus a native barrier that blocks new recordings and closes the database pool before installer handoff. Failed saves leave the downloaded update retryable. Notes use a queue per note so pending edits survive navigation.

The new [database safety module](../../kiminola/src-tauri/src/db_safety.rs) owns startup, verified pre-migration snapshots, history validation, exclusive database access, and startup-only restore. Restore upgrades a staged backup before moving originals, archives the original database and sidecars, and leaves a durable interruption marker until replacement succeeds. Normal app use is gated by a [recovery screen](../../kiminola/src/lib/components/DatabaseGate.svelte); shortcut initialization no longer aborts the process on a database error. Recovery releases database ownership before restarting so the replacement process can open it.

Regression coverage now uses the actual SQLx runner on populated schema versions 1 through 9, tests backup failure before mutation, failed-SQL rollback, committed WAL data, invalid history, corrupted backup rejection, restore preservation, interrupted restore, and owner/update locks. Frontend tests drive the real update controller with mocked Tauri boundaries. CI and release builds run these database tests on Windows x64; native ARM64 and signed installed-update checks remain explicit release checks. No published SQL migration was rewritten.

Local x64 validation initially found ARM64 DLLs in the existing x64 runtime cache. The test run used an isolated extraction of the existing x64 archive after checking PE machine types. This workaround does not certify the other cached runtime directories or any already-built installers.

