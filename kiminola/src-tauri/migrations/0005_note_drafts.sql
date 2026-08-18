-- Standalone notes created from a meeting-presence prompt.
-- A draft becomes part of a meeting only after the user explicitly starts and
-- saves a recording; it is never removed automatically.

CREATE TABLE note_drafts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    raw_markdown TEXT NOT NULL DEFAULT '',
    meeting_id INTEGER REFERENCES meetings (id)
);

CREATE INDEX note_drafts_updated_at_idx
    ON note_drafts (updated_at DESC, id DESC);
