-- Kiminola initial schema (SPEC.md §6).
-- templates and search_index (FTS5) land with their own tickets.

CREATE TABLE spaces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    parent_id INTEGER REFERENCES spaces (id),
    created_at TEXT NOT NULL
);

CREATE TABLE meetings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    space_id INTEGER REFERENCES spaces (id),
    created_at TEXT NOT NULL,
    duration_seconds INTEGER NOT NULL DEFAULT 0
);

-- start_ms/end_ms stay NULL at MVP: sherpa-onnx lane results carry no word
-- timings yet, so there is nothing honest to put in them.
CREATE TABLE transcript_segments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id INTEGER NOT NULL REFERENCES meetings (id),
    channel TEXT NOT NULL CHECK (channel IN ('you', 'others')),
    start_ms INTEGER,
    end_ms INTEGER,
    text TEXT NOT NULL
);

CREATE TABLE notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id INTEGER NOT NULL UNIQUE REFERENCES meetings (id),
    raw_markdown TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Every recording files into the default space until space management ships.
INSERT INTO spaces (name, created_at)
VALUES ('Personal', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
