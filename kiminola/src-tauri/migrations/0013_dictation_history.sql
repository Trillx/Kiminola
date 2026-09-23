-- Only completed final text belongs here. Recovery stays in process memory.
CREATE TABLE dictation_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX dictation_history_created_at ON dictation_history(created_at);
