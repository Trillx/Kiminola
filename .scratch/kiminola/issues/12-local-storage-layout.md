# Local storage layout

Type: grilling
Status: resolved

## Question

Where and how does Kiminola persist data on disk? The scope cut (ticket 09) now requires a meeting library with FTS5 search, inline transcript editing, custom templates, and Markdown export. Decide:

- Storage engine: SQLite (rusqlite or sqlx) with FTS5 for everything, vs a hybrid (DB + plain .md files for notes)
- Data location: `%APPDATA%/Kiminola` vs a user-visible folder
- How the Spaces tree is represented
- How Markdown export interacts (render-on-export vs notes already living as .md files)

Mostly a technical call — bring a recommendation, confirm with the user.

## Resolution

Resolved on 2026-08-13 after a one-question-at-a-time grilling session. Future growth was the deciding lens.

### Decisions

- **Storage engine — SQLite + FTS5, single store.** Meetings, transcript segments, notes, templates, Spaces, and settings all live in one database file. An FTS5 virtual table powers full-text search over titles, notes, and transcripts.
- **Database layer — sqlx + SQLite with migrations.** sqlx fits Tauri's async command model and makes a future backend swap (e.g., for cloud sync) cheaper than rusqlite. `libsqlite3-sys` bundled keeps the build self-contained. Migrations are managed from day one.
- **Data location — `%LOCALAPPDATA%\Kiminola\data`.** User data is irreplaceable and can grow large, so it stays local (not roaming `AppData`). Models are already in `%LOCALAPPDATA%\Kiminola\models` (ticket 07), so the app has one root under `Kiminola\`.
- **Spaces tree — adjacency list.** A `spaces` table with `id`, `name`, `parent_id`, `created_at`. Meetings reference `space_id`. Clean rename/move and arbitrary nesting for future growth.
- **Markdown export — render with YAML frontmatter.** Notes are Markdown text in the database; export writes a `.md` file with a concise YAML frontmatter block (title, date, space, duration, etc.).

### Consequences

- One `.db` file to back up, migrate, or move. Cross-machine sync is a post-MVP feature.
- FTS5 search is native and fast for the expected corpus size.
- The schema is normalized and migration-ready; adding future entities (e.g., tags, sharing, sync clocks) does not require re-architecture.
